use crate::certs::parse_album_cert;
use crate::handlers::press::fingerprint_str;
use crate::state::Store;
use crate::AppSW;
use alloc::ffi::CString;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use ledger_device_sdk::io::{Command, CommandResponse};

#[cfg(not(any(target_os = "stax", target_os = "flex")))]
use crate::state::PRESSED_LOG_LEN;
#[cfg(not(any(target_os = "stax", target_os = "flex")))]
use ledger_device_sdk::nbgl::{Field, NbglGenericReview, NbglPageContent, TagValueList};

#[cfg(any(target_os = "stax", target_os = "flex"))]
use ledger_device_sdk::{include_gif, nbgl::NbglGlyph};

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

/// The first 8 hex chars of SHA256(albpub): the edition's public identifier,
/// shown as "Edition ID" and the fingerprint a buyer checks against the
/// artist's channel.
fn edition_id(albpub: &[u8; crate::crypto::PUBKEY_LEN]) -> Result<String, AppSW> {
    let hash = crate::crypto::sha256(&[albpub])?;
    let mut fp = [0u8; 4];
    fp.copy_from_slice(&hash[..4]);
    Ok(fingerprint_str(&fp))
}

/// Whether the stored NVM sleeve is the one the album signature commits to.
/// Same rule as [`album_sleeve`], but returns only the verdict so the card can
/// pick its cover source without a heap copy.
fn sleeve_verified(sleeve_hash: &[u8; 32]) -> bool {
    *sleeve_hash != [0u8; 32]
        && !crate::state::Art::is_blank()
        && crate::crypto::sha256(&[crate::state::Art::get()])
            .map(|h| crate::crypto::mac_eq(&h, sleeve_hash))
            .unwrap_or(false)
}

// ==========================================================================
//  Flex / Stax: the record card, its back, and the authenticity sub-page.
// ==========================================================================

/// The circled-i info affordance. A *compiled* glyph (`include_gif`), generated
/// at build time by `build.rs`: a runtime heap icon faults under PIC relocation
/// on this target, so the info button must point at baked flash.
#[cfg(any(target_os = "stax", target_os = "flex"))]
const INFO_ICON: NbglGlyph = NbglGlyph::from_include(include_gif!("glyphs/info_nbgl.png", NBGL));

/// Which record a card shows. A device holds a master, a pressing, or (rarely)
/// both; the library row tapped selects which.
#[cfg(any(target_os = "stax", target_os = "flex"))]
#[derive(Clone, Copy)]
pub enum RecordKind {
    Master,
    Pressing,
}

/// Everything the card needs, gathered once from NVM so the draw loop can
/// repaint pages without touching the certificate again.
#[cfg(any(target_os = "stax", target_os = "flex"))]
struct CardData {
    title: String,
    artist: String,
    edition: u16,
    /// The copy number for a pressing; `None` for a master (a plate, not a copy).
    number: Option<u16>,
    /// Pressings still to press, for a master; `None` for a pressing.
    left: Option<u16>,
    edition_id: String,
    /// The album id, used to generate the fallback cover when no verified
    /// sleeve is stored.
    album_id: [u8; 32],
    /// True when the stored NVM sleeve hashes to the signed sleeve hash, so the
    /// card reads the real cover straight out of flash; otherwise it renders the
    /// generative fallback.
    sleeve_verified: bool,
}

#[cfg(any(target_os = "stax", target_os = "flex"))]
fn gather_card(kind: RecordKind) -> Result<Option<CardData>, AppSW> {
    let nvm = Store::get()?;
    match kind {
        RecordKind::Master if nvm.has_master == 1 => {
            let album_id = crate::crypto::sha256(&[&nvm.alb_pub])?;
            let sleeve_hash = crate::certs::album_cert_sleeve_hash(&nvm.album_cert);
            let (artist_buf, artist_len) = crate::certs::album_cert_artist(&nvm.album_cert);
            let mut id = [0u8; 32];
            id.copy_from_slice(&album_id);
            Ok(Some(CardData {
                title: String::from(title_str(&nvm.title, nvm.title_len)?),
                artist: String::from(title_str(&artist_buf, artist_len)?),
                edition: nvm.edition,
                number: None,
                left: Some(nvm.counter),
                edition_id: edition_id(&nvm.alb_pub)?,
                album_id: id,
                sleeve_verified: sleeve_verified(&sleeve_hash),
            }))
        }
        RecordKind::Pressing if nvm.has_pressing == 1 => {
            let album = parse_album_cert(&nvm.pressing_album_cert)?;
            let pressing = crate::certs::parse_pressing_cert(&nvm.pressing_cert, &album.albpub)?;
            Ok(Some(CardData {
                title: String::from(title_str(&album.title, album.title_len)?),
                artist: String::from(title_str(&album.artist, album.artist_len)?),
                edition: pressing.edition,
                number: Some(pressing.number),
                left: None,
                edition_id: edition_id(&album.albpub)?,
                album_id: pressing.album_id,
                sleeve_verified: sleeve_verified(&album.sleeve_hash),
            }))
        }
        _ => Ok(None),
    }
}

