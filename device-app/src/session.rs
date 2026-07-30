//! RAM-only pairing session. Dies on power cycle, on any MAC failure, and on
//! SAS rejection: a session is cheap, trust is not.

use crate::certs::{ALBUM_CERT_LEN, PRESSING_CERT_LEN};
use crate::crypto::{self, PUBKEY_LEN};
use crate::state::RING_MAX;
use crate::wordlist::WORDS;
use crate::AppSW;

const COMMIT_TAG: &[u8] = b"presse-commit";
const SAS_TAG: &[u8] = b"presse-sas";
const SESSION_TAG: &[u8] = b"presse-session";
/// Domain separator for the bearer-key pad. Distinct from every other use of
/// the session key so a pad can never collide with a MAC or a SAS value.
const BEARER_TAG: &[u8] = b"presse-bearer";

/// Online SAS-grinding cap: pairing attempts allowed per power cycle.
const MAX_ATTEMPTS: u8 = 8;

/// The two positions of the pairing handshake. Named for the press ceremony
/// that introduced them: the initiator (a master offering a copy, or a giver
/// handing one on) and the responder (the device that ends up holding it).
#[derive(Clone, Copy, PartialEq)]
pub enum Role {
    Master,
    Receiver,
}

#[derive(Clone, Copy, PartialEq)]
pub enum PairState {
    Idle,
    /// Master generated its ephemeral and sent the commitment.
    Committed,
    /// Receiver stored the commitment and revealed its ephemeral.
    Responded,
    /// Shared secret derived; awaiting human SAS confirmation.
    Derived,
    /// SAS confirmed on this device; MACed payloads may flow.
    Ready,
}

pub struct Session {
    pub state: PairState,
    pub role: Role,
    attempts: u8,
    eph_priv: [u8; 32],
    pub my_pub: [u8; PUBKEY_LEN],
    pub peer_commit: [u8; 32],
    session_key: [u8; 32],
    pub sas: [u8; 4],
    pub send_seq: u8,
    pub recv_seq: u8,
    /// Album cert staged by the receiver between PRESS_LOAD_ALBUM and
    /// PRESS_ACCEPT.
    pub staged_album: [u8; ALBUM_CERT_LEN],
    pub staged_album_valid: bool,
    /// A transfer's pressing cert and holder ring, staged by the taker before
    /// TAKE_ACCEPT. A copy's three pieces (album, pressing, ring) each exceed
    /// what is left of a 255-byte frame once the others are in it, so they
    /// arrive separately and are held here until the accept binds them
    /// together. RAM only: an interrupted transfer leaves nothing behind.
    pub staged_pressing: [u8; PRESSING_CERT_LEN],
    pub staged_pressing_valid: bool,
    pub staged_ring: [[u8; 4]; RING_MAX],
    pub staged_ring_len: u8,
    pub staged_ring_valid: bool,
    /// The handover record staged by the taker: who is giving, and their
    /// signature over it. Verified at the confirmation and again at the accept,
    /// never here, so the relay stays free to stage in any order it likes.
    pub staged_giverpub: [u8; PUBKEY_LEN],
    pub staged_handover_sig: [u8; crate::crypto::SIG_MAX_LEN],
    pub staged_handover_sig_len: u8,
    pub staged_handover_valid: bool,
    /// The peer's device key, learned from the frame it produced for
    /// PRESS_REQUEST. The giver needs it twice (to sign the handover, then to
    /// check its commitment) but the taker sends it once, so it is remembered
    /// here rather than asked for again.
    pub peer_devpub: [u8; PUBKEY_LEN],
    pub peer_devpub_valid: bool,
    /// Set by the taker's confirmation screen. The accept that follows carries
    /// no gate of its own, so this is the only thing standing between an
    /// incoming copy and NVM.
    pub take_confirmed: bool,
}

impl Session {
    pub fn new() -> Session {
        Session {
            state: PairState::Idle,
            role: Role::Master,
            attempts: 0,
            eph_priv: [0; 32],
            my_pub: [0; PUBKEY_LEN],
            peer_commit: [0; 32],
            session_key: [0; 32],
            sas: [0; 4],
            send_seq: 0,
            recv_seq: 0,
            staged_album: [0; ALBUM_CERT_LEN],
            staged_album_valid: false,
            staged_pressing: [0; PRESSING_CERT_LEN],
            staged_pressing_valid: false,
            staged_ring: [[0u8; 4]; RING_MAX],
            staged_ring_len: 0,
            staged_ring_valid: false,
            staged_giverpub: [0; PUBKEY_LEN],
            staged_handover_sig: [0; crate::crypto::SIG_MAX_LEN],
            staged_handover_sig_len: 0,
            staged_handover_valid: false,
            peer_devpub: [0; PUBKEY_LEN],
            peer_devpub_valid: false,
            take_confirmed: false,
        }
    }

