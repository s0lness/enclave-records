use crate::certs::{
    parse_album_cert, parse_pressing_cert, build_pressing_cert, ALBUM_CERT_LEN, PRESSING_CERT_LEN,
};
use crate::crypto::{self, PUBKEY_LEN};
use crate::session::{Role, Session};
use crate::state::Store;
use crate::AppSW;
use alloc::format;
use ledger_device_sdk::io::{Command, CommandResponse};

pub const MAC_LEN: usize = 32;
/// Width of a bearer key's scalar, and so of its sealed form: the pad is one
/// HMAC-SHA256 output, which is exactly this wide.
pub const BEARER_KEY_LEN: usize = 32;

pub const INS_GET_ALBUM: u8 = 0x30;
pub const INS_PRESS_REQUEST: u8 = 0x31;
const INS_PRESS_OFFER: u8 = 0x32;

fn title_str(title: &[u8], title_len: u8) -> Result<&str, AppSW> {
    core::str::from_utf8(&title[..title_len as usize]).map_err(|_| AppSW::BadCert)
}

/// First 4 bytes of SHA256(devpub): the device fingerprint, shown as 8 hex
/// chars wherever a recipient is named.
pub fn fingerprint_bytes(devpub: &[u8; PUBKEY_LEN]) -> Result<[u8; 4], AppSW> {
    let hash = crypto::sha256(&[devpub])?;
    let mut fp = [0u8; 4];
    fp.copy_from_slice(&hash[..4]);
    Ok(fp)
}

pub fn fingerprint_str(fp: &[u8; 4]) -> alloc::string::String {
    format!("{:02X}{:02X}{:02X}{:02X}", fp[0], fp[1], fp[2], fp[3])
}

/// GET_ALBUM (master, paired): AlbumCert || MAC.
pub fn handler_get_album<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    session.require_ready()?;
    if session.role != Role::Master {
        return Err(AppSW::BadState);
    }
    let nvm = Store::get()?;
    if nvm.has_master != 1 {
        return Err(AppSW::NoMaster);
    }
    let mac = session.mac_send(INS_GET_ALBUM, &nvm.album_cert)?;
    let mut response = command.into_response();
    response.append(&nvm.album_cert)?;
    response.append(&mac)?;
    Ok(response)
}

/// PRESS_REQUEST (receiver, paired): devpub || MAC.
pub fn handler_press_request<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    session.require_ready()?;
    if session.role != Role::Receiver {
        return Err(AppSW::BadState);
    }
    let nvm = Store::get()?;
    let mac = session.mac_send(INS_PRESS_REQUEST, &nvm.dev_pub)?;
    let mut response = command.into_response();
    response.append(&nvm.dev_pub)?;
    response.append(&mac)?;
    Ok(response)
}

/// PRESS_OFFER (master, paired): data = receiver devpub(65) || MAC(32).
/// Returns PressingCert(178) || sealed bearer key(32) || MAC(32) = 242 bytes,
/// inside the 255-byte frame.
///
/// UI-gated. Decrements the counter atomically before the certificate leaves
/// the device: a power cut burns a number, never duplicates one.
///
/// The copy is bound to a bearer key minted here, not to the recipient's
/// device: the recipient's public key names *who* the press is for, on screen
/// and in the master's own log, but never enters the certificate. That is what
/// lets the copy be handed on later without the album key ever signing again.
pub fn handler_press_offer<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    session.require_ready()?;
    if session.role != Role::Master {
        return Err(AppSW::BadState);
    }
    let data = command.get_data();
    if data.len() != PUBKEY_LEN + MAC_LEN {
        return Err(AppSW::WrongApduLength);
    }
    let (payload, mac) = data.split_at(PUBKEY_LEN);
    session.mac_verify(INS_PRESS_REQUEST, payload, mac)?;

    let mut nvm = Store::get()?;
    if nvm.has_master != 1 {
        return Err(AppSW::NoMaster);
    }
    if nvm.counter == 0 {
        return Err(AppSW::SoldOut);
    }
    let mut recipient_pub = [0u8; PUBKEY_LEN];
    recipient_pub.copy_from_slice(payload);
    let number = nvm.edition - nvm.counter + 1;
    let title = title_str(&nvm.title, nvm.title_len)?;
    let fp_bytes = fingerprint_bytes(&recipient_pub)?;
    let fp = fingerprint_str(&fp_bytes);

    let message = format!("Press {}\n{} of {}?", title, number, nvm.edition);
    let submessage = format!("For device {}.\n{} pressings will remain.", fp, nvm.counter - 1);
    let comm = command.into_comm();
    let approved = crate::app_ui::menu::ceremony_choice().show(comm, &message, &submessage, "Press this copy", "Cancel");
    if !approved {
        return Err(AppSW::Deny);
    }

    let album_id = crypto::sha256(&[&nvm.alb_pub])?;
    let (bearer_priv, bearer_pub) = crypto::gen_keypair()?;
    let cert = build_pressing_cert(&nvm.alb_priv, &album_id, number, nvm.edition, &bearer_pub)?;
    // Sealed against the send counter this very frame will carry, so the pad is
    // the frame's own and cannot be shared with another transfer in the session.
    let sealed = session.bearer_xor(INS_PRESS_OFFER, session.send_seq, &bearer_priv)?;

    nvm.counter -= 1;
    let log_idx = (number - 1) as usize;
    if log_idx < crate::state::PRESSED_LOG_LEN {
        nvm.pressed_log[log_idx] = crate::state::PressedEntry {
            number,
            recipient_fp: fp_bytes,
        };
    }
    Store::put(&nvm);

    // The master keeps no copy of the bearer key: it minted the copy, it does
    // not hold it. Scrub the scalar now that it is sealed for the wire.
    let mut bearer_priv = bearer_priv;
    bearer_priv.fill(0);

    let mut payload = [0u8; PRESSING_CERT_LEN + BEARER_KEY_LEN];
    payload[..PRESSING_CERT_LEN].copy_from_slice(&cert);
    payload[PRESSING_CERT_LEN..].copy_from_slice(&sealed);
    let mac = session.mac_send(INS_PRESS_OFFER, &payload)?;
    let mut response = comm.begin_response();
    response.append(&payload)?;
    response.append(&mac)?;
    Ok(response)
}

