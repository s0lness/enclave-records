//! Handing a copy on: the same paired channel as a press, carrying a key
//! instead of minting a certificate.
//!
//! A copy is a bearer key. Giving it away is sending that scalar and ceasing to
//! use it. Nothing is re-signed, nothing grows, and the number of hands a copy
//! can go through is unbounded. The album key is not involved and need not still
//! exist: an artist who destroyed their plates has not frozen the copies already
//! out there.
//!
//! ## Why this is two phases and not one
//!
//! A device cannot delete and deliver in the same instant. Erasing the copy in
//! the write that releases the key makes double spending impossible and makes a
//! dropped cable *destroy the copy*. That is the worse failure: an owner losing
//! a record to a bad USB port is not a threat model, it is a defect.
//!
//! So the dangerous write is a **commitment** rather than a deletion:
//!
//! 1. **GIVE_OFFER** moves the giver, in one atomic write, to *committed to one
//!    named recipient*. The copy stops being usable here immediately: it answers
//!    no challenge and can be offered to nobody else. The bearer key stays in
//!    flash for one reason only, to be re-sent to that same recipient as often
//!    as a flaky link requires.
//! 2. **GIVE_FINISH** consumes the recipient's receipt and does the erase, in
//!    one atomic write.
//!
//! Double spending is still impossible, now by a stated invariant rather than by
//! absence: *a committed copy is deliverable to exactly one recipient and the
//! commitment is never widened.* An interrupted transfer is retried by re-running
//! the ceremony with the same two devices; a transfer to anyone else is refused
//! for good. The cost of that safety is a copy stranded forever if its committed
//! recipient never comes back, which is survivable in a way that erasure is not.
//!
//! The order of the human confirmations follows from the same reasoning. The
//! taker gates **first**, on a copy whose certificates and handover record it
//! has already verified in full. By the time the giver is asked, every failure a
//! human or a bad certificate can cause has already happened, and the giver's
//! tap commits only to something known to be acceptable on the other side.
//!
//! ## What is proven
//!
//! One step of provenance: the giver signs the handover with its *device* key,
//! and the taker keeps that signature. The frame carrying the giver's public key
//! is itself under the session MAC, so the fingerprint the taker later displays
//! is the device it actually paired with, not a name the relay chose. Everything
//! deeper is the unsigned ring: where the copy has been, never a proof of it.

use crate::certs::{
    parse_album_cert, parse_pressing_cert, AlbumInfo, PressingInfo, ALBUM_CERT_LEN,
    PRESSING_CERT_LEN,
};
use crate::crypto::{self, PUBKEY_LEN, SIG_MAX_LEN};
use crate::handlers::press::{fingerprint_bytes, fingerprint_str, BEARER_KEY_LEN, MAC_LEN};
use crate::session::{Role, Session};
use crate::state::{PresseNvm, Store, RING_MAX};
use crate::AppSW;
use alloc::format;
use ledger_device_sdk::io::{Command, CommandResponse};

pub const INS_GIVE_ALBUM: u8 = 0x70;
pub const INS_GIVE_PRESSING: u8 = 0x71;
pub const INS_GIVE_RING: u8 = 0x72;
pub const INS_GIVE_OFFER: u8 = 0x73;
pub const INS_GIVE_HANDOVER: u8 = 0x78;
pub const INS_TAKE_RECEIPT: u8 = 0x7C;

/// The handover record's domain tag, keeping this signature from being mistaken
/// for a certificate or a possession proof under the same device key.
const HANDOVER_TAG: &[u8] = b"presse-handover";
/// tag(15) | album_id(32) | number(2 LE) | giver devpub(65) | taker devpub(65).
/// Both devices are named, so a signature captured from one handover cannot be
/// replayed to legitimise another.
const HANDOVER_MSG_LEN: usize = 15 + 32 + 2 + PUBKEY_LEN + PUBKEY_LEN;

/// The ring on the wire: its length, then the whole fixed-size array. Sent flat
/// rather than trimmed so the frame is one size whatever the copy's history.
/// 1 + 128 + MAC(32) = 161 bytes, well inside a frame.
pub const RING_WIRE_LEN: usize = 1 + RING_MAX * 4;

