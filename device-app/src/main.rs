/*****************************************************************************
 *   presse - silicon-enforced finite editions on Ledger Flex.
 *
 *  Licensed under the Apache License, Version 2.0 (the "License");
 *  you may not use this file except in compliance with the License.
 *  You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *****************************************************************************/

#![no_std]
#![no_main]

mod certs;
mod crypto;
mod session;
mod sleeve;
mod state;
mod wordlist;

mod app_ui {
    pub mod library;
    pub mod menu;
}
mod handlers {
    pub mod art;
    pub mod collection;
    pub mod cut;
    pub mod give;
    pub mod info;
    pub mod pair;
    pub mod press;
    pub mod provision;
    pub mod verify;
}

#[cfg(not(any(target_os = "stax", target_os = "flex")))]
use app_ui::menu::ui_menu_main;
use ledger_device_sdk::io::{self, init_comm, ApduHeader, Command, Reply, StatusWords};
use session::Session;

ledger_device_sdk::set_panic!(ledger_device_sdk::exiting_panic);

extern crate alloc;

ledger_device_sdk::define_comm!(COMM);

/// Application status words. Security rule: fail closed, every unexpected
/// condition maps to an explicit error, never to a default value.
#[repr(u16)]
#[derive(Clone, Copy, PartialEq)]
pub enum AppSW {
    Deny = 0x6985,
    WrongP1P2 = 0x6A86,
    InsNotSupported = 0x6D00,
    ClaNotSupported = 0x6E00,
    CommError = 0x6F00,
    BadState = 0xB101,
    BadMac = 0xB102,
    BadCert = 0xB103,
    SoldOut = 0xB104,
    NoMaster = 0xB105,
    HasMaster = 0xB106,
    CryptoFail = 0xB107,
    NoPressing = 0xB108,
    TooManyAttempts = 0xB109,
    /// A promise can no longer be taken back: the sealed bearer key has already
    /// left this device, so the copy may exist elsewhere and un-promising it
    /// here would be a double-spend primitive rather than a repair.
    KeyFlown = 0xB10A,
    WrongApduLength = StatusWords::BadLen as u16,
    Ok = 0x9000,
}

impl From<AppSW> for Reply {
    fn from(sw: AppSW) -> Reply {
        Reply(sw as u16)
    }
}

impl From<io::CommError> for AppSW {
    fn from(_e: io::CommError) -> Self {
        AppSW::CommError
    }
}

/// APDU instructions. See docs/protocol.md for the ceremony flows.
///
/// Deliberately not `Debug`: a derived one bakes all forty variant names and
/// the formatting machinery to print them into flash, and nothing on the device
/// ever prints an instruction. Decode errors are matched, never `unwrap_err`ed.
#[derive(Clone, Copy, PartialEq)]
pub enum Instruction {
    GetInfo,
    Collection,
    /// Development screen probes, compiled in only under their own features.
    #[cfg(feature = "artprobe")]
    ArtTest { stage: u8 },
    #[cfg(feature = "uiprobe")]
    LibraryPreview { count: u8 },
    #[cfg(feature = "uiprobe")]
    CardPreview,
    SetArt { slot: u8 },
    GetArt { chunk: u8, slot: u8 },
    Cut,
    PairCommit,
    PairRespond,
    PairReveal,
    PairFinish,
    PairSas,
    GetAlbum,
    PressRequest,
    PressOffer,
    PressLoadAlbum,
    PressAccept,
    GetBundle { part: u8 },
    Challenge,
    ResetMaster,
    ProvisionAlbum,
    ProvisionPressing,
    /// Handing a held copy on. The giver reads out what it holds and signs the
    /// handover, the taker stages it and confirms first, then the giver commits
    /// and releases the key, and the taker's receipt finally frees the giver.
    GiveAlbum,
    GivePressing,
    GiveChain,
    GiveHandover,
    /// Phase 2 on the giver, in two halves: the commitment (`release: false`,
    /// UI-gated, changes state and returns nothing) and the release of the
    /// sealed bearer key (`release: true`). Splitting them is what makes
    /// "promised but the key never left" a state a human can still take back.
    GiveOffer { release: bool },
    GiveCancel,
    GiveFinish,
    TakeAlbum,
    TakePressing,
    TakeChain,
    TakeHandover,
    TakeConfirm,
    TakeAccept,
    TakeReceipt,
}

impl TryFrom<ApduHeader> for Instruction {
    type Error = AppSW;

