//! Development probe for `os_factory_setting_get`, off by default.
//!
//! The syscall is declared by the SDK (`include/os_id.h`, id `0x0300014c`) and
//! no ID enumeration exists anywhere public: not in the SDK, not in speculos,
//! not in the 2023 OS export. The only way to learn what an ID answers is to
//! ask the silicon, so this handler is a thin, verbatim window onto one call:
//! it passes the caller's id and length through and reports exactly what came
//! back, with no interpretation and no reuse of the bytes.
//!
//! Two things make the report trustworthy rather than merely plausible:
//!
//! - the output buffer is poisoned with [`POISON`] before the call, so "the OS
//!   wrote nothing" is distinguishable from "the OS wrote zeros". An emulator
//!   that does not implement the syscall answers 0 and leaves the buffer alone,
//!   which reads as convincing zeros unless the buffer says otherwise.
//! - the call runs under its own BOLOS try context, so an OS-side `THROW` comes
//!   back as an exception code in the reply instead of unwinding to the C entry
//!   point, which would exit the app. On a real device that exit costs a
//!   physical relaunch, so catching is what makes a sweep affordable.

use crate::AppSW;
use ledger_device_sdk::io::{Command, CommandResponse};
use ledger_secure_sdk_sys::{
    os_factory_setting_get, os_longjmp, setjmp, try_context_set, try_context_t,
};

/// Fill byte for the output buffer. Not 0x00 and not 0xFF, so neither a zeroed
/// answer nor an erased-flash answer can be mistaken for an untouched buffer.
const POISON: u8 = 0xA5;

/// Reserved id: instead of calling the syscall, throw inside the probe's own
/// try context. It is how the catch is verified in the emulator before a sweep
/// runs on hardware, where a catch that silently does not work costs a physical
/// relaunch of the app per faulting id.
const SELFTEST_ID: u32 = 0xDEAD_BEEF;
/// Exception code [`SELFTEST_ID`] throws. Arbitrary, and outside the OS's own
/// `EXCEPTION_*` range so a caught 0x1234 cannot be confused with a real fault.
const SELFTEST_EX: u32 = 0x1234;

/// Largest answer we will ask for. The reply carries the whole buffer verbatim,
/// so this bounds the response at `HEADER_LEN + MAX_LEN` bytes; 128 leaves room
/// for anything certificate-shaped (a secp256k1 point is 65, a DER signature at
/// most 72) while staying well inside one APDU.
const MAX_LEN: u8 = 128;

/// FACTORY_PROBE: data = id(u32 LE) || maxlen(u8).
///
/// Reply: `caught(u8) || ex(u16 LE) || ret(u32 LE) || maxlen(u8) || buffer`.
/// `caught` is 1 when the syscall threw, and then `ex` is the exception code and
/// `ret` is meaningless. `buffer` is always `maxlen` bytes, poison included, so
/// the caller sees how far the OS actually wrote.
pub fn handler_factory_probe(command: Command<'_>) -> Result<CommandResponse<'_>, AppSW> {
    let data = command.get_data();
    if data.len() != 5 {
        return Err(AppSW::WrongApduLength);
    }
    let id = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let maxlen = data[4];
    if maxlen == 0 || maxlen > MAX_LEN {
        return Err(AppSW::WrongApduLength);
    }

    let mut buf = [POISON; MAX_LEN as usize];
    let mut ret: u32 = 0;
    let mut ex: u16 = 0;
    let caught = probe_call(id, buf.as_mut_ptr(), maxlen as u32, &mut ret, &mut ex);

    let mut response = command.into_response();
    response.append(&[caught])?;
    response.append(&ex.to_le_bytes())?;
    response.append(&ret.to_le_bytes())?;
    response.append(&[maxlen])?;
    response.append(&buf[..maxlen as usize])?;
    Ok(response)
}

/// Call the syscall under a private try context. Returns 1 if it threw.
///
/// Everything the caller needs back is written through raw pointers rather than
/// returned in locals: `setjmp` restores registers but not stack slots the
/// compiler chose to keep in registers across the call, so locals that live
/// across it are not reliable. `#[inline(never)]` keeps that reasoning local to
/// this frame instead of spreading it into the handler's.
#[inline(never)]
fn probe_call(id: u32, out: *mut u8, maxlen: u32, ret: *mut u32, ex: *mut u16) -> u8 {
    // SAFETY: `out` points at MAX_LEN writable bytes and `maxlen <= MAX_LEN`, so
    // the syscall's PLENGTH check sees a buffer this app owns. The try context
    // is pushed only around the call and popped on both paths, which is the same
    // discipline the SDK's C `BEGIN_TRY`/`END_TRY` macros enforce.
    unsafe {
        let mut ctx = try_context_t::default();
        let thrown = setjmp(ctx.jmp_buf.as_mut_ptr());
        if thrown == 0 {
            ctx.previous = try_context_set(&mut ctx);
            if id == SELFTEST_ID {
                os_longjmp(SELFTEST_EX);
            }
            let r = os_factory_setting_get(id, out, maxlen);
            try_context_set(ctx.previous);
            *ret = r;
            0
        } else {
            try_context_set(ctx.previous);
            *ex = thrown as u16;
            1
        }
    }
}
