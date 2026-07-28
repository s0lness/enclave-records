//! Development provisioning: hand a device a pressing it did not receive
//! through a ceremony.
//!
//! Staging a scene needs copies with plausible numbers, and the honest path to
//! copy #15 is fourteen earlier presses to fourteen distinct devices, because a
//! pressing binds to its recipient and a device holds one at a time. With two
//! Flex on a desk that is unreachable, so the relay acts as the master: it
//! holds the album key, signs the certificates, and pushes the result here.
//!
//! This does not weaken the model. Provisioning only ever *adds* a holding,
//! never removes one: a device that already holds a pressing is refused, so
//! "bound to this device forever" stays literally true. The certificates are
//! verified exactly as `PRESS_ACCEPT` verifies them, and a pressing whose
//! `recvpub` is not this device is rejected, so nothing unsigned or misaddressed
//! can be planted. What is fictional is only who pressed it.

use crate::certs::{parse_album_cert, parse_pressing_cert, ALBUM_CERT_LEN, PRESSING_CERT_LEN};
use crate::crypto;
use crate::session::Session;
use crate::state::Store;
use crate::AppSW;
use ledger_device_sdk::io::{Command, CommandResponse};

/// PROVISION_ALBUM: data = AlbumCert. Verified and staged for the pressing
/// step, which cannot carry both certificates in one 255-byte APDU. Uses the
/// session's staging slot but requires no pairing: there is no peer here, the
/// relay is the authority by construction.
pub fn handler_provision_album<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    let data = command.get_data();
    if data.len() != ALBUM_CERT_LEN {
        return Err(AppSW::WrongApduLength);
    }
    parse_album_cert(data)?;
    session.staged_album.copy_from_slice(data);
    session.staged_album_valid = true;
    Ok(command.into_response())
}

/// PROVISION_PRESSING: data = PressingCert. Same chain checks as PRESS_ACCEPT,
/// then stores. Refuses to overwrite an existing holding.
pub fn handler_provision_pressing<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    if !session.staged_album_valid {
        return Err(AppSW::BadState);
    }
    let data = command.get_data();
    if data.len() != PRESSING_CERT_LEN {
        return Err(AppSW::WrongApduLength);
    }
    let mut cert_buf = [0u8; PRESSING_CERT_LEN];
    cert_buf.copy_from_slice(data);

    let staged_album = session.staged_album;
    let album = parse_album_cert(&staged_album)?;
    let pressing = parse_pressing_cert(&cert_buf, &album.albpub)?;

    let album_id = crypto::sha256(&[&album.albpub])?;
    if !crypto::mac_eq(&album_id, &pressing.album_id) {
        return Err(AppSW::BadCert);
    }
    if pressing.edition != album.edition {
        return Err(AppSW::BadCert);
    }
    let mut nvm = Store::get()?;
    if pressing.recvpub != nvm.dev_pub {
        return Err(AppSW::BadCert);
    }
    if nvm.has_pressing == 1 {
        return Err(AppSW::BadState);
    }

    nvm.has_pressing = 1;
    nvm.pressing_cert = cert_buf;
    nvm.pressing_album_cert = staged_album;
    Store::put(&nvm);
    session.staged_album_valid = false;

    Ok(command.into_response())
}