#[cfg(any(target_os = "stax", target_os = "flex"))]
#[derive(Clone, Copy, PartialEq)]
enum Page {
    Card,
    Back,
    Auth,
}

/// Build one tag/value pair with zeroed options (no value icon, no extension).
#[cfg(any(target_os = "stax", target_os = "flex"))]
fn pair(
    item: *const core::ffi::c_char,
    value: *const core::ffi::c_char,
) -> ledger_secure_sdk_sys::nbgl_contentTagValue_t {
    let mut tv: ledger_secure_sdk_sys::nbgl_contentTagValue_t = unsafe { core::mem::zeroed() };
    tv.item = item;
    tv.value = value;
    tv
}

/// Draw one page of the record card and block until the user acts (or an APDU
/// arrives). Everything NBGL keeps a pointer into is local here and outlives the
/// wait, so each call is self-contained.
#[cfg(any(target_os = "stax", target_os = "flex"))]
fn draw_page(card: &CardData, page: Page) -> Result<crate::app_ui::library::Exit, AppSW> {
    use crate::app_ui::library::{
        run_event_loop, Layout, ScreenArena, TOKEN_BACK, TOKEN_INFO, TOKEN_PAGER,
    };
    use ledger_secure_sdk_sys::nbgl_contentTagValue_t;

    let mut arena = ScreenArena::new();
    let mut strings: Vec<CString> = Vec::new();
    let mut cstr = |s: String| -> *const core::ffi::c_char {
        let owned = CString::new(s.replace('\0', " ")).unwrap_or_default();
        strings.push(owned);
        strings[strings.len() - 1].as_ptr()
    };

    let info_details: ledger_secure_sdk_sys::nbgl_icon_details_t = (&INFO_ICON).into();
    let mut pairs: Vec<nbgl_contentTagValue_t> = Vec::new();

    let mut layout = Layout::new();

    match page {
        Page::Card => {
            layout.header(cstr(String::from("Enclave Records")), core::ptr::null());
            let cover = if card.sleeve_verified {
                crate::sleeve::Cover::Canonical(crate::state::Art::get())
            } else {
                crate::sleeve::Cover::Fallback(card.album_id)
            };
            let (bmp, w, h) = crate::sleeve::record_card(&cover, crate::state::ART_W, card.number);
            let icon = arena.icon(bmp, w, h, ledger_secure_sdk_sys::NBGL_BPP_1);
            layout.centered_info(
                icon,
                cstr(card.title.clone()),
                core::ptr::null(),
                core::ptr::null(),
                0,
            );
            layout.split_footer(
                cstr(String::from("Back")),
                TOKEN_BACK,
                cstr(String::from("< 1 of 2 >")),
                TOKEN_PAGER,
            );
        }
        Page::Back => {
            layout.header(cstr(String::from("Enclave Records")), core::ptr::null());
            let copy_value = match card.number {
                Some(n) => format!("#{} of {}", n, card.edition),
                None => format!("Master plate of {}", card.edition),
            };
            pairs.push(pair(cstr(String::from("Copy")), cstr(copy_value)));
            let artist = if card.artist.is_empty() {
                String::from("Unknown artist")
            } else {
                card.artist.clone()
            };
            pairs.push(pair(cstr(String::from("Artist")), cstr(artist)));
            pairs.push(pair(cstr(String::from("Album")), cstr(card.title.clone())));
            layout.tag_value_list(&pairs);
            // The Edition ID gets its own touchable row carrying the compiled
            // (i) glyph: tapping it opens the authenticity page. A top-right
            // header button faults on tap in this raw-layout context, but a
            // touchable bar (the same primitive the library rows use) is sound.
            layout.touchable_bar(
                &info_details,
                cstr(String::from("Edition ID")),
                cstr(card.edition_id.clone()),
                TOKEN_INFO,
            );
            layout.split_footer(
                cstr(String::from("Back")),
                TOKEN_BACK,
                cstr(String::from("< 2 of 2 >")),
                TOKEN_PAGER,
            );
        }
        Page::Auth => {
            layout.header(cstr(String::from("Authenticity")), core::ptr::null());
            let proven = match card.number {
                Some(n) => format!("Copy #{} of edition {}: genuine artwork, bound to this device", n, card.edition),
                None => format!("The master of edition {}: genuine artwork, bound to this device", card.edition),
            };
            pairs.push(pair(cstr(String::from("This device proves")), cstr(proven)));
            pairs.push(pair(
                cstr(String::from("It cannot prove")),
                cstr(String::from("The album key may not be the real artist's; a copycat could reuse this art")),
            ));
            pairs.push(pair(
                cstr(String::from("Check the Edition ID")),
                cstr(format!("Confirm {} on the artist's channel", card.edition_id)),
            ));
            layout.tag_value_list(&pairs);
            layout.footer(cstr(String::from("Back")), TOKEN_BACK);
        }
    }

    layout.draw();
    drop(cstr);
    Ok(run_event_loop())
}