/// PRESS_LOAD_ALBUM (receiver, paired): data = AlbumCert || MAC. Stages the
/// verified album for the accept step (both certs don't fit one APDU).
pub fn handler_press_load_album<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    session.require_ready()?;
    if session.role != Role::Receiver {
        return Err(AppSW::BadState);
    }
    let data = command.get_data();
    if data.len() != ALBUM_CERT_LEN + MAC_LEN {
        return Err(AppSW::WrongApduLength);
    }
    let (payload, mac) = data.split_at(ALBUM_CERT_LEN);
    session.mac_verify(INS_GET_ALBUM, payload, mac)?;
    parse_album_cert(payload)?;
    session.staged_album.copy_from_slice(payload);
    session.staged_album_valid = true;
    let response = command.into_response();
    Ok(response)
}

/// PRESS_ACCEPT (receiver, paired): data = PressingCert(178) || sealed bearer
/// key(32) || MAC(32) = 242 bytes, inside the 255-byte frame. Verifies the full
/// chain against the staged album, gates on the human, stores.
///
/// The binding this checks is possession, not address: the bearer key must be
/// the one the certificate's `holderpub` was signed over. A relay that swaps in
/// a scalar of its own is caught there, because it cannot make the album key
/// sign the matching point.
pub fn handler_press_accept<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    session.require_ready()?;
    if session.role != Role::Receiver || !session.staged_album_valid {
        return Err(AppSW::BadState);
    }
    let data = command.get_data();
    if data.len() != PRESSING_CERT_LEN + BEARER_KEY_LEN + MAC_LEN {
        return Err(AppSW::WrongApduLength);
    }
    let (payload, mac) = data.split_at(PRESSING_CERT_LEN + BEARER_KEY_LEN);
    // Unsealed before the MAC check only because the pad is keyed to the
    // *current* receive counter, which the check itself advances. The plaintext
    // is not looked at until the MAC has passed.
    let mut sealed = [0u8; BEARER_KEY_LEN];
    sealed.copy_from_slice(&payload[PRESSING_CERT_LEN..]);
    let bearer_priv = session.bearer_xor(INS_PRESS_OFFER, session.recv_seq, &sealed)?;
    session.mac_verify(INS_PRESS_OFFER, payload, mac)?;
    let mut cert_buf = [0u8; PRESSING_CERT_LEN];
    cert_buf.copy_from_slice(&payload[..PRESSING_CERT_LEN]);

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

    let title = title_str(&album.title, album.title_len)?;
    let message = format!("Receive {}\n{} of {}?", title, pressing.number, pressing.edition);
    let comm = command.into_comm();
    let approved = crate::app_ui::menu::ceremony_choice().show(
        comm,
        &message,
        "This copy becomes yours to\nkeep or to hand on.",
        "Receive it",
        "Cancel",
    );
    if !approved {
        return Err(AppSW::Deny);
    }

    nvm.has_pressing = 1;
    nvm.pressing_cert = cert_buf;
    nvm.pressing_album_cert = staged_album;
    nvm.pressing_priv = bearer_priv;
    // Straight from the press: no earlier holder to name, and no ring yet.
    nvm.has_from = 0;
    nvm.ring_len = 0;
    Store::put(&nvm);

    let response = comm.begin_response();
    Ok(response)
}