/// The handover frame, identical in both directions so the relay forwards it
/// whole: giver devpub(65) | sig_len(1) | sig(72). 138 + MAC(32) = 170 bytes.
pub const HANDOVER_WIRE_LEN: usize = PUBKEY_LEN + 1 + SIG_MAX_LEN;

/// The receipt frame: album_id(32) | number(2 LE) | taker devpub(65).
/// 99 + MAC(32) = 131 bytes.
///
/// The session MAC *is* the receipt, and it needs no signature of its own: only
/// the two paired devices know the session key, so a relay cannot produce one,
/// and the `ins` byte the MAC covers is one the giver never sends under, so no
/// frame of the giver's own can be reflected back at it to release its copy.
/// The payload travels in the clear anyway so the giver can check *which* copy
/// is being acknowledged instead of taking a bare token's word for it.
pub const RECEIPT_WIRE_LEN: usize = 32 + 2 + PUBKEY_LEN;

fn handover_message(
    album_id: &[u8; 32],
    number: u16,
    giver: &[u8; PUBKEY_LEN],
    taker: &[u8; PUBKEY_LEN],
) -> [u8; HANDOVER_MSG_LEN] {
    let mut msg = [0u8; HANDOVER_MSG_LEN];
    msg[..15].copy_from_slice(HANDOVER_TAG);
    msg[15..47].copy_from_slice(album_id);
    msg[47..49].copy_from_slice(&number.to_le_bytes());
    msg[49..49 + PUBKEY_LEN].copy_from_slice(giver);
    msg[49 + PUBKEY_LEN..].copy_from_slice(taker);
    msg
}

/// The copy this device holds, with its own certificates re-parsed. Reading them
/// back through the same verification a foreign certificate gets costs one
/// signature check and means a corrupted NVM cannot be handed on.
fn held_copy(nvm: &PresseNvm) -> Result<(AlbumInfo, PressingInfo), AppSW> {
    if nvm.has_pressing != 1 {
        return Err(AppSW::NoPressing);
    }
    let album = parse_album_cert(&nvm.pressing_album_cert)?;
    let pressing = parse_pressing_cert(&nvm.pressing_cert, &album.albpub)?;
    Ok((album, pressing))
}

/// A paired giver holding a copy. Note `has_pressing`, not "usable": a committed
/// copy must still answer these reads, because that is exactly the state an
/// interrupted transfer is retried from.
fn giver_ready(session: &Session) -> Result<PresseNvm, AppSW> {
    session.require_ready()?;
    if session.role != Role::Master {
        return Err(AppSW::BadState);
    }
    let nvm = Store::get()?;
    if nvm.has_pressing != 1 {
        return Err(AppSW::NoPressing);
    }
    Ok(nvm)
}

/// GIVE_ALBUM (giver, paired): the held copy's AlbumCert || MAC.
/// 223 + 32 == 255, the frame exactly.
pub fn handler_give_album<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    let nvm = giver_ready(session)?;
    let mac = session.mac_send(INS_GIVE_ALBUM, &nvm.pressing_album_cert)?;
    let mut response = command.into_response();
    response.append(&nvm.pressing_album_cert)?;
    response.append(&mac)?;
    Ok(response)
}

/// GIVE_PRESSING (giver, paired): the held PressingCert || MAC. 178 + 32 = 210.
pub fn handler_give_pressing<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    let nvm = giver_ready(session)?;
    let mac = session.mac_send(INS_GIVE_PRESSING, &nvm.pressing_cert)?;
    let mut response = command.into_response();
    response.append(&nvm.pressing_cert)?;
    response.append(&mac)?;
    Ok(response)
}

/// GIVE_RING (giver, paired): ring_len(1) || ring(128) || MAC(32) = 161.
///
/// The ring sent here is the holders *before* this device; the taker appends
/// this device itself, which is the one entry it can attest to.
pub fn handler_give_ring<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    let nvm = giver_ready(session)?;
    let mut wire = [0u8; RING_WIRE_LEN];
    wire[0] = nvm.ring_len.min(RING_MAX as u8);
    for (i, entry) in nvm.ring.iter().enumerate() {
        wire[1 + i * 4..1 + i * 4 + 4].copy_from_slice(entry);
    }
    let mac = session.mac_send(INS_GIVE_RING, &wire)?;
    let mut response = command.into_response();
    response.append(&wire)?;
    response.append(&mac)?;
    Ok(response)
}

