//! Thin helpers over the SDK crypto primitives. All curve operations are
//! secp256k1; all hashes SHA-256; MACs HMAC-SHA256.

use crate::AppSW;
use ledger_device_sdk::ecc::{CurvesId, ECPrivateKey, ECPublicKey, Secp256k1};
use ledger_device_sdk::hash::{sha2::Sha2_256, HashInit};
use ledger_device_sdk::hmac::{sha2::Sha2_256 as HmacSha256, HMACInit};
use ledger_device_sdk::random::rand_bytes;

pub const PUBKEY_LEN: usize = 65;
pub const SIG_MAX_LEN: usize = 72;

pub type PrivKey = ECPrivateKey<32, 'W'>;

/// SHA-256 over a list of byte slices.
pub fn sha256(parts: &[&[u8]]) -> Result<[u8; 32], AppSW> {
    let mut ctx = Sha2_256::new();
    for p in parts {
        ctx.update(p).map_err(|_| AppSW::CryptoFail)?;
    }
    let mut out = [0u8; 32];
    ctx.finalize(&mut out).map_err(|_| AppSW::CryptoFail)?;
    Ok(out)
}

/// HMAC-SHA256 over a list of byte slices.
pub fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> Result<[u8; 32], AppSW> {
    let mut ctx = HmacSha256::new(key);
    for p in parts {
        ctx.update(p).map_err(|_| AppSW::CryptoFail)?;
    }
    let mut out = [0u8; 32];
    ctx.finalize(&mut out).map_err(|_| AppSW::CryptoFail)?;
    Ok(out)
}

/// Constant-time MAC comparison.
pub fn mac_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Generate a fresh keypair from the TRNG. The scalar never derives from the
/// seed: the device owner knows their 24 words and could recompute a
/// seed-derived key off-device, voiding the captivity argument.
pub fn gen_keypair() -> Result<([u8; 32], [u8; PUBKEY_LEN]), AppSW> {
    for _ in 0..4 {
        let mut sk_bytes = [0u8; 32];
        rand_bytes(&mut sk_bytes);
        let sk = Secp256k1::from(&sk_bytes);
        if let Ok(pk) = sk.public_key() {
            return Ok((sk_bytes, pk.pubkey));
        }
    }
    Err(AppSW::CryptoFail)
}

pub fn privkey_from(bytes: &[u8; 32]) -> PrivKey {
    Secp256k1::from(bytes)
}

/// The public point of a scalar. A received bearer key is only accepted once
/// this matches the `holderpub` its certificate was signed over, which is the
/// check a lying relay cannot get past: it can substitute a scalar, but not one
/// whose point the album key already signed.
pub fn pubkey_of(sk_bytes: &[u8; 32]) -> Result<[u8; PUBKEY_LEN], AppSW> {
    let sk = privkey_from(sk_bytes);
    let pk = sk.public_key().map_err(|_| AppSW::CryptoFail)?;
    Ok(pk.pubkey)
}

/// Deterministic ECDSA over SHA-256(payload). Returns (DER signature, length).
pub fn sign_payload(sk_bytes: &[u8; 32], payload: &[u8]) -> Result<([u8; SIG_MAX_LEN], u8), AppSW> {
    let hash = sha256(&[payload])?;
    let sk = privkey_from(sk_bytes);
    let (sig, sig_len, _) = sk.deterministic_sign(&hash).map_err(|_| AppSW::CryptoFail)?;
    if sig_len as usize > SIG_MAX_LEN {
        return Err(AppSW::CryptoFail);
    }
    let mut out = [0u8; SIG_MAX_LEN];
    out[..sig_len as usize].copy_from_slice(&sig[..sig_len as usize]);
    Ok((out, sig_len as u8))
}

/// ECDSA verification of a DER signature against SHA-256(payload).
pub fn verify_payload(pubkey: &[u8; PUBKEY_LEN], payload: &[u8], sig: &[u8]) -> Result<bool, AppSW> {
    let hash = sha256(&[payload])?;
    let mut pk = ECPublicKey::<PUBKEY_LEN, 'W'>::new(CurvesId::Secp256k1);
    pk.pubkey.copy_from_slice(pubkey);
    Ok(pk.verify((sig, sig.len() as u32), &hash))
}

/// ECDH shared secret (x coordinate) between our scalar and a peer point.
/// Fails closed on invalid peer points.
pub fn ecdh(sk_bytes: &[u8; 32], peer_pub: &[u8; PUBKEY_LEN]) -> Result<[u8; 32], AppSW> {
    let sk = privkey_from(sk_bytes);
    sk.ecdh(peer_pub).map_err(|_| AppSW::CryptoFail)
}
