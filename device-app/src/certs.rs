//! Fixed-layout certificates. Hand-packed: no serde on-device, and the
//! independent TypeScript verifier re-implements this layout from
//! docs/protocol.md (deliberately no shared code).

use crate::crypto::{self, PUBKEY_LEN, SIG_MAX_LEN};
use crate::AppSW;

pub const TITLE_MAX: usize = 32;
/// Artist name, sealed alongside the title at cut. Capped at 13 bytes so the
/// whole AlbumCert (223) plus its session MAC (32) still fits one 255-byte
/// APDU when carried in PRESS_LOAD_ALBUM: 223 + 32 == 255 exactly. See
/// docs/protocol.md ("Artist field and the APDU-size constraint").
pub const ARTIST_MAX: usize = 13;
/// SHA-256 of the canonical sleeve bytes (as produced by scripts/sleeve.py).
/// All-zero means "no sleeve bound"; the device then shows generative art.
pub const SLEEVE_HASH_LEN: usize = 32;

pub const ALBUM_MAGIC: &[u8; 4] = b"PRA1";
/// Offset of the artist length byte, right after the sleeve hash. Placing the
/// artist after the pre-existing fields keeps every earlier offset (albpub,
/// title, edition, sleeve_hash) unchanged.
pub const ARTIST_LEN_OFF: usize = 4 + PUBKEY_LEN + 1 + TITLE_MAX + 2 + SLEEVE_HASH_LEN;
pub const ARTIST_OFF: usize = ARTIST_LEN_OFF + 1;
pub const ALBUM_PAYLOAD_LEN: usize = ARTIST_OFF + ARTIST_MAX;
pub const ALBUM_CERT_LEN: usize = ALBUM_PAYLOAD_LEN + 1 + SIG_MAX_LEN;

pub const PRESSING_MAGIC: &[u8; 4] = b"PRP1";
pub const PRESSING_PAYLOAD_LEN: usize = 4 + 32 + 2 + 2 + PUBKEY_LEN;
pub const PRESSING_CERT_LEN: usize = PRESSING_PAYLOAD_LEN + 1 + SIG_MAX_LEN;

/// AlbumCert layout:
/// magic(4) | albpub(65) | title_len(1) | title(32, zero-padded) | edition(2 LE)
/// | sleeve_hash(32) | artist_len(1) | artist(13, zero-padded) | sig_len(1)
/// | sig(72, zero-padded). The signature covers the whole payload including the
/// sleeve hash and the artist, so both are part of the signed identity of the
/// edition, fixed at cut time.
pub fn build_album_cert(
    alb_priv: &[u8; 32],
    albpub: &[u8; PUBKEY_LEN],
    title: &[u8],
    edition: u16,
    sleeve_hash: &[u8; SLEEVE_HASH_LEN],
    artist: &[u8],
) -> Result<[u8; ALBUM_CERT_LEN], AppSW> {
    if title.is_empty() || title.len() > TITLE_MAX || artist.len() > ARTIST_MAX {
        return Err(AppSW::BadCert);
    }
    let mut cert = [0u8; ALBUM_CERT_LEN];
    cert[..4].copy_from_slice(ALBUM_MAGIC);
    cert[4..4 + PUBKEY_LEN].copy_from_slice(albpub);
    cert[69] = title.len() as u8;
    cert[70..70 + title.len()].copy_from_slice(title);
    cert[102..104].copy_from_slice(&edition.to_le_bytes());
    cert[104..104 + SLEEVE_HASH_LEN].copy_from_slice(sleeve_hash);
    cert[ARTIST_LEN_OFF] = artist.len() as u8;
    cert[ARTIST_OFF..ARTIST_OFF + artist.len()].copy_from_slice(artist);
    let (sig, sig_len) = crypto::sign_payload(alb_priv, &cert[..ALBUM_PAYLOAD_LEN])?;
    cert[ALBUM_PAYLOAD_LEN] = sig_len;
    cert[ALBUM_PAYLOAD_LEN + 1..].copy_from_slice(&sig);
    Ok(cert)
}

pub struct AlbumInfo {
    pub albpub: [u8; PUBKEY_LEN],
    pub title: [u8; TITLE_MAX],
    pub title_len: u8,
    pub edition: u16,
    pub sleeve_hash: [u8; SLEEVE_HASH_LEN],
    pub artist: [u8; ARTIST_MAX],
    pub artist_len: u8,
}

/// The sleeve hash from a device's own AlbumCert, read straight from the
/// signed bytes without re-verifying the signature (the NVM cert is trusted;
/// a foreign cert reaches this only after `parse_album_cert`).
pub fn album_cert_sleeve_hash(cert: &[u8; ALBUM_CERT_LEN]) -> [u8; SLEEVE_HASH_LEN] {
    let mut h = [0u8; SLEEVE_HASH_LEN];
    h.copy_from_slice(&cert[104..104 + SLEEVE_HASH_LEN]);
    h
}

/// The artist bytes and length from a device's own AlbumCert, read straight
/// from the trusted NVM cert without re-verifying its signature (the same
/// treatment as `album_cert_sleeve_hash`). A foreign cert only reaches display
/// after `parse_album_cert` has verified it.
pub fn album_cert_artist(cert: &[u8; ALBUM_CERT_LEN]) -> ([u8; ARTIST_MAX], u8) {
    let mut artist = [0u8; ARTIST_MAX];
    let len = (cert[ARTIST_LEN_OFF] as usize).min(ARTIST_MAX);
    artist[..len].copy_from_slice(&cert[ARTIST_OFF..ARTIST_OFF + len]);
    (artist, len as u8)
}