/// GIVE_HANDOVER (giver, paired): data = taker devpub(65) || MAC(32) = 97.
/// Returns giverpub(65) || sig_len(1) || handover sig(72) || MAC(32) = 170.
///
/// Not gated, and changes nothing. A handover signature without the bearer key
/// confers no ownership, so signing it early is free, and it is what lets the
/// taker verify the whole copy and put its own question to its human before the
/// giver is asked anything at all.
///
/// The taker's public key arrives in the frame it produced for PRESS_REQUEST: a
/// give needs exactly what a press needs from the other side, so it reuses that
/// command rather than adding a second way to ask. It is remembered on the
/// session because GIVE_OFFER needs it too and it is only ever sent once.
pub fn handler_give_handover<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    let nvm = giver_ready(session)?;
    let data = command.get_data();
    if data.len() != PUBKEY_LEN + MAC_LEN {
        return Err(AppSW::WrongApduLength);
    }
    let (payload, mac) = data.split_at(PUBKEY_LEN);
    session.mac_verify(crate::handlers::press::INS_PRESS_REQUEST, payload, mac)?;
    let mut takerpub = [0u8; PUBKEY_LEN];
    takerpub.copy_from_slice(payload);

    let (_, pressing) = held_copy(&nvm)?;
    let msg = handover_message(&pressing.album_id, pressing.number, &nvm.dev_pub, &takerpub);
    let (sig, sig_len) = crypto::sign_payload(&nvm.dev_priv, &msg)?;

    session.peer_devpub = takerpub;
    session.peer_devpub_valid = true;

    let mut wire = [0u8; HANDOVER_WIRE_LEN];
    wire[..PUBKEY_LEN].copy_from_slice(&nvm.dev_pub);
    wire[PUBKEY_LEN] = sig_len;
    wire[PUBKEY_LEN + 1..].copy_from_slice(&sig);
    let mac = session.mac_send(INS_GIVE_HANDOVER, &wire)?;
    let mut response = command.into_response();
    response.append(&wire)?;
    response.append(&mac)?;
    Ok(response)
}

