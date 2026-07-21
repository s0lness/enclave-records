use alloc::format;
use alloc::string::String;
use ledger_device_sdk::include_gif;
use ledger_device_sdk::io::Comm;
use ledger_device_sdk::nbgl::{NbglChoice, NbglGlyph, NbglHomeAndSettings};

use crate::state::Store;

#[cfg(target_os = "apex_p")]
pub const RECORD: NbglGlyph = NbglGlyph::from_include(include_gif!("glyphs/crab_48x48.png", NBGL));
#[cfg(any(target_os = "stax", target_os = "flex"))]
pub const RECORD: NbglGlyph = NbglGlyph::from_include(include_gif!("glyphs/vinyl_64x64.gif", NBGL));
#[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
pub const RECORD: NbglGlyph =
    NbglGlyph::from_include(include_gif!("glyphs/home_nano_nbgl.png", NBGL));

/// The idle screen shows what this device holds; the default tagline only
/// appears while it holds nothing.
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

pub fn ui_menu_main(_: &mut Comm) -> NbglHomeAndSettings {
    NbglHomeAndSettings::new()
        .glyph(&RECORD)
        .tagline(&tagline())
        .infos("Presse", env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_AUTHORS"))
}

/// The confirmation page used by every ceremony, vinyl front and center.
pub fn ceremony_choice() -> NbglChoice<'static> {
    NbglChoice::new().glyph(&RECORD)
}