/// Parse and cryptographically verify an AlbumCert.
pub fn parse_album_cert(cert: &[u8]) -> Result<AlbumInfo, AppSW> {
    if cert.len() != ALBUM_CERT_LEN || &cert[..4] != ALBUM_MAGIC {
        return Err(AppSW::BadCert);
    }
    let title_len = cert[69] as usize;
    if title_len == 0 || title_len > TITLE_MAX {
        return Err(AppSW::BadCert);
    }
    let sig_len = cert[ALBUM_PAYLOAD_LEN] as usize;
    if sig_len == 0 || sig_len > SIG_MAX_LEN {
        return Err(AppSW::BadCert);
    }
    let mut albpub = [0u8; PUBKEY_LEN];
    albpub.copy_from_slice(&cert[4..4 + PUBKEY_LEN]);
    let sig = &cert[ALBUM_PAYLOAD_LEN + 1..ALBUM_PAYLOAD_LEN + 1 + sig_len];
    if !crypto::verify_payload(&albpub, &cert[..ALBUM_PAYLOAD_LEN], sig)? {
        return Err(AppSW::BadCert);
    }
    let mut title = [0u8; TITLE_MAX];
    title.copy_from_slice(&cert[70..70 + TITLE_MAX]);
    let edition = u16::from_le_bytes([cert[102], cert[103]]);
    if edition == 0 {
        return Err(AppSW::BadCert);
    }
    let mut sleeve_hash = [0u8; SLEEVE_HASH_LEN];
    sleeve_hash.copy_from_slice(&cert[104..104 + SLEEVE_HASH_LEN]);
    let artist_len = cert[ARTIST_LEN_OFF] as usize;
    if artist_len > ARTIST_MAX {
        return Err(AppSW::BadCert);
    }
    let mut artist = [0u8; ARTIST_MAX];
    artist.copy_from_slice(&cert[ARTIST_OFF..ARTIST_OFF + ARTIST_MAX]);
    Ok(AlbumInfo {
        albpub,
        title,
        title_len: title_len as u8,
        edition,
        sleeve_hash,
        artist,
        artist_len: artist_len as u8,
    })
}

/// PressingCert layout:
/// magic(4) | album_id(32) | number(2 LE) | edition(2 LE) | holderpub(65)
/// | sig_len(1) | sig(72, zero-padded). album_id = SHA256(albpub).
///
/// `holderpub` is the *bearer key* of the copy, generated fresh at press time
/// and never a device identity: whoever holds the matching scalar holds the
/// copy. That indirection is what makes a copy transferable without ever
/// re-signing it, so the album key is needed only once, at the press.
pub fn build_pressing_cert(
    alb_priv: &[u8; 32],
    album_id: &[u8; 32],
    number: u16,
    edition: u16,
    holderpub: &[u8; PUBKEY_LEN],
) -> Result<[u8; PRESSING_CERT_LEN], AppSW> {
    let mut cert = [0u8; PRESSING_CERT_LEN];
    cert[..4].copy_from_slice(PRESSING_MAGIC);
    cert[4..36].copy_from_slice(album_id);
    cert[36..38].copy_from_slice(&number.to_le_bytes());
    cert[38..40].copy_from_slice(&edition.to_le_bytes());
    cert[40..40 + PUBKEY_LEN].copy_from_slice(holderpub);
    let (sig, sig_len) = crypto::sign_payload(alb_priv, &cert[..PRESSING_PAYLOAD_LEN])?;
    cert[PRESSING_PAYLOAD_LEN] = sig_len;
    cert[PRESSING_PAYLOAD_LEN + 1..].copy_from_slice(&sig);
    Ok(cert)
}

pub struct PressingInfo {
    pub album_id: [u8; 32],
    pub number: u16,
    pub edition: u16,
    /// The bearer key the copy is bound to. A device proves it holds the copy
    /// by signing with the matching scalar, not by being a named recipient.
    pub holderpub: [u8; PUBKEY_LEN],
}

/// Parse a PressingCert and verify its signature under the given album key.
pub fn parse_pressing_cert(cert: &[u8], albpub: &[u8; PUBKEY_LEN]) -> Result<PressingInfo, AppSW> {
    if cert.len() != PRESSING_CERT_LEN || &cert[..4] != PRESSING_MAGIC {
        return Err(AppSW::BadCert);
    }
    let sig_len = cert[PRESSING_PAYLOAD_LEN] as usize;
    if sig_len == 0 || sig_len > SIG_MAX_LEN {
        return Err(AppSW::BadCert);
    }
    let sig = &cert[PRESSING_PAYLOAD_LEN + 1..PRESSING_PAYLOAD_LEN + 1 + sig_len];
    if !crypto::verify_payload(albpub, &cert[..PRESSING_PAYLOAD_LEN], sig)? {
        return Err(AppSW::BadCert);
    }
    let mut album_id = [0u8; 32];
    album_id.copy_from_slice(&cert[4..36]);
    let number = u16::from_le_bytes([cert[36], cert[37]]);
    let edition = u16::from_le_bytes([cert[38], cert[39]]);
    let mut holderpub = [0u8; PUBKEY_LEN];
    holderpub.copy_from_slice(&cert[40..40 + PUBKEY_LEN]);
    if number == 0 || edition == 0 || number > edition {
        return Err(AppSW::BadCert);
    }
    Ok(PressingInfo {
        album_id,
        number,
        edition,
        holderpub,
    })
}