/// GIVE_OFFER (giver, paired): no data, in two halves selected by P1.
///
/// * **P1 = 0, the commitment.** UI-gated the first time, and the write the
///   design rests on: it moves the copy from held to *promised to this
///   recipient*, atomically. From that moment the copy answers no challenge here
///   and can go to nobody else. Returns nothing.
/// * **P1 = 1, the release.** Marks the key *flown* and returns sealed bearer
///   key(32) || MAC(32) = 64.
///
/// The halves are separate commands because the byte that separates them is the
/// only thing a human can still act on. Between them the copy is promised and
/// exists in exactly one place, so the promise can be taken back (GIVE_CANCEL);
/// after the release it may exist in two, so it cannot. Folding them back
/// together would make every commitment irrevocable the instant it is made,
/// including the ones whose recipient never answers.
///
/// Neither half is gated again on a repeat. The human already approved exactly
/// this handover to exactly this device, and the commitment in flash is the
/// record of it; asking twice would put a human back on the path where an
/// unanswered tap strands a copy. Nor does it help an attacker: reaching this
/// point needs a fresh session whose four words were read aloud and confirmed on
/// both screens, and the only thing obtainable is a key that recipient is owed.
pub fn handler_give_offer<'a>(
    command: Command<'a>,
    session: &mut Session,
    release: bool,
) -> Result<CommandResponse<'a>, AppSW> {
    let mut nvm = giver_ready(session)?;
    if !command.get_data().is_empty() {
        return Err(AppSW::WrongApduLength);
    }
    if !session.peer_devpub_valid {
        return Err(AppSW::BadState);
    }
    let takerpub = session.peer_devpub;
    let recipient = crypto::sha256(&[&takerpub])?;
    // The commitment is never widened: one recipient, decided once. Checked in
    // both halves, so neither is a way round the other.
    if nvm.committed != 0 && !crypto::mac_eq(&recipient, &nvm.committed_to) {
        return Err(AppSW::BadState);
    }

    if !release {
        if nvm.committed != 0 {
            // Already promised to this very device: a retry, not a new promise.
            return Ok(command.into_response());
        }
        let (album, pressing) = held_copy(&nvm)?;
        let title = core::str::from_utf8(&album.title[..album.title_len as usize])
            .map_err(|_| AppSW::BadCert)?;
        let fp = fingerprint_str(&fingerprint_bytes(&takerpub)?);
        let message = format!("Give {}\n#{} of {}?", title, pressing.number, pressing.edition);
        let submessage = format!("To device {}.\nYou will no longer hold it.", fp);
        let comm = command.into_comm();
        let approved = crate::app_ui::menu::ceremony_choice().show(
            comm,
            &message,
            &submessage,
            "Give it away",
            "Cancel",
        );
        if !approved {
            return Err(AppSW::Deny);
        }
        nvm.committed = 1;
        nvm.committed_to = recipient;
        Store::put(&nvm);
        return Ok(comm.begin_response());
    }

    if nvm.committed == 0 {
        return Err(AppSW::BadState);
    }
    // The key is about to reach the wire, so record that it has *before* it
    // does. A power cut between this write and the send then lands on the
    // resumable side (flown, never received) instead of leaving a promise this
    // device would still let its owner take back after handing the key out.
    if nvm.committed != 2 {
        nvm.committed = 2;
        Store::put(&nvm);
    }

    let mut bearer = nvm.pressing_priv;
    let sealed = session.bearer_xor(INS_GIVE_OFFER, session.send_seq, &bearer)?;
    bearer.fill(0);
    let mac = session.mac_send(INS_GIVE_OFFER, &sealed)?;
    let mut response = command.into_response();
    response.append(&sealed)?;
    response.append(&mac)?;
    Ok(response)
}

/// GIVE_CANCEL (giver, no session): no data. UI-gated. Takes back a promise
/// whose key never left, and refuses one whose key did.
///
/// Needs no pairing, because it is not a ceremony: nothing crosses to another
/// device, and the only authority it requires is the owner's finger on this
/// screen. Refusing it at `committed == 2` is the whole invariant: once the
/// sealed key has been put on the wire the copy may exist elsewhere, and a
/// device that could still un-promise it would be a double-spend primitive
/// wearing a repair's clothes. The way out of *that* state stays what it was:
/// the recipient's receipt, or nothing.
pub fn handler_give_cancel(command: Command<'_>) -> Result<CommandResponse<'_>, AppSW> {
    if !command.get_data().is_empty() {
        return Err(AppSW::WrongApduLength);
    }
    let mut nvm = Store::get()?;
    if nvm.has_pressing != 1 {
        return Err(AppSW::NoPressing);
    }
    match nvm.committed {
        1 => {}
        2 => return Err(AppSW::KeyFlown),
        _ => return Err(AppSW::BadState),
    }

    let mut fp = [0u8; 4];
    fp.copy_from_slice(&nvm.committed_to[..4]);
    let message = format!("Take back the promise\nto {}?", fingerprint_str(&fp));
    let comm = command.into_comm();
    let approved = crate::app_ui::menu::ceremony_choice().show(
        comm,
        &message,
        "The key never left.\nThe copy stays yours.",
        "Take it back",
        "Keep waiting",
    );
    if !approved {
        return Err(AppSW::Deny);
    }
    nvm.committed = 0;
    nvm.committed_to = [0u8; 32];
    Store::put(&nvm);
    Ok(comm.begin_response())
}

/// GIVE_FINISH (giver, paired): data = receipt(99) || MAC(32) = 131. The second
/// and last atomic write: the copy is gone from here.
///
/// Not gated. It completes an act the human already approved, and a gate would
/// put a human back on the path where an unanswered tap strands a copy. Refused
/// unless the receipt names the copy this device holds and comes from the
/// recipient it committed to, so no unrelated acknowledgement can be used to
/// make a copy disappear.
pub fn handler_give_finish<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    let mut nvm = giver_ready(session)?;
    // Promised or flown alike: this is a resume path, and which of the two the
    // giver stopped in is not the receipt's business. A receipt at `promised`
    // is unreachable anyway (the taker cannot hold a copy whose key never left),
    // so accepting it costs nothing and refusing it could strand a copy.
    if nvm.committed == 0 {
        return Err(AppSW::BadState);
    }
    let data = command.get_data();
    if data.len() != RECEIPT_WIRE_LEN + MAC_LEN {
        return Err(AppSW::WrongApduLength);
    }
    let (payload, mac) = data.split_at(RECEIPT_WIRE_LEN);
    session.mac_verify(INS_TAKE_RECEIPT, payload, mac)?;

    let mut album_id = [0u8; 32];
    album_id.copy_from_slice(&payload[..32]);
    let number = u16::from_le_bytes([payload[32], payload[33]]);
    let mut takerpub = [0u8; PUBKEY_LEN];
    takerpub.copy_from_slice(&payload[34..34 + PUBKEY_LEN]);

    if !crypto::mac_eq(&crypto::sha256(&[&takerpub])?, &nvm.committed_to) {
        return Err(AppSW::BadState);
    }
    let (_, pressing) = held_copy(&nvm)?;
    if !crypto::mac_eq(&album_id, &pressing.album_id) || number != pressing.number {
        return Err(AppSW::BadCert);
    }

    nvm.clear_pressing();
    nvm.has_given = 1;
    Store::put(&nvm);

    Ok(command.into_response())
}

