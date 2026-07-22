use crate::certs::{build_album_cert, ARTIST_MAX, SLEEVE_HASH_LEN, TITLE_MAX};
use crate::crypto;
use crate::state::{Art, Store};
use crate::AppSW;
use alloc::format;
use ledger_device_sdk::io::{Command, CommandResponse};

/// CUT: data = edition(u16 LE) || title_len(1) || title(title_len bytes utf-8)
/// || artist(0..=13 bytes utf-8). The explicit title length delimits the two
/// variable-length names in one frame. UI-gated. One master per device;
/// re-cutting requires RESET_MASTER first.
pub fn handler_cut(command: Command<'_>) -> Result<CommandResponse<'_>, AppSW> {
    let data = command.get_data();
    if data.len() < 4 {
        return Err(AppSW::WrongApduLength);
    }
    let edition = u16::from_le_bytes([data[0], data[1]]);
    if edition == 0 {
        return Err(AppSW::BadCert);
    }
    let title_len = data[2] as usize;
    if title_len == 0 || title_len > TITLE_MAX || data.len() < 3 + title_len {
        return Err(AppSW::WrongApduLength);
    }
    let artist_len = data.len() - 3 - title_len;
    if artist_len > ARTIST_MAX {
        return Err(AppSW::WrongApduLength);
    }
    let mut title_buf = [0u8; TITLE_MAX];
    title_buf[..title_len].copy_from_slice(&data[3..3 + title_len]);
    let title = core::str::from_utf8(&title_buf[..title_len]).map_err(|_| AppSW::BadCert)?;
    let mut artist_buf = [0u8; ARTIST_MAX];
    artist_buf[..artist_len].copy_from_slice(&data[3 + title_len..]);
    let artist = core::str::from_utf8(&artist_buf[..artist_len]).map_err(|_| AppSW::BadCert)?;

    let mut nvm = Store::get()?;
    if nvm.has_master == 1 {
        return Err(AppSW::HasMaster);
    }

    let message = if artist_len == 0 {
        format!("Cut master of\n{}?", title)
    } else {
        format!("Cut master of\n{} by {}?", title, artist)
    };
    let submessage = format!(
        "Edition of {}, fixed forever.\nLosing this device destroys the plates.",
        edition
    );
    let comm = command.into_comm();
    let approved = crate::app_ui::menu::ceremony_choice().show(comm, &message, &submessage, "Cut the master", "Cancel");
    if !approved {
        return Err(AppSW::Deny);
    }

    // The sleeve is bound at cut time: whatever art was uploaded before the
    // cut becomes part of the signed identity. A blank region binds the
    // all-zero sentinel, meaning "no sleeve".
    let sleeve_hash: [u8; SLEEVE_HASH_LEN] = if Art::is_blank() {
        [0u8; SLEEVE_HASH_LEN]
    } else {
        crypto::sha256(&[Art::get()])?
    };

    let (alb_priv, alb_pub) = crypto::gen_keypair()?;
    let cert = build_album_cert(
        &alb_priv,
        &alb_pub,
        &title_buf[..title_len],
        edition,
        &sleeve_hash,
        &artist_buf[..artist_len],
    )?;

    nvm.has_master = 1;
    nvm.alb_priv = alb_priv;
    nvm.alb_pub = alb_pub;
    nvm.title = title_buf;
    nvm.title_len = title_len as u8;
    nvm.edition = edition;
    nvm.counter = edition;
    nvm.album_cert = cert;
    Store::put(&nvm);

    let mut response = comm.begin_response();
    response.append(&cert)?;
    Ok(response)
}

/// RESET_MASTER: destroy the plates. UI-gated, deliberately scary.
pub fn handler_reset_master(command: Command<'_>) -> Result<CommandResponse<'_>, AppSW> {
    let mut nvm = Store::get()?;
    if nvm.has_master != 1 {
        return Err(AppSW::NoMaster);
    }
    let comm = command.into_comm();
    let approved = crate::app_ui::menu::ceremony_choice().show(
        comm,
        "Destroy the plates?",
        "The album key is erased forever.\nNo further pressing will ever exist.",
        "Destroy forever",
        "Cancel",
    );
    if !approved {
        return Err(AppSW::Deny);
    }
    nvm.has_master = 0;
    nvm.alb_priv = [0; 32];
    nvm.alb_pub = [0; crate::crypto::PUBKEY_LEN];
    nvm.title = [0; TITLE_MAX];
    nvm.title_len = 0;
    nvm.edition = 0;
    nvm.counter = 0;
    nvm.album_cert = [0; crate::certs::ALBUM_CERT_LEN];
    nvm.pressed_log = [crate::state::PressedEntry {
        number: 0,
        recipient_fp: [0; 4],
    }; crate::state::PRESSED_LOG_LEN];
    Store::put(&nvm);
    let response = comm.begin_response();
    Ok(response)
}