    /// Reset everything but the per-boot attempt counter.
    pub fn reset(&mut self) {
        let attempts = self.attempts;
        *self = Session::new();
        self.attempts = attempts;
    }

    /// Begin a pairing attempt: fresh ephemeral, counted against the per-boot
    /// cap so a hostile relay cannot silently retry its way to a SAS match.
    pub fn begin(&mut self, role: Role) -> Result<(), AppSW> {
        if self.attempts >= MAX_ATTEMPTS {
            return Err(AppSW::TooManyAttempts);
        }
        self.reset();
        self.attempts += 1;
        self.role = role;
        let (sk, pk) = crypto::gen_keypair()?;
        self.eph_priv = sk;
        self.my_pub = pk;
        Ok(())
    }

    pub fn commitment(&self) -> Result<[u8; 32], AppSW> {
        crypto::sha256(&[COMMIT_TAG, &self.my_pub])
    }

    /// Derive session key + SAS from the ECDH secret and the transcript.
    /// Transcript order is (master ephemeral, receiver ephemeral) on both
    /// sides, so a MITM running two handshakes cannot make them collide.
    pub fn derive(&mut self, peer_pub: &[u8; PUBKEY_LEN]) -> Result<(), AppSW> {
        let secret = crypto::ecdh(&self.eph_priv, peer_pub)?;
        let (master_pub, receiver_pub) = match self.role {
            Role::Master => (&self.my_pub, peer_pub),
            Role::Receiver => (peer_pub, &self.my_pub),
        };
        let transcript = crypto::sha256(&[SAS_TAG, master_pub, receiver_pub])?;
        self.session_key = crypto::hmac_sha256(&secret, &[SESSION_TAG, &transcript])?;
        let sas_full = crypto::hmac_sha256(&secret, &[SAS_TAG, &transcript])?;
        self.sas.copy_from_slice(&sas_full[..4]);
        self.state = PairState::Derived;
        // The ephemeral has served its purpose; scrub it.
        self.eph_priv = [0u8; 32];
        Ok(())
    }

    pub fn sas_words(&self) -> [&'static str; 4] {
        [
            WORDS[self.sas[0] as usize],
            WORDS[self.sas[1] as usize],
            WORDS[self.sas[2] as usize],
            WORDS[self.sas[3] as usize],
        ]
    }

    pub fn confirm_sas(&mut self) {
        self.state = PairState::Ready;
        self.attempts = 0;
    }

    pub fn require_ready(&self) -> Result<(), AppSW> {
        if self.state == PairState::Ready {
            Ok(())
        } else {
            Err(AppSW::BadState)
        }
    }

    /// Mask a 32-byte bearer key with a pad derived from the session key, which
    /// both seals and opens it (XOR is its own inverse).
    ///
    /// No new primitive is introduced: HMAC-SHA256 under a dedicated tag is
    /// already this protocol's KDF, and its output is exactly the key's width,
    /// so the whole cipher is one XOR. Integrity is not this function's job and
    /// must not be attempted here: the session MAC already covers the
    /// ciphertext, and a second integrity mechanism would only be a second
    /// thing to get wrong.
    ///
    /// `seq` is the sequence number of the frame carrying the key, so no two
    /// frames of a session ever share a pad. That matters: a device holding
    /// both a master and a pressing can press one copy and give another inside
    /// a single pairing, and a reused pad would publish the XOR of the two
    /// bearer keys to anyone watching the wire.
    pub fn bearer_xor(&self, ins: u8, seq: u8, key: &[u8; 32]) -> Result<[u8; 32], AppSW> {
        let pad = crypto::hmac_sha256(&self.session_key, &[BEARER_TAG, &[ins, seq]])?;
        let mut out = [0u8; 32];
        for i in 0..32 {
            out[i] = key[i] ^ pad[i];
        }
        Ok(out)
    }

    /// MAC an outgoing payload and bump the send counter.
    pub fn mac_send(&mut self, ins: u8, payload: &[u8]) -> Result<[u8; 32], AppSW> {
        let mac = crypto::hmac_sha256(&self.session_key, &[&[ins, self.send_seq], payload])?;
        self.send_seq += 1;
        Ok(mac)
    }

    /// Verify an incoming payload's MAC. Any failure kills the session.
    pub fn mac_verify(&mut self, ins: u8, payload: &[u8], mac: &[u8]) -> Result<(), AppSW> {
        if mac.len() != 32 {
            self.reset();
            return Err(AppSW::BadMac);
        }
        let expected = crypto::hmac_sha256(&self.session_key, &[&[ins, self.recv_seq], payload])?;
        let mut given = [0u8; 32];
        given.copy_from_slice(mac);
        if !crypto::mac_eq(&expected, &given) {
            self.reset();
            return Err(AppSW::BadMac);
        }
        self.recv_seq += 1;
        Ok(())
    }
}
