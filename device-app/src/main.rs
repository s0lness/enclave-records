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
    pub mod info;
    pub mod pair;
    pub mod press;
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Instruction {
    GetInfo,
    Collection,
    ArtTest { stage: u8 },
    LibraryPreview { count: u8 },
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
}

impl TryFrom<ApduHeader> for Instruction {
    type Error = AppSW;

    fn try_from(value: ApduHeader) -> Result<Self, Self::Error> {
        match (value.ins, value.p1, value.p2) {
            (0x01, 0, 0) => Ok(Instruction::GetInfo),
            (0x02, 0, 0) => Ok(Instruction::Collection),
            (0x61, stage, 0) => Ok(Instruction::ArtTest { stage }),
            (0x63, count, 0) => Ok(Instruction::LibraryPreview { count }),
            (0x65, 0, 0) => Ok(Instruction::CardPreview),
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
            (0x40, part @ (0 | 1), 0) => Ok(Instruction::GetBundle { part }),
            (0x41, 0, 0) => Ok(Instruction::Challenge),
            (0x50, 0, 0) => Ok(Instruction::ResetMaster),
            (0x01 | 0x02 | 0x10 | 0x21..=0x25 | 0x30..=0x34 | 0x40 | 0x41 | 0x50 | 0x61 | 0x62 | 0x63 | 0x64 | 0x65, _, _) => {
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
            // UI-only: they cover the library, so it must be repainted under
            // them, but they change nothing (SAS confirmation, the record card,
            // the art-test probe).
            | Instruction::PairSas
            | Instruction::Collection
            | Instruction::ArtTest { .. }
            | Instruction::LibraryPreview { .. }
            | Instruction::CardPreview
    )
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
    let decoded = command.decode::<Instruction>();
    let Ok(ins) = decoded else {
        let _ = comm.send(&[], decoded.unwrap_err());
        return;
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
        let decoded = command.decode::<Instruction>();
        let Ok(ins) = decoded else {
            let _ = comm.send(&[], decoded.unwrap_err());
            continue;
        };
        let ui_gated = matches!(
            ins,
            Instruction::Cut
                | Instruction::Collection
                | Instruction::PairSas
                | Instruction::PressOffer
                | Instruction::PressAccept
                | Instruction::ResetMaster
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
        Instruction::ArtTest { stage } => handlers::collection::handler_art_test(command, stage),
        Instruction::LibraryPreview { count } => {
            handlers::collection::handler_library_preview(command, count)
        }
        Instruction::CardPreview => handlers::collection::handler_card_preview(command),
        Instruction::SetArt { slot } => handlers::art::handler_set_art(command, slot),
        Instruction::GetArt { chunk, slot } => handlers::art::handler_get_art(command, chunk, slot),
        Instruction::Cut => handlers::cut::handler_cut(command),
        Instruction::ResetMaster => handlers::cut::handler_reset_master(command),
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
    }
}