    fn try_from(value: ApduHeader) -> Result<Self, Self::Error> {
        match (value.ins, value.p1, value.p2) {
            (0x01, 0, 0) => Ok(Instruction::GetInfo),
            (0x02, 0, 0) => Ok(Instruction::Collection),
            #[cfg(feature = "artprobe")]
            (0x61, stage, 0) => Ok(Instruction::ArtTest { stage }),
            #[cfg(feature = "artprobe")]
            (0x61, _, _) => Err(AppSW::WrongP1P2),
            #[cfg(feature = "uiprobe")]
            (0x63, count, 0) => Ok(Instruction::LibraryPreview { count }),
            #[cfg(feature = "uiprobe")]
            (0x65, 0, 0) => Ok(Instruction::CardPreview),
            #[cfg(feature = "uiprobe")]
            (0x63 | 0x65, _, _) => Err(AppSW::WrongP1P2),
            (0x62, slot @ (0 | 1), 0) => Ok(Instruction::SetArt { slot }),
            (0x64, chunk, slot @ (0 | 1)) => Ok(Instruction::GetArt { chunk, slot }),
            (0x10, 0, 0) => Ok(Instruction::Cut),
            (0x21, 0, 0) => Ok(Instruction::PairCommit),
            (0x22, 0, 0) => Ok(Instruction::PairRespond),
            (0x23, 0, 0) => Ok(Instruction::PairReveal),
            (0x24, 0, 0) => Ok(Instruction::PairFinish),
            (0x25, 0, 0) => Ok(Instruction::PairSas),
            (0x30, 0, 0) => Ok(Instruction::GetAlbum),
            (0x31, 0, 0) => Ok(Instruction::PressRequest),
            (0x32, 0, 0) => Ok(Instruction::PressOffer),
            (0x33, 0, 0) => Ok(Instruction::PressLoadAlbum),
            (0x34, 0, 0) => Ok(Instruction::PressAccept),
            (0x40, part @ (0 | 1 | 2), 0) => Ok(Instruction::GetBundle { part }),
            (0x41, 0, 0) => Ok(Instruction::Challenge),
            (0x50, 0, 0) => Ok(Instruction::ResetMaster),
            (0x66, 0, 0) => Ok(Instruction::ProvisionAlbum),
            (0x67, 0, 0) => Ok(Instruction::ProvisionPressing),
            (0x70, 0, 0) => Ok(Instruction::GiveAlbum),
            (0x71, 0, 0) => Ok(Instruction::GivePressing),
            (0x72, 0, 0) => Ok(Instruction::GiveChain),
            (0x73, 0, 0) => Ok(Instruction::GiveOffer { release: false }),
            (0x73, 1, 0) => Ok(Instruction::GiveOffer { release: true }),
            (0x74, 0, 0) => Ok(Instruction::TakeAlbum),
            (0x75, 0, 0) => Ok(Instruction::TakePressing),
            (0x76, 0, 0) => Ok(Instruction::TakeChain),
            (0x77, 0, 0) => Ok(Instruction::TakeAccept),
            (0x78, 0, 0) => Ok(Instruction::GiveHandover),
            (0x79, 0, 0) => Ok(Instruction::GiveFinish),
            (0x7A, 0, 0) => Ok(Instruction::TakeHandover),
            (0x7B, 0, 0) => Ok(Instruction::TakeConfirm),
            (0x7C, 0, 0) => Ok(Instruction::TakeReceipt),
            (0x7D, 0, 0) => Ok(Instruction::GiveCancel),
            (0x01 | 0x02 | 0x10 | 0x21..=0x25 | 0x30..=0x34 | 0x40 | 0x41 | 0x50 | 0x62 | 0x64 | 0x66 | 0x67 | 0x70..=0x7D, _, _) => {
                Err(AppSW::WrongP1P2)
            }
            (_, _, _) => Err(AppSW::InsNotSupported),
        }
    }
}

#[no_mangle]
extern "C" fn sample_main(_arg0: u32) {
    let comm = init_comm(&COMM);
    comm.set_expected_cla(0xb5);
    let mut session = Session::new();

    #[cfg(any(target_os = "stax", target_os = "flex"))]
    library_main(comm, &mut session);
    #[cfg(not(any(target_os = "stax", target_os = "flex")))]
    legacy_home_main(comm, &mut session);
}