fn taker_ready(session: &Session) -> Result<(), AppSW> {
    session.require_ready()?;
    if session.role != Role::Receiver {
        return Err(AppSW::BadState);
    }
    Ok(())
}

/// TAKE_ALBUM (taker, paired): data = AlbumCert(223) || MAC(32) = 255, the frame
/// exactly. Staged, verified, not yet stored.
pub fn handler_take_album<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    taker_ready(session)?;
    let data = command.get_data();
    if data.len() != ALBUM_CERT_LEN + MAC_LEN {
        return Err(AppSW::WrongApduLength);
    }
    let (payload, mac) = data.split_at(ALBUM_CERT_LEN);
    session.mac_verify(INS_GIVE_ALBUM, payload, mac)?;
    parse_album_cert(payload)?;
    session.staged_album.copy_from_slice(payload);
    session.staged_album_valid = true;
    Ok(command.into_response())
}

/// TAKE_PRESSING (taker, paired): data = PressingCert(178) || MAC(32) = 210.
/// Held until the pieces are checked together: staging order is the relay's to
/// choose, so nothing here may depend on it.
pub fn handler_take_pressing<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    taker_ready(session)?;
    let data = command.get_data();
    if data.len() != PRESSING_CERT_LEN + MAC_LEN {
        return Err(AppSW::WrongApduLength);
    }
    let (payload, mac) = data.split_at(PRESSING_CERT_LEN);
    session.mac_verify(INS_GIVE_PRESSING, payload, mac)?;
    session.staged_pressing.copy_from_slice(payload);
    session.staged_pressing_valid = true;
    Ok(command.into_response())
}

/// TAKE_RING (taker, paired): data = ring_len(1) || ring(128) || MAC(32) = 161.
pub fn handler_take_ring<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    taker_ready(session)?;
    let data = command.get_data();
    if data.len() != RING_WIRE_LEN + MAC_LEN {
        return Err(AppSW::WrongApduLength);
    }
    let (payload, mac) = data.split_at(RING_WIRE_LEN);
    session.mac_verify(INS_GIVE_RING, payload, mac)?;
    if payload[0] as usize > RING_MAX {
        return Err(AppSW::BadCert);
    }
    session.staged_ring_len = payload[0];
    for (i, entry) in session.staged_ring.iter_mut().enumerate() {
        entry.copy_from_slice(&payload[1 + i * 4..1 + i * 4 + 4]);
    }
    session.staged_ring_valid = true;
    Ok(command.into_response())
}