/// Show a record's card: page 1 the cover with its "#N" and reflection, page 2
/// the back-of-record fields, and (from the back's info affordance) the
/// authenticity page. Blocks until "Back" from the card, or an incoming APDU.
#[cfg(any(target_os = "stax", target_os = "flex"))]
pub fn show_record_card(kind: RecordKind) -> Result<(), AppSW> {
    use crate::app_ui::library::{Exit, Layout, TOKEN_BACK};

    let Some(card) = gather_card(kind)? else {
        // Empty: a single page with a Back footer.
        let mut strings: Vec<CString> = Vec::new();
        let mut cstr = |s: &str| -> *const core::ffi::c_char {
            strings.push(CString::new(s).unwrap_or_default());
            strings[strings.len() - 1].as_ptr()
        };
        let mut layout = Layout::new();
        layout.header(cstr("Enclave Records"), core::ptr::null());
        layout.text(cstr("Empty"), cstr("Cut a master or receive a pressing."));
        layout.footer(cstr("Back"), TOKEN_BACK);
        layout.draw();
        drop(cstr);
        let _ = crate::app_ui::library::run_event_loop();
        return Ok(());
    };

    let mut page = Page::Card;
    loop {
        match draw_page(&card, page)? {
            Exit::Apdu => return Ok(()),
            Exit::Touched(TOKEN_BACK) => match page {
                Page::Auth => page = Page::Back,
                _ => return Ok(()),
            },
            Exit::Touched(crate::app_ui::library::TOKEN_INFO) => page = Page::Auth,
            Exit::Touched(crate::app_ui::library::TOKEN_PAGER) => {
                page = if page == Page::Card { Page::Back } else { Page::Card };
            }
            _ => {}
        }
    }
}

// ==========================================================================
//  Nano (button home): the original review-based collection screen.
// ==========================================================================

#[cfg(not(any(target_os = "stax", target_os = "flex")))]
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

