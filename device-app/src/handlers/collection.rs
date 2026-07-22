use crate::certs::parse_album_cert;
use crate::handlers::press::fingerprint_str;
use crate::state::{Store, PRESSED_LOG_LEN};
use crate::AppSW;
use alloc::ffi::CString;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use ledger_device_sdk::io::{Command, CommandResponse};
use ledger_device_sdk::nbgl::{Field, NbglGenericReview, NbglPageContent, TagValueList};
#[cfg(any(target_os = "stax", target_os = "flex"))]
use ledger_device_sdk::nbgl::{CenteredInfo, CenteredInfoStyle, NbglGlyph};

fn title_str(title: &[u8], title_len: u8) -> Result<&str, AppSW> {
    core::str::from_utf8(&title[..title_len as usize]).map_err(|_| AppSW::BadCert)
}

/// The sleeve this device should show for an album: the stored art when its
/// hash matches the one signed into the album certificate, otherwise the
/// generated label art.
///
/// A mismatch is not an error to report but a fact to render honestly: the
/// device shows the album's own generated face rather than a bitmap the
/// signed identity does not vouch for. An all-zero `sleeve_hash` means the
/// edition was cut with no sleeve, so it too falls back.
pub fn album_sleeve(sleeve_hash: &[u8; 32], album_id: &[u8; 32]) -> alloc::vec::Vec<u8> {
    let unbound = *sleeve_hash == [0u8; 32];
    if !unbound && !crate::state::Art::is_blank() {
        let art = crate::state::Art::get();
        if let Ok(hash) = crate::crypto::sha256(&[art]) {
            if crate::crypto::mac_eq(&hash, sleeve_hash) {
                return art.to_vec();
            }
        }
    }
    crate::sleeve::fallback_sleeve(crate::state::ART_W, album_id)
}