/// TAKE_HANDOVER (taker, paired): data = giverpub(65) || sig_len(1) || sig(72)
/// || MAC(32) = 170. Staged only; the signature is checked where the album id
/// and copy number it covers are also in hand.
pub fn handler_take_handover<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    taker_ready(session)?;
    let data = command.get_data();
    if data.len() != HANDOVER_WIRE_LEN + MAC_LEN {
        return Err(AppSW::WrongApduLength);
    }
    let (payload, mac) = data.split_at(HANDOVER_WIRE_LEN);
    session.mac_verify(INS_GIVE_HANDOVER, payload, mac)?;
    let sig_len = payload[PUBKEY_LEN] as usize;
    if sig_len == 0 || sig_len > SIG_MAX_LEN {
        return Err(AppSW::BadCert);
    }
    session.staged_giverpub.copy_from_slice(&payload[..PUBKEY_LEN]);
    session
        .staged_handover_sig
        .copy_from_slice(&payload[PUBKEY_LEN + 1..]);
    session.staged_handover_sig_len = sig_len as u8;
    session.staged_handover_valid = true;
    Ok(command.into_response())
}

/// Everything the taker checks about an incoming copy, in one place because it
/// runs twice: once to decide what to put on the confirmation screen, and again
/// at the accept, where the bearer key finally turns up.
///
/// Every check is mandatory and every one fails closed. Together they say: this
/// is a real copy of a real edition, the device handing it over meant to, and
/// this device has room for it. `bearer` is the only part the confirmation
/// cannot know yet; passing `None` skips exactly that one check and nothing else.
fn verify_incoming(
    session: &Session,
    nvm: &PresseNvm,
    bearer: Option<&[u8; BEARER_KEY_LEN]>,
) -> Result<(AlbumInfo, PressingInfo, [u8; PUBKEY_LEN]), AppSW> {
    if !session.staged_album_valid
        || !session.staged_pressing_valid
        || !session.staged_ring_valid
        || !session.staged_handover_valid
    {
        return Err(AppSW::BadState);
    }
    let album = parse_album_cert(&session.staged_album)?;
    let pressing = parse_pressing_cert(&session.staged_pressing, &album.albpub)?;

    let album_id = crypto::sha256(&[&album.albpub])?;
    if !crypto::mac_eq(&album_id, &pressing.album_id) {
        return Err(AppSW::BadCert);
    }
    if pressing.edition != album.edition {
        return Err(AppSW::BadCert);
    }
    if let Some(bearer) = bearer {
        // The check a lying relay cannot get past: the scalar handed over must
        // be the one whose point the album key signed into the certificate.
        if crypto::pubkey_of(bearer)? != pressing.holderpub {
            return Err(AppSW::BadCert);
        }
    }
    let giverpub = session.staged_giverpub;
    let msg = handover_message(&pressing.album_id, pressing.number, &giverpub, &nvm.dev_pub);
    let sig = &session.staged_handover_sig[..session.staged_handover_sig_len as usize];
    if !crypto::verify_payload(&giverpub, &msg, sig)? {
        return Err(AppSW::BadCert);
    }
    if nvm.has_pressing == 1 {
        return Err(AppSW::BadState);
    }
    Ok((album, pressing, giverpub))
}

/// TAKE_CONFIRM (taker, paired): no data. UI-gated, and the *first* human
/// decision of the ceremony.
///
/// Everything checkable without the key is checked here, so a refusal -- by this
/// human, by an already-occupied device, or by a bad certificate -- happens
/// while the giver still holds its copy outright and has been asked nothing.
/// That is what makes the common failures free.
pub fn handler_take_confirm<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    taker_ready(session)?;
    if !command.get_data().is_empty() {
        return Err(AppSW::WrongApduLength);
    }
    let nvm = Store::get()?;
    let (album, pressing, giverpub) = verify_incoming(session, &nvm, None)?;

    let title = core::str::from_utf8(&album.title[..album.title_len as usize])
        .map_err(|_| AppSW::BadCert)?;
    let fp = fingerprint_str(&fingerprint_bytes(&giverpub)?);
    let message = format!("Receive {}\n#{} of {}?", title, pressing.number, pressing.edition);
    let submessage = format!("From device {}.", fp);
    let comm = command.into_comm();
    let approved = crate::app_ui::menu::ceremony_choice().show(
        comm,
        &message,
        &submessage,
        "Receive it",
        "Cancel",
    );
    if !approved {
        return Err(AppSW::Deny);
    }
    session.take_confirmed = true;
    Ok(comm.begin_response())
}

