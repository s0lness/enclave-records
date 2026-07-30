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
//! never removes one: a device that already holds a pressing is refused. The
//! certificates are verified exactly as `PRESS_ACCEPT` verifies them, including
//! that the bearer key handed in derives the `holderpub` the album key signed,
//! so nothing unsigned or mismatched can be planted. What is fictional is only
//! who pressed it.
//!
//! **This is a development path and its bearer key crosses the USB cable in
//! the clear.** There is no pairing here and so no session key to seal it with:
//! the relay is playing the master, and anything watching that cable owns the
//! copy. A real press seals the key against the paired channel; this one cannot
//! and must never be reached other than by a scene being staged on a desk.

use crate::certs::{parse_album_cert, parse_pressing_cert, ALBUM_CERT_LEN, PRESSING_CERT_LEN};
use crate::crypto;
use crate::handlers::press::BEARER_KEY_LEN;
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

/// PROVISION_PRESSING: data = PressingCert(178) || bearer key(32) = 210, one
/// frame. Same chain checks as PRESS_ACCEPT, then stores. Refuses to overwrite
/// an existing holding.
pub fn handler_provision_pressing<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    if !session.staged_album_valid {
        return Err(AppSW::BadState);
    }
    let data = command.get_data();
    if data.len() != PRESSING_CERT_LEN + BEARER_KEY_LEN {
        return Err(AppSW::WrongApduLength);
    }
    let mut cert_buf = [0u8; PRESSING_CERT_LEN];
    cert_buf.copy_from_slice(&data[..PRESSING_CERT_LEN]);
    let mut bearer_priv = [0u8; BEARER_KEY_LEN];
    bearer_priv.copy_from_slice(&data[PRESSING_CERT_LEN..]);

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
    if crypto::pubkey_of(&bearer_priv)? != pressing.holderpub {
        return Err(AppSW::BadCert);
    }
    let mut nvm = Store::get()?;
    if nvm.has_pressing == 1 {
        return Err(AppSW::BadState);
    }

    nvm.has_pressing = 1;
    nvm.pressing_cert = cert_buf;
    nvm.pressing_album_cert = staged_album;
    nvm.pressing_priv = bearer_priv;
    nvm.has_from = 0;
    nvm.ring_len = 0;
    Store::put(&nvm);
    session.staged_album_valid = false;

    Ok(command.into_response())
}
