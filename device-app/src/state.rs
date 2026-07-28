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
///
/// Two constraints pin the width at 128 once the device keeps two slots.
///
/// * **Multiple of 32.** The art is written in [`ART_CHUNK`]-byte cells, so
///   `ART_W * ART_W / 8` has to divide by 64. A width that does not (144, 152)
///   leaves `ART_CELLS * ART_CHUNK` short of `ART_LEN`, and every read through
///   [`Art::get`] runs off the end of the array.
/// * **Boot.** Two 160-wide slots produce an app the loader installs but that
///   exits before serving its first APDU, in every arrangement tried: one
///   nested array, one flat array, and two separate storage objects all die.
///   Two 128-wide slots boot. `data_size` alone does not explain it (an inert
///   ballast array boots at 21504, well past the 19968 of two 160-wide slots),
///   so treat the working configuration as measured, not as derived: re-run
///   `scripts/build-video.sh` and the boot check after any change here.
pub const ART_W: usize = 128;
pub const ART_LEN: usize = ART_W * ART_W / 8;

/// Stored as page-aligned cells and written one cell at a time: the SDK's
/// storage wrappers are the only supported way to reach flash.
pub const ART_CHUNK: usize = 64;
pub const ART_CELLS: usize = ART_LEN / ART_CHUNK;

/// One sleeve per record the device can hold. The NVM data model caps that at
/// a master and a pressing, so two slots cover every reachable state. A single
/// shared region cannot: the sleeve is validated against the hash signed into
/// *its own* album certificate, so two records sharing one region means the
/// second cut silently invalidates the first record's cover and it falls back
/// to generative art.
pub const ART_SLOTS: usize = 2;
pub const SLOT_MASTER: usize = 0;
pub const SLOT_PRESSING: usize = 1;

/// One `.nvm_data` object per slot. [`Art::slot_cells`] is the only place that
/// maps a slot to its storage, so the arrangement can change without touching
/// the callers.
type ArtCells = [AlignedStorage<[u8; ART_CHUNK]>; ART_CELLS];
const EMPTY_ART: ArtCells = [AlignedStorage::new([0u8; ART_CHUNK]); ART_CELLS];

#[link_section = ".nvm_data"]
static mut ART_MASTER: NVMData<ArtCells> = NVMData::new(EMPTY_ART);

#[link_section = ".nvm_data"]
static mut ART_PRESSING: NVMData<ArtCells> = NVMData::new(EMPTY_ART);

pub struct Art;

impl Art {
    /// The storage object backing a slot. An out-of-range slot resolves to the
    /// master's, since a caller with a bad slot must still get a valid bitmap
    /// rather than a wild pointer.
    fn slot_cells(slot: usize) -> *mut NVMData<ArtCells> {
        if slot == SLOT_PRESSING {
            &raw mut ART_PRESSING
        } else {
            &raw mut ART_MASTER
        }
    }

    /// Borrow one slot's stored art. A slot's cells are contiguous in flash, so
    /// its first cell's address is the start of a single ART_LEN bitmap: this is
    /// what lets NBGL render straight out of NVM, with no RAM copy.
    pub fn get(slot: usize) -> &'static [u8; ART_LEN] {
        let data = Self::slot_cells(slot);
        unsafe {
            let first = (*data).get_ref()[0].get_ref().as_ptr();
            &*(first as *const [u8; ART_LEN])
        }
    }

    /// True when no art has been uploaded to this slot (still all zeros).
    /// A cut with a blank slot binds the all-zero sleeve-hash sentinel, and
    /// rendering shows generative art rather than a blank square.
    pub fn is_blank(slot: usize) -> bool {
        Self::get(slot).iter().all(|&b| b == 0)
    }

    /// Burn one chunk at `offset` through the NVM write syscall (a plain
    /// slice assignment would not reach flash). A partial upload leaves
    /// partial art, which the album hash check then rejects.
    pub fn write_chunk(slot: usize, offset: usize, chunk: &[u8]) -> Result<(), AppSW> {
        if slot >= ART_SLOTS
            || chunk.len() != ART_CHUNK
            || offset % ART_CHUNK != 0
            || offset + chunk.len() > ART_LEN
        {
            return Err(AppSW::WrongApduLength);
        }
        let mut cell = [0u8; ART_CHUNK];
        cell.copy_from_slice(chunk);
        let data = Self::slot_cells(slot);
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
