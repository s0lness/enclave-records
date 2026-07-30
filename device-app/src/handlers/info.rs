use crate::state::Store;
use crate::AppSW;
use ledger_device_sdk::io::{Command, CommandResponse};

const FLAG_HAS_MASTER: u8 = 1 << 0;
const FLAG_HAS_PRESSING: u8 = 1 << 1;
/// The held copy is promised to a recipient and awaiting its receipt. A relay
/// resuming an interrupted transfer needs to know this without guessing: it is
/// the difference between starting a give and finishing one.
const FLAG_COMMITTED: u8 = 1 << 2;
/// ...and the sealed key has already left, so the promise can no longer be
/// taken back. Reported beside the commitment rather than instead of it: a
/// relay resuming a transfer treats both alike, and only a relay offering to
/// cancel one needs to tell them apart.
const FLAG_KEY_FLOWN: u8 = 1 << 3;

/// GET_INFO: [flags][devpub 65][edition u16][counter u16][title_len][title 32]
pub fn handler_get_info(command: Command<'_>) -> Result<CommandResponse<'_>, AppSW> {
    let nvm = Store::get()?;
    let mut flags = 0u8;
    if nvm.has_master == 1 {
        flags |= FLAG_HAS_MASTER;
    }
    if nvm.has_pressing == 1 {
        flags |= FLAG_HAS_PRESSING;
    }
    if nvm.committed != 0 {
        flags |= FLAG_COMMITTED;
    }
    if nvm.committed == 2 {
        flags |= FLAG_KEY_FLOWN;
    }
    let mut response = command.into_response();
    response.append(&[flags])?;
    response.append(&nvm.dev_pub)?;
    response.append(&nvm.edition.to_le_bytes())?;
    response.append(&nvm.counter.to_le_bytes())?;
    response.append(&[nvm.title_len])?;
    response.append(&nvm.title)?;
    Ok(response)
}
