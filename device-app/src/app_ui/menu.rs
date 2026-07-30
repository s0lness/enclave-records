use ledger_device_sdk::nbgl::NbglChoice;

#[cfg(not(any(target_os = "stax", target_os = "flex")))]
use ledger_device_sdk::include_gif;
#[cfg(not(any(target_os = "stax", target_os = "flex")))]
use ledger_device_sdk::nbgl::NbglGlyph;
#[cfg(not(any(target_os = "stax", target_os = "flex")))]
use alloc::format;
#[cfg(not(any(target_os = "stax", target_os = "flex")))]
use alloc::string::String;
#[cfg(not(any(target_os = "stax", target_os = "flex")))]
use ledger_device_sdk::io::Comm;
#[cfg(not(any(target_os = "stax", target_os = "flex")))]
use ledger_device_sdk::nbgl::NbglHomeAndSettings;
#[cfg(not(any(target_os = "stax", target_os = "flex")))]
use crate::state::Store;

// The generic vinyl glyph only survives on the button-based home (Nano/Apex).
// Flex/Stax open on the library and confirmations carry no decorative art.
#[cfg(target_os = "apex_p")]
pub const RECORD: NbglGlyph = NbglGlyph::from_include(include_gif!("glyphs/crab_48x48.png", NBGL));
#[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
pub const RECORD: NbglGlyph =
    NbglGlyph::from_include(include_gif!("glyphs/home_nano_nbgl.png", NBGL));

/// The idle screen shows what this device holds; the default tagline only
/// appears while it holds nothing. Only the legacy (non-touch) landing loop
/// uses it; Flex/Stax open on the library instead.
#[cfg(not(any(target_os = "stax", target_os = "flex")))]
fn tagline() -> String {
    let Ok(nvm) = Store::get() else {
        return String::from("Finite editions,\npressed in silicon.");
    };
    let mut lines: alloc::vec::Vec<String> = alloc::vec::Vec::new();
    if nvm.has_master == 1 {
        if let Ok(title) = core::str::from_utf8(&nvm.title[..nvm.title_len as usize]) {
            lines.push(format!(
                "Master: {}\n{} of {} left to press",
                title, nvm.counter, nvm.edition
            ));
        }
    }
    if nvm.has_pressing == 1 {
        if let Ok(album) = crate::certs::parse_album_cert(&nvm.pressing_album_cert) {
            if let (Ok(title), Ok(pressing)) = (
                core::str::from_utf8(&album.title[..album.title_len as usize]),
                crate::certs::parse_pressing_cert(&nvm.pressing_cert, &album.albpub),
            ) {
                // TODO: a promised copy (nvm.committed != 0) still reads
                // "Holding:" here. It should say it is promised, as the Flex
                // library row does. Unverified: only the Flex SDK is installed,
                // so no Nano build can be booted to check the wording.
                lines.push(format!(
                    "Holding: {} ({} of {})",
                    title, pressing.number, pressing.edition
                ));
            }
        }
    }
    if lines.is_empty() {
        String::from("Finite editions,\npressed in silicon.")
    } else {
        lines.join("\n")
    }
}

#[cfg(not(any(target_os = "stax", target_os = "flex")))]
pub fn ui_menu_main(_: &mut Comm) -> NbglHomeAndSettings {
    NbglHomeAndSettings::new()
        .glyph(&RECORD)
        .tagline(&tagline())
        .action("My records", None, on_my_records)
        .infos("Enclave Records", env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_AUTHORS"))
}

/// The home drawn from the NBGL callback must outlive the draw call (NBGL
/// keeps pointers into its strings). Heap-allocated and tracked through an
/// atomic so the static stays zero-initialized (.bss: this target forbids a
/// .data section). Single UI thread.
#[cfg(not(any(target_os = "stax", target_os = "flex")))]
static CALLBACK_HOME: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Home action button: draw the collection, then put the home back up.
/// Runs inside the NBGL event loop, no Comm in reach.
#[cfg(not(any(target_os = "stax", target_os = "flex")))]
fn on_my_records() {
    let _ = crate::handlers::collection::show_collection_screen();
    let home = alloc::boxed::Box::new(
        NbglHomeAndSettings::new()
            .glyph(&RECORD)
            .tagline(&tagline())
            .action("My records", None, on_my_records)
            .infos("Enclave Records", env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_AUTHORS")),
    );
    let ptr = alloc::boxed::Box::into_raw(home);
    unsafe {
        (*ptr).show_and_return();
    }
    // The previous callback-home's strings are no longer referenced once the
    // new home is on screen; release it only now.
    let old = CALLBACK_HOME.swap(ptr as usize, core::sync::atomic::Ordering::Relaxed);
    if old != 0 {
        drop(unsafe { alloc::boxed::Box::from_raw(old as *mut NbglHomeAndSettings) });
    }
}

/// The confirmation page used by every ceremony. No decorative glyph: the
/// message text carries the whole meaning, and album art belongs only on the
/// record card and the library row, never on a yes/no confirmation.
pub fn ceremony_choice() -> NbglChoice<'static> {
    NbglChoice::new()
}
