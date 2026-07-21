use crate::certs::parse_album_cert;
use crate::handlers::press::fingerprint_str;
use crate::state::{Store, PRESSED_LOG_LEN};
use crate::AppSW;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use ledger_device_sdk::io::{Command, CommandResponse};
use ledger_device_sdk::nbgl::{Field, NbglGenericReview, NbglPageContent, TagValueList};
#[cfg(any(target_os = "stax", target_os = "flex"))]
use ledger_device_sdk::nbgl::{CenteredInfo, CenteredInfoStyle, NbglGlyph};
#[cfg(any(target_os = "stax", target_os = "flex"))]
use ledger_device_sdk::include_gif;

/// Eight compiled label arts; an album picks art by its id, so every album
/// keeps a stable, distinct record face.
#[cfg(any(target_os = "stax", target_os = "flex"))]
const ART: [NbglGlyph; 8] = [
    NbglGlyph::from_include(include_gif!("glyphs/vinyl_v0_96x96.gif", NBGL)),
    NbglGlyph::from_include(include_gif!("glyphs/vinyl_v1_96x96.gif", NBGL)),
    NbglGlyph::from_include(include_gif!("glyphs/vinyl_v2_96x96.gif", NBGL)),
    NbglGlyph::from_include(include_gif!("glyphs/vinyl_v3_96x96.gif", NBGL)),
    NbglGlyph::from_include(include_gif!("glyphs/vinyl_v4_96x96.gif", NBGL)),
    NbglGlyph::from_include(include_gif!("glyphs/vinyl_v5_96x96.gif", NBGL)),
    NbglGlyph::from_include(include_gif!("glyphs/vinyl_v6_96x96.gif", NBGL)),
    NbglGlyph::from_include(include_gif!("glyphs/vinyl_v7_96x96.gif", NBGL)),
];

fn title_str(title: &[u8], title_len: u8) -> Result<&str, AppSW> {
    core::str::from_utf8(&title[..title_len as usize]).map_err(|_| AppSW::BadCert)
}

#[cfg(any(target_os = "stax", target_os = "flex"))]
fn album_card(title: &str, line2: &str, line3: &str, album_id_byte: u8) -> NbglPageContent {
    NbglPageContent::CenteredInfo(CenteredInfo::new(
        title,
        line2,
        line3,
        Some(&ART[(album_id_byte & 7) as usize]),
        false,
        CenteredInfoStyle::LargeCaseBoldInfo,
        0,
    ))
}

fn fields_page(names: &[String], values: &[String]) -> NbglPageContent {
    let fields: Vec<Field> = names
        .iter()
        .zip(values.iter())
        .map(|(n, v)| Field {
            name: n.as_str(),
            value: v.as_str(),
        })
        .collect();
    NbglPageContent::TagValueList(TagValueList::new(&fields, 0, false, true))
}

/// Draws the collection as swipeable record cards and blocks until "Back"
/// (or an incoming APDU). Callable from the APDU handler and from the home
/// action button's NBGL callback.
pub fn show_collection_screen() -> Result<(), AppSW> {
    let nvm = Store::get()?;
    let mut review = NbglGenericReview::new();
    let mut any = false;

    // Owned strings must outlive show(): collect them here.
    let mut names_m: Vec<String> = Vec::new();
    let mut values_m: Vec<String> = Vec::new();
    let mut names_h: Vec<String> = Vec::new();
    let mut values_h: Vec<String> = Vec::new();
    let mut card_lines: Vec<String> = Vec::new();

    if nvm.has_master == 1 {
        any = true;
        let title = title_str(&nvm.title, nvm.title_len)?;
        let album_id = crate::crypto::sha256(&[&nvm.alb_pub])?;
        card_lines.push(format!("My master, edition of {}", nvm.edition));
        card_lines.push(format!("{} left to press", nvm.counter));
        #[cfg(any(target_os = "stax", target_os = "flex"))]
        {
            review = review.add_content(album_card(
                title,
                &card_lines[card_lines.len() - 2],
                &card_lines[card_lines.len() - 1],
                album_id[0],
            ));
        }
        names_m.push(String::from("Still to press"));
        values_m.push(format!("{}", nvm.counter));
        let pressed = (nvm.edition - nvm.counter) as usize;
        for entry in nvm.pressed_log.iter().take(pressed.min(PRESSED_LOG_LEN)) {
            if entry.number == 0 {
                continue;
            }
            names_m.push(format!("Pressed {} of {}", entry.number, nvm.edition));
            values_m.push(format!("for device {}", fingerprint_str(&entry.recipient_fp)));
        }
        if pressed > PRESSED_LOG_LEN {
            names_m.push(String::from("Earlier pressings"));
            values_m.push(format!("{} more, not listed", pressed - PRESSED_LOG_LEN));
        }
        review = review.add_content(fields_page(&names_m, &values_m));
    }

    if nvm.has_pressing == 1 {
        any = true;
        let album = parse_album_cert(&nvm.pressing_album_cert)?;
        let title = title_str(&album.title, album.title_len)?;
        let pressing = crate::certs::parse_pressing_cert(&nvm.pressing_cert, &album.albpub)?;
        card_lines.push(format!("Pressing {} of {}", pressing.number, pressing.edition));
        #[cfg(any(target_os = "stax", target_os = "flex"))]
        {
            review = review.add_content(album_card(
                title,
                &card_lines[card_lines.len() - 1],
                "Bound to this device",
                pressing.album_id[0],
            ));
        }
        names_h.push(String::from("In my collection"));
        values_h.push(format!(
            "{}, {} of {}",
            title, pressing.number, pressing.edition
        ));
        review = review.add_content(fields_page(&names_h, &values_h));
    }

    if !any {
        names_m.push(String::from("Collection"));
        values_m.push(String::from("Empty. Cut a master or receive a pressing."));
        review = review.add_content(fields_page(&names_m, &values_m));
    }

    review.show_from_callback("Back");
    Ok(())
}

/// COLLECTION over APDU: same screen, host-triggered (used by tests and the
/// relay demos).
pub fn handler_collection(command: Command<'_>) -> Result<CommandResponse<'_>, AppSW> {
    show_collection_screen()?;
    let response = command.into_response();
    Ok(response)
}