/// Whether serving `ins` warrants repainting the library afterwards.
///
/// Two things can make the drawn library stale: a command that changed the
/// records it lists, or a command that drew its own screen over it and must
/// have the library restored underneath. Both sets are exactly the UI-gated
/// commands here (every state change on this device is behind a confirmation),
/// so the rule is: repaint after a UI-gated command, never after a pure
/// data-plane one. This is what keeps a bulk sleeve transfer (~50 SET_ART
/// chunks) from repainting the whole screen once per chunk.
fn warrants_library_redraw(ins: Instruction) -> bool {
    matches!(
        ins,
        // State-changing, and each draws a confirmation over the library.
        Instruction::Cut
            | Instruction::PressOffer
            | Instruction::PressAccept
            | Instruction::ResetMaster
            | Instruction::ProvisionPressing
            | Instruction::GiveOffer { .. }
            | Instruction::GiveCancel
            | Instruction::GiveFinish
            | Instruction::TakeConfirm
            | Instruction::TakeAccept
            // UI-only: they cover the library, so it must be repainted under
            // them, but they change nothing (SAS confirmation, the record card).
            | Instruction::PairSas
            | Instruction::Collection
    ) || probe_redraw(ins)
}

/// The development probes draw over the library too, so they warrant the same
/// repaint. Split out so the production match above carries no probe seam, and
/// so it compiles to a constant `false` in a shipped build.
fn probe_redraw(ins: Instruction) -> bool {
    #[cfg(feature = "artprobe")]
    if matches!(ins, Instruction::ArtTest { .. }) {
        return true;
    }
    #[cfg(feature = "uiprobe")]
    if matches!(
        ins,
        Instruction::LibraryPreview { .. } | Instruction::CardPreview
    ) {
        return true;
    }
    let _ = ins;
    false
}

/// Serve exactly one APDU: decode, dispatch, reply. A command that draws its own
/// screen (see [`warrants_library_redraw`]) also owns the whole display and RAM
/// budget while it runs, so the held library is dropped *before* dispatch: the
/// record card composes a wide bitmap in RAM and needs the heap the library's
/// thumbnails would otherwise fragment. Data-plane commands leave the library
/// standing, so a bulk sleeve transfer still does not repaint per chunk.
#[cfg(any(target_os = "stax", target_os = "flex"))]
fn serve_one_command(
    comm: &mut io::Comm,
    session: &mut Session,
    library: &mut Option<handlers::collection::Library>,
) {
    let command = comm.next_command();
    let ins = match command.decode::<Instruction>() {
        Ok(ins) => ins,
        Err(reply) => {
            let _ = comm.send(&[], reply);
            return;
        }
    };
    if warrants_library_redraw(ins) {
        *library = None;
    }
    match handle_apdu(command, ins, session) {
        Ok(reply) => {
            let _ = reply.send(AppSW::Ok);
        }
        Err(sw) => {
            let _ = comm.send(&[], sw);
        }
    }
}

/// The library is the landing screen: it draws, handles taps (open a record,
/// the info page, quit), and steps aside the moment an APDU is pending so the
/// main loop can serve the command.
///
/// The drawn library is held across served commands and repainted only when a
/// command could have changed what it shows (see [`warrants_library_redraw`]).
/// A bulk sleeve transfer is dozens of data-plane APDUs; repainting per chunk
/// flickers the screen and drags the transfer out on real hardware, so the
/// library yields to each command but repaints at most once, after the burst.
/// A cut or a press still repaints, so the new or updated record appears.
#[cfg(any(target_os = "stax", target_os = "flex"))]
fn library_main(comm: &mut io::Comm, session: &mut Session) {
    use handlers::collection::{show_record_card, Library, LibraryAction, RecordKind};

    let mut library: Option<Library> = None;
    loop {
        // (Re)draw the library only when we have no live screen: a fresh start,
        // or after a command/interaction that invalidated the last one.
        if library.is_none() {
            match Library::draw() {
                Ok(l) => library = Some(l),
                // Fail closed: on a state error, don't spin on a broken screen;
                // serve the host and try to redraw next time round.
                Err(_) => {
                    serve_one_command(comm, session, &mut library);
                    continue;
                }
            }
        }

        match library.as_ref().unwrap().wait() {
            LibraryAction::Apdu => {
                // Serve the command; a screen-drawing command drops the library
                // first (freeing its heap and screen), a data-plane one leaves
                // it standing so a burst does not repaint per chunk.
                serve_one_command(comm, session, &mut library);
            }
            LibraryAction::Quit => ledger_device_sdk::exit_app(0),
            LibraryAction::OpenMaster => {
                library = None; // the record card draws over the library
                let _ = show_record_card(RecordKind::Master);
            }
            LibraryAction::OpenPressing => {
                library = None; // the record card draws over the library
                let _ = show_record_card(RecordKind::Pressing);
            }
            LibraryAction::Redraw => library = None,
        }
    }
}