/// The collection as review pages (Nano's button-driven home).
#[cfg(not(any(target_os = "stax", target_os = "flex")))]
pub fn show_collection_screen() -> Result<(), AppSW> {
    let nvm = Store::get()?;
    let mut review = NbglGenericReview::new();
    let mut any = false;

    let mut names_m: Vec<String> = Vec::new();
    let mut values_m: Vec<String> = Vec::new();
    let mut names_h: Vec<String> = Vec::new();
    let mut values_h: Vec<String> = Vec::new();

    if nvm.has_master == 1 {
        any = true;
        let (artist_buf, artist_len) = crate::certs::album_cert_artist(&nvm.album_cert);
        names_m.push(String::from("Album"));
        values_m.push(String::from(title_str(&nvm.title, nvm.title_len)?));
        names_m.push(String::from("Artist"));
        values_m.push(String::from(title_str(&artist_buf, artist_len)?));
        names_m.push(String::from("Still to press"));
        values_m.push(format!("{} of {}", nvm.counter, nvm.edition));
        let pressed = (nvm.edition - nvm.counter) as usize;
        for entry in nvm.pressed_log.iter().take(pressed.min(PRESSED_LOG_LEN)) {
            if entry.number == 0 {
                continue;
            }
            names_m.push(format!("Pressed {} of {}", entry.number, nvm.edition));
            values_m.push(format!("for device {}", fingerprint_str(&entry.recipient_fp)));
        }
        review = review.add_content(fields_page(&names_m, &values_m));
    }

    if nvm.has_pressing == 1 {
        any = true;
        let album = parse_album_cert(&nvm.pressing_album_cert)?;
        let pressing = crate::certs::parse_pressing_cert(&nvm.pressing_cert, &album.albpub)?;
        names_h.push(String::from("Album"));
        values_h.push(String::from(title_str(&album.title, album.title_len)?));
        names_h.push(String::from("Artist"));
        values_h.push(String::from(title_str(&album.artist, album.artist_len)?));
        names_h.push(String::from("Copy"));
        values_h.push(format!("#{} of {}", pressing.number, pressing.edition));
        names_h.push(String::from("Edition ID"));
        values_h.push(edition_id(&album.albpub)?);
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

/// COLLECTION over APDU: the record card on Flex/Stax, the review on Nano.
pub fn handler_collection(command: Command<'_>) -> Result<CommandResponse<'_>, AppSW> {
    #[cfg(any(target_os = "stax", target_os = "flex"))]
    {
        let nvm = Store::get()?;
        let kind = if nvm.has_master == 1 {
            RecordKind::Master
        } else {
            RecordKind::Pressing
        };
        show_record_card(kind)?;
    }
    #[cfg(not(any(target_os = "stax", target_os = "flex")))]
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
/// A drawn library, kept alive between APDUs. NBGL keeps raw pointers into the
/// layout's strings and bitmaps, so the arena and string store must outlive the
/// layout; struct fields drop in declaration order, so `layout` (whose `Drop`
/// releases the NBGL handle) is listed first and released before the memory it
/// points at.
#[cfg(any(target_os = "stax", target_os = "flex"))]
pub struct Library {
    _layout: crate::app_ui::library::Layout,
    _arena: crate::app_ui::library::ScreenArena,
    _strings: Vec<CString>,
}

#[cfg(any(target_os = "stax", target_os = "flex"))]
impl Library {
    /// Build the library from fresh NVM and draw it. The returned handle keeps
    /// the screen (and its touch objects) live until dropped.
    pub fn draw() -> Result<Library, AppSW> {
        use crate::app_ui::library::{Layout, ScreenArena, TOKEN_MASTER, TOKEN_PRESSING, TOKEN_QUIT};

        let nvm = Store::get()?;
        let n = crate::state::ART_W;
        let half = n / 2;

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
                String::from("Your master \u{00b7} sold out")
            } else {
                format!("Your master \u{00b7} {} of {} left", nvm.counter, nvm.edition)
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
            let status = format!("#{} / {}", pressing.number, pressing.edition);
            layout.touchable_bar(icon, cstr(String::from(title)), cstr(status), TOKEN_PRESSING);
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

        drop(cstr);
        Ok(Library {
            _layout: layout,
            _arena: arena,
            _strings: strings,
        })
    }

    /// Yield to the host and the finger: block until an APDU is pending or the
    /// user acts, without repainting.
    pub fn wait(&self) -> LibraryAction {
        use crate::app_ui::library::{run_event_loop, Exit, TOKEN_MASTER, TOKEN_PRESSING, TOKEN_QUIT};
        match run_event_loop() {
            Exit::Apdu => LibraryAction::Apdu,
            Exit::Touched(TOKEN_QUIT) => LibraryAction::Quit,
            Exit::Touched(TOKEN_PRESSING) => LibraryAction::OpenPressing,
            Exit::Touched(TOKEN_MASTER) => LibraryAction::OpenMaster,
            _ => LibraryAction::Redraw,
        }
    }
}

/// ART_TEST: development probe for the raw-NBGL path. P1 = 0 draws a
/// device-generated pattern packed with the agreed 1bpp convention; P1 = 1
/// draws whatever sleeve is currently in NVM. Both put a text label *over*
/// the image, which is the mechanism the record card needs.
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
                    let k = (N - 1 - x) * N + y;
                    bitmap[k >> 3] |= 0x80 >> (k & 7);
                }
            }
        }
        arena.icon(bitmap, N as u16, N as u16, ledger_secure_sdk_sys::NBGL_BPP_1)
    };
    let label = arena.text("PROBE");

    if stage != 3 {
        let mut layout = crate::app_ui::library::Layout::new();
        layout.centered_info(icon, label, core::ptr::null(), core::ptr::null(), 0);
        layout.draw();
        let _ = run_event_loop();
    } else {
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