#[cfg(any(target_os = "stax", target_os = "flex"))]
fn album_card(
    title: &str,
    line2: &str,
    line3: &str,
    glyph: &NbglGlyph,
) -> NbglPageContent {
    NbglPageContent::CenteredInfo(CenteredInfo::new(
        title,
        line2,
        line3,
        Some(glyph),
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

    // Sleeves are computed up front and kept alive for the whole call: NBGL
    // holds a pointer into each bitmap until the screen goes away. Full size
    // here: this is the record card, so the sleeve gets the whole 160x160.
    let full = crate::state::ART_W;
    let master_sleeve = if nvm.has_master == 1 {
        let album_id = crate::crypto::sha256(&[&nvm.alb_pub])?;
        let sleeve_hash = crate::certs::album_cert_sleeve_hash(&nvm.album_cert);
        Some(crate::sleeve::to_display(&album_sleeve(&sleeve_hash, &album_id)))
    } else {
        None
    };
    let pressing_sleeve = if nvm.has_pressing == 1 {
        let album = parse_album_cert(&nvm.pressing_album_cert)?;
        let pressing = crate::certs::parse_pressing_cert(&nvm.pressing_cert, &album.albpub)?;
        Some(crate::sleeve::to_display(&album_sleeve(
            &album.sleeve_hash,
            &pressing.album_id,
        )))
    } else {
        None
    };

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
        card_lines.push(format!("My master, edition of {}", nvm.edition));
        card_lines.push(format!("{} left to press", nvm.counter));
        #[cfg(any(target_os = "stax", target_os = "flex"))]
        {
            let bitmap = master_sleeve.as_deref().unwrap_or(&[]);
            let glyph = NbglGlyph::new(
                bitmap,
                full as u16,
                full as u16,
                crate::state::ART_BPP as u8,
                false,
            );
            review = review.add_content(album_card(
                title,
                &card_lines[card_lines.len() - 2],
                &card_lines[card_lines.len() - 1],
                &glyph,
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
            let bitmap = pressing_sleeve.as_deref().unwrap_or(&[]);
            let glyph = NbglGlyph::new(
                bitmap,
                full as u16,
                full as u16,
                crate::state::ART_BPP as u8,
                false,
            );
            review = review.add_content(album_card(
                title,
                &card_lines[card_lines.len() - 1],
                "Bound to this device",
                &glyph,
            ));
        }
        // Provenance page: only facts the certificate actually authenticates.
        // The album fingerprint identifies the artist's master and edition;
        // the presser's device is deliberately absent, since a receiver has no
        // authenticated way to learn it without a protocol change.
        let album_hash = crate::crypto::sha256(&[&album.albpub])?;
        let mut album_fp = [0u8; 4];
        album_fp.copy_from_slice(&album_hash[..4]);
        names_h.push(String::from("Album fingerprint"));
        values_h.push(fingerprint_str(&album_fp));

        names_h.push(String::from("Pressing"));
        values_h.push(format!("{} of {}", pressing.number, pressing.edition));

        // Honest about the art: verified only when the stored bytes hash to
        // the sleeve the certificate signed; otherwise the record is showing
        // generative fallback art, so say so.
        let sleeve_verified = album.sleeve_hash != [0u8; 32]
            && !crate::state::Art::is_blank()
            && crate::crypto::sha256(&[crate::state::Art::get()])
                .map(|h| crate::crypto::mac_eq(&h, &album.sleeve_hash))
                .unwrap_or(false);
        names_h.push(String::from("Sleeve"));
        values_h.push(String::from(if sleeve_verified {
            "Verified"
        } else {
            "Not loaded"
        }));

        names_h.push(String::from("Edition"));
        values_h.push(String::from("Sealed"));

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

/// What the user asked for on the library screen.
#[cfg(any(target_os = "stax", target_os = "flex"))]
pub enum LibraryAction {
    /// An APDU arrived: leave the screen so the main loop can serve it.
    Apdu,
    /// The "Quitter" footer: exit the app, like the standard home does.
    Quit,
    /// The (i) affordance.
    Info,
    /// A record row was tapped; open its card.
    OpenMaster,
    OpenPressing,
    /// A swipe or an unmapped tap: just redraw the library.
    Redraw,
}

/// The library: the app's landing screen. An iTunes-style list of the records
/// this device holds, each row a decimated sleeve with its title and status,
/// over a "Quitter" footer that really exits.
///
/// It runs its own event loop, and that loop yields to the host: the library
/// is what is on screen when a ceremony begins, so it must step aside for an
/// incoming cut / pair / press instead of deadlocking against it.
#[cfg(any(target_os = "stax", target_os = "flex"))]
pub fn show_library() -> Result<LibraryAction, AppSW> {
    use crate::app_ui::library::{
        run_event_loop, Exit, Layout, ScreenArena, TOKEN_INFO, TOKEN_MASTER, TOKEN_PRESSING,
        TOKEN_QUIT,
    };

    let nvm = Store::get()?;
    let n = crate::state::ART_W;
    let half = n / 2;

    // Everything the layout points at must outlive the draw and the loop.
    let mut arena = ScreenArena::new();
    let mut strings: Vec<CString> = Vec::new();
    let mut cstr = |s: String| -> *const core::ffi::c_char {
        let owned = CString::new(s.replace('\0', " ")).unwrap_or_default();
        strings.push(owned);
        strings[strings.len() - 1].as_ptr()
    };

    let mut layout = Layout::new();
    layout.header(cstr(String::from("Enclave Records")), core::ptr::null());

    let mut has_any = false;

    if nvm.has_master == 1 {
        let title = title_str(&nvm.title, nvm.title_len)?;
        let album_id = crate::crypto::sha256(&[&nvm.alb_pub])?;
        let sleeve_hash = crate::certs::album_cert_sleeve_hash(&nvm.album_cert);
        let thumb = crate::sleeve::to_display(&crate::sleeve::decimate(
            &album_sleeve(&sleeve_hash, &album_id),
            n,
        ));
        let icon = arena.icon(thumb, half as u16, half as u16, ledger_secure_sdk_sys::NBGL_BPP_1);
        let status = if nvm.counter == 0 {
            String::from("Sold out")
        } else {
            format!("{} of {} left to press", nvm.counter, nvm.edition)
        };
        layout.touchable_bar(icon, cstr(String::from(title)), cstr(status), TOKEN_MASTER);
        has_any = true;
    }

    if nvm.has_pressing == 1 {
        let album = parse_album_cert(&nvm.pressing_album_cert)?;
        let title = title_str(&album.title, album.title_len)?;
        let pressing = crate::certs::parse_pressing_cert(&nvm.pressing_cert, &album.albpub)?;
        let thumb = crate::sleeve::to_display(&crate::sleeve::decimate(
            &album_sleeve(&album.sleeve_hash, &pressing.album_id),
            n,
        ));
        let icon = arena.icon(thumb, half as u16, half as u16, ledger_secure_sdk_sys::NBGL_BPP_1);
        let status = format!("{} of {}, on this device", pressing.number, pressing.edition);
        layout.touchable_bar(
            icon,
            cstr(String::from(title)),
            cstr(status),
            TOKEN_PRESSING,
        );
        has_any = true;
    }

    if !has_any {
        layout.text(
            cstr(String::from("No records yet")),
            cstr(String::from("Cut a master or receive a pressing.")),
        );
    }

    layout.footer(cstr(String::from("Quitter")), TOKEN_QUIT);
    layout.draw();

    let action = match run_event_loop() {
        Exit::Apdu => LibraryAction::Apdu,
        Exit::Touched(TOKEN_QUIT) => LibraryAction::Quit,
        Exit::Touched(TOKEN_INFO) => LibraryAction::Info,
        Exit::Touched(TOKEN_PRESSING) => LibraryAction::OpenPressing,
        Exit::Touched(TOKEN_MASTER) => LibraryAction::OpenMaster,
        // Any other tap or a swipe on the list just redraws the library.
        _ => LibraryAction::Redraw,
    };
    // Explicitly release the layout before the arena and strings it points at.
    drop(layout);
    Ok(action)
}

/// The (i) page: what the app is, and its version. Reuses the proven review
/// widget rather than another raw layout.
#[cfg(any(target_os = "stax", target_os = "flex"))]
pub fn show_info_screen() {
    let names = [String::from("Enclave Records"), String::from("Editions")];
    let values = [
        String::from(env!("CARGO_PKG_VERSION")),
        String::from("Finite, pressed in silicon."),
    ];
    NbglGenericReview::new()
        .add_content(fields_page(&names, &values))
        .show_from_callback("Back");
}

/// ART_TEST: development probe for the raw-NBGL path. P1 = 0 draws a
/// device-generated pattern packed with the agreed 1bpp convention; P1 = 1
/// draws whatever sleeve is currently in NVM. Both put a text label *over*
/// the image, which is the mechanism the record card needs.
///
/// The generated pattern is an "F" (asymmetric under all eight flips and
/// rotations) plus a bar flush with the top edge: the F settles the geometry,
/// the bar settles the bit order inside a byte.
#[cfg(any(target_os = "stax", target_os = "flex"))]
pub fn handler_art_test(command: Command<'_>, stage: u8) -> Result<CommandResponse<'_>, AppSW> {
    use crate::app_ui::library::{run_event_loop, Screen, ScreenArena, SCREEN_W};
    use crate::state::{Art, ART_W};

    const N: usize = ART_W;
    let mut arena = ScreenArena::new();
    let icon = if stage == 1 {
        arena.icon_static(Art::get(), N as u16, N as u16, ledger_secure_sdk_sys::NBGL_BPP_1)
    } else {
        let mut bitmap = alloc::vec![0u8; N * N / 8];
        let s = N / 16;
        for y in 0..N {
            for x in 0..N {
                let stem = x >= 3 * s && x < 4 * s && y >= 3 * s && y < 13 * s;
                let top_arm = y >= 3 * s && y < 4 * s && x >= 3 * s && x < 11 * s;
                let mid_arm = y >= 7 * s && y < 8 * s && x >= 3 * s && x < 9 * s;
                let top_bar = y < 2 && x < 4 * s;
                if stem || top_arm || mid_arm || top_bar {
                    // The convention agreed with the host-side packer: the
                    // display is the row-major decode of the buffer rotated
                    // 90 clockwise, so packing pre-rotates counter-clockwise.
                    let k = (N - 1 - x) * N + y;
                    bitmap[k >> 3] |= 0x80 >> (k & 7);
                }
            }
        }
        arena.icon(bitmap, N as u16, N as u16, ledger_secure_sdk_sys::NBGL_BPP_1)
    };
    let label = arena.text("PROBE");

    if stage != 3 {
        // The layout path, which is the one that renders.
        let mut layout = crate::app_ui::library::Layout::new();
        layout.centered_info(icon, label, core::ptr::null(), core::ptr::null(), 0);
        layout.draw();
        let _ = run_event_loop();
    } else {
        // P1 = 3 keeps the raw object path reachable for further study.
        let x0 = (SCREEN_W - N as i16) / 2;
        let y0 = 160;
        let mut screen = Screen::push(2, false);
        screen.image(icon, N as u16, N as u16, x0, y0, None);
        screen.text(
            label,
            x0,
            y0 + N as i16 - 44,
            N as u16,
            40,
            ledger_secure_sdk_sys::BAGL_FONT_INTER_SEMIBOLD_24px,
            ledger_secure_sdk_sys::BLACK,
            ledger_secure_sdk_sys::CENTER,
            None,
        );
        screen.draw();
        let _ = run_event_loop();
    }

    let response = command.into_response();
    Ok(response)
}