/// Fallback landing loop for devices without the touch library screen
/// (the Nanos): the original home + collection button.
#[cfg(not(any(target_os = "stax", target_os = "flex")))]
fn legacy_home_main(comm: &mut io::Comm, session: &mut Session) {
    let mut home = ui_menu_main(comm);
    home.show_and_return();
    loop {
        let command = comm.next_command();
        let ins = match command.decode::<Instruction>() {
            Ok(ins) => ins,
            Err(reply) => {
                let _ = comm.send(&[], reply);
                continue;
            }
        };
        let ui_gated = matches!(
            ins,
            Instruction::Cut
                | Instruction::Collection
                | Instruction::PairSas
                | Instruction::PressOffer
                | Instruction::PressAccept
                | Instruction::ResetMaster
                | Instruction::GiveOffer { .. }
                | Instruction::GiveCancel
                | Instruction::GiveFinish
                | Instruction::TakeConfirm
                | Instruction::TakeAccept
        );
        match handle_apdu(command, ins, session) {
            Ok(reply) => {
                let _ = reply.send(AppSW::Ok);
            }
            Err(sw) => {
                let _ = comm.send(&[], sw);
            }
        }
        if ui_gated {
            home = ui_menu_main(comm);
            home.show_and_return();
        }
    }
}

fn handle_apdu<'a>(
    command: Command<'a>,
    ins: Instruction,
    session: &mut Session,
) -> Result<io::CommandResponse<'a>, AppSW> {
    match ins {
        Instruction::GetInfo => handlers::info::handler_get_info(command),
        Instruction::Collection => handlers::collection::handler_collection(command),
        #[cfg(feature = "artprobe")]
        Instruction::ArtTest { stage } => handlers::collection::handler_art_test(command, stage),
        #[cfg(feature = "uiprobe")]
        Instruction::LibraryPreview { count } => {
            handlers::collection::handler_library_preview(command, count)
        }
        #[cfg(feature = "uiprobe")]
        Instruction::CardPreview => handlers::collection::handler_card_preview(command),
        Instruction::SetArt { slot } => handlers::art::handler_set_art(command, slot),
        Instruction::GetArt { chunk, slot } => handlers::art::handler_get_art(command, chunk, slot),
        Instruction::Cut => handlers::cut::handler_cut(command),
        Instruction::ResetMaster => handlers::cut::handler_reset_master(command),
        Instruction::ProvisionAlbum => handlers::provision::handler_provision_album(command, session),
        Instruction::ProvisionPressing => {
            handlers::provision::handler_provision_pressing(command, session)
        }
        Instruction::PairCommit => handlers::pair::handler_commit(command, session),
        Instruction::PairRespond => handlers::pair::handler_respond(command, session),
        Instruction::PairReveal => handlers::pair::handler_reveal(command, session),
        Instruction::PairFinish => handlers::pair::handler_finish(command, session),
        Instruction::PairSas => handlers::pair::handler_sas(command, session),
        Instruction::GetAlbum => handlers::press::handler_get_album(command, session),
        Instruction::PressRequest => handlers::press::handler_press_request(command, session),
        Instruction::PressOffer => handlers::press::handler_press_offer(command, session),
        Instruction::PressLoadAlbum => handlers::press::handler_press_load_album(command, session),
        Instruction::PressAccept => handlers::press::handler_press_accept(command, session),
        Instruction::GetBundle { part } => handlers::verify::handler_get_bundle(command, part),
        Instruction::Challenge => handlers::verify::handler_challenge(command),
        Instruction::GiveAlbum => handlers::give::handler_give_album(command, session),
        Instruction::GivePressing => handlers::give::handler_give_pressing(command, session),
        Instruction::GiveChain => handlers::give::handler_give_chain(command, session),
        Instruction::GiveHandover => handlers::give::handler_give_handover(command, session),
        Instruction::GiveOffer { release } => {
            handlers::give::handler_give_offer(command, session, release)
        }
        Instruction::GiveCancel => handlers::give::handler_give_cancel(command),
        Instruction::GiveFinish => handlers::give::handler_give_finish(command, session),
        Instruction::TakeAlbum => handlers::give::handler_take_album(command, session),
        Instruction::TakePressing => handlers::give::handler_take_pressing(command, session),
        Instruction::TakeChain => handlers::give::handler_take_chain(command, session),
        Instruction::TakeHandover => handlers::give::handler_take_handover(command, session),
        Instruction::TakeConfirm => handlers::give::handler_take_confirm(command, session),
        Instruction::TakeAccept => handlers::give::handler_take_accept(command, session),
        Instruction::TakeReceipt => handlers::give::handler_take_receipt(command, session),
    }
}
