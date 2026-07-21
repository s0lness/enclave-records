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
mod state;
mod wordlist;

mod app_ui {
    pub mod menu;
}
mod handlers {
    pub mod collection;
    pub mod cut;
    pub mod info;
    pub mod pair;
    pub mod press;
    pub mod verify;
}

use app_ui::menu::ui_menu_main;
use ledger_device_sdk::io::{self, init_comm, ApduHeader, Comm, Command, Reply, StatusWords};
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
            (0x01 | 0x02 | 0x10 | 0x21..=0x25 | 0x30..=0x34 | 0x40 | 0x41 | 0x50, _, _) => {
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

    let mut home = ui_menu_main(comm);
    home.show_and_return();

    let mut session = Session::new();

    loop {
        let command = comm.next_command();
        let decoded = command.decode::<Instruction>();
        let Ok(ins) = decoded else {
            let _ = comm.send(&[], decoded.unwrap_err());
            continue;
        };

        let status = match handle_apdu(command, ins, &mut session) {
            Ok(reply) => {
                let _ = reply.send(AppSW::Ok);
                AppSW::Ok
            }
            Err(sw) => {
                let _ = comm.send(&[], sw);
                sw
            }
        };

        // UI-gated commands leave their review screen up; restore home.
        let ui_gated = matches!(
            ins,
            Instruction::Cut
                | Instruction::Collection
                | Instruction::PairSas
                | Instruction::PressOffer
                | Instruction::PressAccept
                | Instruction::ResetMaster
        );
        if ui_gated {
            let _ = status;
            // Ceremonies change what the device holds; rebuild the home so
            // the idle screen always tells the current story.
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
