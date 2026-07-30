use crate::crypto;
use crate::state::Store;
use crate::AppSW;
use ledger_device_sdk::io::{Command, CommandResponse};

/// GET_BUNDLE: p1=0 -> PressingCert, p1=1 -> its AlbumCert. Public data.
pub fn handler_get_bundle(command: Command<'_>, part: u8) -> Result<CommandResponse<'_>, AppSW> {
    let nvm = Store::get()?;
    if nvm.has_pressing != 1 {
        return Err(AppSW::NoPressing);
    }
    let mut response = command.into_response();
    match part {
        0 => {
            response.append(&nvm.pressing_cert)?;
        }
        1 => {
            response.append(&nvm.pressing_album_cert)?;
        }
        _ => return Err(AppSW::WrongP1P2),
    }
    Ok(response)
}

/// CHALLENGE: data = nonce(32). Returns sig_len(1) || DER signature by the
/// held copy's **bearer key** over SHA256("presse-verify" || nonce).
///
/// Signed with the bearer key and not the device key because that is what the
/// certificate names: possession of the copy is possession of that scalar, and
/// a device that has handed the copy on can no longer answer. Fail closed with
/// no pressing held: an answer from the device key would prove only that a
/// Ledger is on the other end, which is not what a verifier is asking.
///
/// A copy promised to a recipient but not yet acknowledged is equally silent.
/// It is still in flash, but its owner has given it away and only the delivery
/// is outstanding; answering for it would be claiming something already sold.
pub fn handler_challenge(command: Command<'_>) -> Result<CommandResponse<'_>, AppSW> {
    let data = command.get_data();
    if data.len() != 32 {
        return Err(AppSW::WrongApduLength);
    }
    let nvm = Store::get()?;
    if !nvm.holds_usable_pressing() {
        return Err(AppSW::NoPressing);
    }
    let mut msg = [0u8; 13 + 32];
    msg[..13].copy_from_slice(b"presse-verify");
    msg[13..].copy_from_slice(data);
    let (sig, sig_len) = crypto::sign_payload(&nvm.pressing_priv, &msg)?;
    let mut response = command.into_response();
    response.append(&[sig_len])?;
    response.append(&sig[..sig_len as usize])?;
    Ok(response)
}
