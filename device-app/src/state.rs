//! Persistent state in the secure element's NVM. One AtomicStorage holding
//! the whole app state: updates are all-or-nothing across power loss, which
//! is what makes "a burned number is never a duplicated number" true.

use crate::certs::{ALBUM_CERT_LEN, PRESSING_CERT_LEN, TITLE_MAX};
use crate::crypto::{self, PUBKEY_LEN};
use crate::AppSW;
use ledger_device_sdk::nvm::{AlignedStorage, AtomicStorage, SingleStorage};
use ledger_device_sdk::NVMData;

/// How many issued pressings the master keeps on record for the on-device
/// collection screen (the counter itself has no such limit).
pub const PRESSED_LOG_LEN: usize = 8;

#[derive(Clone, Copy)]
pub struct PressedEntry {
    pub number: u16,
    /// First 4 bytes of SHA256(recipient devpub): the fingerprint shown at
    /// press time, so the screens tell one consistent story.
    pub recipient_fp: [u8; 4],
}

#[derive(Clone, Copy)]
pub struct PresseNvm {
    pub initialized: u8,
    pub dev_priv: [u8; 32],
    pub dev_pub: [u8; PUBKEY_LEN],

    pub has_master: u8,
    pub alb_priv: [u8; 32],
    pub alb_pub: [u8; PUBKEY_LEN],
    pub title: [u8; TITLE_MAX],
    pub title_len: u8,
    pub edition: u16,
    pub counter: u16,
    pub album_cert: [u8; ALBUM_CERT_LEN],
    pub pressed_log: [PressedEntry; PRESSED_LOG_LEN],

    pub has_pressing: u8,
    pub pressing_cert: [u8; PRESSING_CERT_LEN],
    pub pressing_album_cert: [u8; ALBUM_CERT_LEN],
}

const EMPTY: PresseNvm = PresseNvm {
    initialized: 0,
    dev_priv: [0; 32],
    dev_pub: [0; PUBKEY_LEN],
    has_master: 0,
    alb_priv: [0; 32],
    alb_pub: [0; PUBKEY_LEN],
    title: [0; TITLE_MAX],
    title_len: 0,
    edition: 0,
    counter: 0,
    album_cert: [0; ALBUM_CERT_LEN],
    pressed_log: [PressedEntry {
        number: 0,
        recipient_fp: [0; 4],
    }; PRESSED_LOG_LEN],
    has_pressing: 0,
    pressing_cert: [0; PRESSING_CERT_LEN],
    pressing_album_cert: [0; ALBUM_CERT_LEN],
};

#[link_section = ".nvm_data"]
static mut DATA: NVMData<AtomicStorage<PresseNvm>> = NVMData::new(AtomicStorage::new(&EMPTY));

/// Cover art: a square 1bpp sleeve. Kept out of `PresseNvm` because that
/// struct is copied through the stack on every read.
/// 160x160 is the largest square that still boots: the app's NVRAM data
/// region tops out at 32256 bytes (63 pages), and 192x192 pushes it to 32768,
/// where the loader accepts the app but it exits before the first APDU.
pub const ART_W: usize = 160;
pub const ART_LEN: usize = ART_W * ART_W / 8;

/// Stored as page-aligned cells and written one cell at a time: the SDK's
/// storage wrappers are the only supported way to reach flash.
pub const ART_CHUNK: usize = 64;
pub const ART_CELLS: usize = ART_LEN / ART_CHUNK;

#[link_section = ".nvm_data"]
static mut ART: NVMData<[AlignedStorage<[u8; ART_CHUNK]>; ART_CELLS]> =
    NVMData::new([AlignedStorage::new([0u8; ART_CHUNK]); ART_CELLS]);

pub struct Art;

impl Art {
    /// Borrow the stored art. The cells are contiguous in flash, so the
    /// first cell's address is the start of a single ART_LEN bitmap: this is
    /// what lets NBGL render straight out of NVM, with no RAM copy.
    pub fn get() -> &'static [u8; ART_LEN] {
        let data = &raw const ART;
        unsafe {
            let first = (*data).get_ref()[0].get_ref().as_ptr();
            &*(first as *const [u8; ART_LEN])
        }
    }

    /// True when no art has been uploaded (the region is still all zeros).
    /// A cut with a blank region binds the all-zero sleeve-hash sentinel, and
    /// rendering shows generative art rather than a blank square.
    pub fn is_blank() -> bool {
        Self::get().iter().all(|&b| b == 0)
    }

    /// Burn one chunk at `offset` through the NVM write syscall (a plain
    /// slice assignment would not reach flash). A partial upload leaves
    /// partial art, which the album hash check then rejects.
    pub fn write_chunk(offset: usize, chunk: &[u8]) -> Result<(), AppSW> {
        if chunk.len() != ART_CHUNK || offset % ART_CHUNK != 0 || offset + chunk.len() > ART_LEN {
            return Err(AppSW::WrongApduLength);
        }
        let mut cell = [0u8; ART_CHUNK];
        cell.copy_from_slice(chunk);
        let data = &raw mut ART;
        unsafe {
            (*data).get_mut()[offset / ART_CHUNK].update(&cell);
        }
        Ok(())
    }
}

pub struct Store;

impl Store {
    /// Read-only snapshot of NVM. Lazily generates the device identity key on
    /// first access so every device has an identity before any ceremony.
    pub fn get() -> Result<PresseNvm, AppSW> {
        let data = &raw mut DATA;
        let storage = unsafe { (*data).get_mut() };
        let mut current = *storage.get_ref();
        if current.initialized == 0 {
            let (sk, pk) = crypto::gen_keypair()?;
            current.dev_priv = sk;
            current.dev_pub = pk;
            current.initialized = 1;
            storage.update(&current);
        }
        Ok(current)
    }

    /// Atomically persist a full new state.
    pub fn put(new_state: &PresseNvm) {
        let data = &raw mut DATA;
        let storage = unsafe { (*data).get_mut() };
        storage.update(new_state);
    }
}