/// TAKE_ACCEPT (taker, paired): data = sealed bearer key(32) || MAC(32) = 64.
///
/// Carries no gate of its own: the human answered at TAKE_CONFIRM, and this
/// command exists only to receive the key that confirmation was about. It
/// nonetheless re-runs every check, including the one the confirmation could
/// not: nothing between the two commands is trusted because a screen came first.
pub fn handler_take_accept<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    taker_ready(session)?;
    if !session.take_confirmed {
        return Err(AppSW::BadState);
    }
    let data = command.get_data();
    if data.len() != BEARER_KEY_LEN + MAC_LEN {
        return Err(AppSW::WrongApduLength);
    }
    let (payload, mac) = data.split_at(BEARER_KEY_LEN);
    // Unsealed before the MAC check only because the pad is keyed to the
    // *current* receive counter, which the check itself advances. Nothing is
    // read out of the plaintext until the MAC has passed.
    let mut sealed = [0u8; BEARER_KEY_LEN];
    sealed.copy_from_slice(payload);
    let bearer_priv = session.bearer_xor(INS_GIVE_OFFER, session.recv_seq, &sealed)?;
    session.mac_verify(INS_GIVE_OFFER, payload, mac)?;

    let mut nvm = Store::get()?;
    let (_, _, giverpub) = verify_incoming(session, &nvm, Some(&bearer_priv))?;
    let giver_fp = fingerprint_bytes(&giverpub)?;

    nvm.has_pressing = 1;
    nvm.pressing_cert = session.staged_pressing;
    nvm.pressing_album_cert = session.staged_album;
    nvm.pressing_priv = bearer_priv;
    nvm.pressing_from = giverpub;
    nvm.pressing_from_sig = session.staged_handover_sig;
    nvm.pressing_from_sig_len = session.staged_handover_sig_len;
    nvm.has_from = 1;
    // The giver is the newest entry: it is the one hand this device saw the copy
    // leave. Everything before it comes from the ring it was sent.
    nvm.ring = session.staged_ring;
    nvm.ring_len = push_ring(&mut nvm.ring, session.staged_ring_len, giver_fp);
    Store::put(&nvm);

    Ok(command.into_response())
}

/// TAKE_RECEIPT (taker, paired): no data. Returns album_id(32) || number(2 LE)
/// || devpub(65) || MAC(32) = 131, the acknowledgement that releases the giver.
///
/// Built from what this device *holds*, not from what was staged, so it is also
/// the answer on a retry where the copy landed and only the receipt was lost:
/// re-pair, ask again, and the giver can finish.
pub fn handler_take_receipt<'a>(
    command: Command<'a>,
    session: &mut Session,
) -> Result<CommandResponse<'a>, AppSW> {
    taker_ready(session)?;
    if !command.get_data().is_empty() {
        return Err(AppSW::WrongApduLength);
    }
    let nvm = Store::get()?;
    let (_, pressing) = held_copy(&nvm)?;

    let mut wire = [0u8; RECEIPT_WIRE_LEN];
    wire[..32].copy_from_slice(&pressing.album_id);
    wire[32..34].copy_from_slice(&pressing.number.to_le_bytes());
    wire[34..].copy_from_slice(&nvm.dev_pub);
    let mac = session.mac_send(INS_TAKE_RECEIPT, &wire)?;
    let mut response = command.into_response();
    response.append(&wire)?;
    response.append(&mac)?;
    Ok(response)
}

/// Append a fingerprint to the ring, dropping the oldest when it is full. The
/// ring is a bounded display of a history that is not: a copy can change hands
/// more often than the ring can remember, and losing the earliest hop is the
/// deliberate cost of a fixed-size record.
fn push_ring(ring: &mut [[u8; 4]; RING_MAX], len: u8, fp: [u8; 4]) -> u8 {
    let len = (len as usize).min(RING_MAX);
    if len < RING_MAX {
        ring[len] = fp;
        return len as u8 + 1;
    }
    ring.rotate_left(1);
    ring[RING_MAX - 1] = fp;
    RING_MAX as u8
}
