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

/// The sleeve this device should show for an album: the art stored in that
/// record's own slot when its hash matches the one signed into the album
/// certificate, otherwise the generated label art.
///
/// A mismatch is not an error to report but a fact to render honestly: the
/// device shows the album's own generated face rather than a bitmap the
/// signed identity does not vouch for. An all-zero `sleeve_hash` means the
/// edition was cut with no sleeve, so it too falls back.
pub fn album_sleeve(sleeve_hash: &[u8; 32], album_id: &[u8; 32], slot: usize) -> alloc::vec::Vec<u8> {
    let unbound = *sleeve_hash == [0u8; 32];
    if !unbound && !crate::state::Art::is_blank(slot) {
        let art = crate::state::Art::get(slot);
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

/// The first 8 hex chars of SHA256(devpub): the device's public identity, shown
/// as "Device ID" (the device that physically holds this record). The same
/// fingerprint a press is addressed to ("For device ...").
fn device_id(devpub: &[u8; crate::crypto::PUBKEY_LEN]) -> Result<String, AppSW> {
    let hash = crate::crypto::sha256(&[devpub])?;
    let mut fp = [0u8; 4];
    fp.copy_from_slice(&hash[..4]);
    Ok(fingerprint_str(&fp))
}

/// Whether the sleeve in a record's slot is the one its album signature commits
/// to. Same rule as [`album_sleeve`], but returns only the verdict so the card
/// can pick its cover source without a heap copy.
fn sleeve_verified(sleeve_hash: &[u8; 32], slot: usize) -> bool {
    *sleeve_hash != [0u8; 32]
        && !crate::state::Art::is_blank(slot)
        && crate::crypto::sha256(&[crate::state::Art::get(slot)])
            .map(|h| crate::crypto::mac_eq(&h, sleeve_hash))
            .unwrap_or(false)
}

// ==========================================================================
//  Flex / Stax: the record card, its back, and the authenticity sub-page.
// ==========================================================================

/// The right-pointing chevron ("›") that ends each back-of-record row: the tap
/// affordance for the row's sub-page. A *compiled* glyph (`include_gif`),
/// generated at build time by `build.rs`, because a runtime heap icon faults
/// under PIC relocation on this target, so it must point at baked flash.
#[cfg(any(target_os = "stax", target_os = "flex"))]
const CHEVRON_ICON: NbglGlyph =
    NbglGlyph::from_include(include_gif!("glyphs/chevron_nbgl.png", NBGL));

/// The separator between the two facts of a library row's status line
/// ("Master record{SEP}N of M left to press"). The device font ships no dot
/// glyph (neither the U+2022 bullet nor the U+00B7 middle dot render: they drop
/// out to a bare gap), so the separator is a hyphen, which does render. One
/// space on each side.
#[cfg(any(target_os = "stax", target_os = "flex"))]
const ROW_SEP: &str = " - ";

/// Shorten `text` so it renders on a single line no wider than `max_px` in
/// `font`, appending an ASCII ellipsis when it had to cut. A library row's
/// title must never wrap: a wrapped title grows the row and shoves the status
/// line into the record below it. Measured with the SDK's own metrics so it
/// holds for any title, not a guessed character budget.
#[cfg(any(target_os = "stax", target_os = "flex"))]
fn fit_one_line(text: &str, font: ledger_secure_sdk_sys::nbgl_font_id_e, max_px: u16) -> String {
    let measure = |s: &str| -> u16 {
        let c = CString::new(s).unwrap_or_default();
        unsafe { ledger_secure_sdk_sys::nbgl_getSingleLineTextWidth(font, c.as_ptr()) }
    };
    if measure(text) <= max_px {
        return String::from(text);
    }
    let ellipsis = "...";
    let mut end = text.len();
    while end > 0 {
        end -= 1;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        let candidate = format!("{}{}", text[..end].trim_end(), ellipsis);
        if measure(&candidate) <= max_px {
            return candidate;
        }
    }
    String::from(ellipsis)
}

/// Widest a library row's title may be before it is truncated: the row's inner
/// width less the thumbnail on its left and a margin so the status line's first
/// word is never crowded. Flex is 480 wide; the value is deliberately
/// conservative so the title always clears the thumbnail on one line.
#[cfg(any(target_os = "stax", target_os = "flex"))]
const ROW_TITLE_MAX_PX: u16 = 300;

/// A back-of-record row, laid out label-left / value-right: the label, then
/// enough spaces that the value's right edge lands near `right_px`, so the
/// values line up in a column just left of the chevron. Padded with real spaces
/// (the bar's text is a single left-aligned string; NBGL keeps interior runs of
/// spaces), and the target is kept short of the text area so the row can never
/// wrap. Measured with the SDK metrics so the column holds whatever the value.
#[cfg(any(target_os = "stax", target_os = "flex"))]
fn label_value_row(
    label: &str,
    value: &str,
    font: ledger_secure_sdk_sys::nbgl_font_id_e,
    right_px: u16,
) -> String {
    let measure = |s: &str| -> u16 {
        let c = CString::new(s).unwrap_or_default();
        unsafe { ledger_secure_sdk_sys::nbgl_getSingleLineTextWidth(font, c.as_ptr()) }
    };
    let space_w = measure("A A").saturating_sub(measure("AA")).max(1);
    let min_gap = 2usize;
    let base = format!("{}{}{}", label, " ".repeat(min_gap), value);
    let base_w = measure(&base);
    if base_w >= right_px {
        return base;
    }
    let extra = ((right_px - base_w) / space_w) as usize;
    format!("{}{}{}", label, " ".repeat(min_gap + extra), value)
}

/// Where the value column's right edge sits on the back-of-record rows: short
/// of the ~372px text area (416 inner, less the chevron and its interval) so a
/// padded row never wraps, and clear enough of the chevron to read as a column.
#[cfg(any(target_os = "stax", target_os = "flex"))]
const ROW_VALUE_RIGHT_PX: u16 = 344;

/// How many rows the back of the record carries. Fixed, and fixed by type: the
/// rows are built into an array of this length, so a fifth one does not compile.
///
/// The list area between the header's rule (y=96) and the split footer's (y=504)
/// is 408px on Flex and a touchable bar is 92px, so four rows fit and a fifth is
/// drawn under the footer, showing 8px of glyph tops above the rule and nothing
/// else. Worse, the count must not depend on what the device happens to hold: a
/// row that appears only for a copy that changed hands clips the page on exactly
/// the devices that travelled, which is the state nobody tests. A fact with no
/// row of its own goes on the sub-page of the row it qualifies.
#[cfg(any(target_os = "stax", target_os = "flex"))]
const BACK_ROWS: usize = 4;

/// A library row's status line, indented to start under the row's title rather
/// than at the screen's left edge. NBGL's touchable bar pushes a bar's sub-text
/// back to the container's left margin (under the thumbnail), while the title is
/// inset past the thumbnail; left as-is the two lines don't share a left edge.
/// Prepending spaces worth the thumbnail's inset (its width plus the bar's
/// interval) lines the status up with the title, so title and status read as a
/// single text column to the right of the thumbnail. Measured in the sub-text's
/// own font.
#[cfg(any(target_os = "stax", target_os = "flex"))]
fn indent_subtext(status: &str, indent_px: u16) -> String {
    let font = ledger_secure_sdk_sys::BAGL_FONT_INTER_REGULAR_28px;
    let measure = |s: &str| -> u16 {
        let c = CString::new(s).unwrap_or_default();
        unsafe { ledger_secure_sdk_sys::nbgl_getSingleLineTextWidth(font, c.as_ptr()) }
    };
    let space_w = measure("A A").saturating_sub(measure("AA")).max(1);
    let n = (indent_px / space_w) as usize;
    format!("{}{}", " ".repeat(n), status)
}

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
    edition_id: String,
    /// The fingerprint of the device holding this record, shown as the
    /// "Device ID" on the back of the card.
    device_id: String,
    /// The album id, used to generate the fallback cover when no verified
    /// sleeve is stored.
    album_id: [u8; 32],
    /// True when the sleeve in this record's slot hashes to the signed sleeve
    /// hash, so the card reads the real cover straight out of flash; otherwise
    /// it renders the generative fallback.
    sleeve_verified: bool,
    /// Which art slot holds this record's sleeve: the master's or the pressing's.
    slot: usize,
    /// The fingerprint of the device this copy was handed over by, when there
    /// is one. Empty for a copy that came straight from a press, and for a
    /// master: only a transfer has a previous holder to name.
    received_from: String,
    /// How many holders came before that one. A count and not their
    /// fingerprints: the proof of them is the chain digest, which is not
    /// readable as a list of names, and a list that grows with every hop is a
    /// screen that eventually runs off its bottom edge. The count states the
    /// same fact in a fixed number of characters.
    earlier: u8,
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
                edition_id: edition_id(&nvm.alb_pub)?,
                device_id: device_id(&nvm.dev_pub)?,
                album_id: id,
                sleeve_verified: sleeve_verified(&sleeve_hash, crate::state::SLOT_MASTER),
                slot: crate::state::SLOT_MASTER,
                received_from: String::new(),
                earlier: 0,
            }))
        }
        RecordKind::Pressing if nvm.has_pressing == 1 => {
            let album = parse_album_cert(&nvm.pressing_album_cert)?;
            let pressing = crate::certs::parse_pressing_cert(&nvm.pressing_cert, &album.albpub)?;
            let received_from = if nvm.has_from == 1 {
                device_id(&nvm.pressing_from)?
            } else {
                String::new()
            };
            // The newest hop is the device the provenance names; only what lies
            // behind it is "earlier".
            let earlier = nvm.hops.saturating_sub(1);
            Ok(Some(CardData {
                title: String::from(title_str(&album.title, album.title_len)?),
                artist: String::from(title_str(&album.artist, album.artist_len)?),
                edition: pressing.edition,
                number: Some(pressing.number),
                edition_id: edition_id(&album.albpub)?,
                device_id: device_id(&nvm.dev_pub)?,
                album_id: pressing.album_id,
                sleeve_verified: sleeve_verified(&album.sleeve_hash, crate::state::SLOT_PRESSING),
                slot: crate::state::SLOT_PRESSING,
                received_from,
                earlier,
            }))
        }
        _ => Ok(None),
    }
}

#[cfg(any(target_os = "stax", target_os = "flex"))]
#[derive(Clone, Copy, PartialEq)]
enum Page {
    /// The cover, with the big `#N` numeral and the title + artist.
    Card,
    /// The back-envelope info: the number, the Edition ID, the Device ID, and a
    /// "Learn more" row, each opening a sub-page.
    Back,
    /// "#N of M", explained: what the numbering means and the sealed counter.
    InfoNumber,
    /// The Edition ID, explained: what it is and how to confirm it.
    InfoEdition,
    /// The Device ID, explained: the device that holds the record, and where it
    /// came from before that.
    InfoDevice,
    /// The limits of the model: what the device can and cannot prove.
    LearnMore,
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
        pager_label, run_event_loop, Layout, ScreenArena, TOKEN_BACK, TOKEN_INFO_DEVICE,
        TOKEN_INFO_EDITION, TOKEN_INFO_NUMBER, TOKEN_LEARN_MORE, TOKEN_PAGER,
    };
    use ledger_secure_sdk_sys::nbgl_contentTagValue_t;

    let mut arena = ScreenArena::new();
    let mut strings: Vec<CString> = Vec::new();
    let mut cstr = |s: String| -> *const core::ffi::c_char {
        let owned = CString::new(s.replace('\0', " ")).unwrap_or_default();
        strings.push(owned);
        strings[strings.len() - 1].as_ptr()
    };

    let chevron_details: ledger_secure_sdk_sys::nbgl_icon_details_t = (&CHEVRON_ICON).into();
    let mut pairs: Vec<nbgl_contentTagValue_t> = Vec::new();

    // "#N of M" everywhere the number is shown; the master plate is #0, its own
    // marker in the same numbering as the copies it presses.
    let number_line = match card.number {
        Some(n) => format!("#{} of {}", n, card.edition),
        None => format!("#0 of {}", card.edition),
    };

    let mut layout = Layout::new();

    match page {
        Page::Card => {
            layout.header(cstr(String::from("Enclave Records")), core::ptr::null());
            let cover = if card.sleeve_verified {
                crate::sleeve::Cover::Canonical(crate::state::Art::get(card.slot))
            } else {
                crate::sleeve::Cover::Fallback(card.album_id)
            };
            // The master shows a big "#0"; a pressing shows its own "#N".
            let numeral = Some(card.number.unwrap_or(0));
            let (bmp, w, h) = crate::sleeve::record_card(&cover, crate::state::ART_W, numeral);
            let icon = arena.icon(bmp, w, h, ledger_secure_sdk_sys::NBGL_BPP_1);
            // The artist sits under the title on the front of the card.
            let artist = if card.artist.is_empty() {
                core::ptr::null()
            } else {
                cstr(card.artist.clone())
            };
            layout.centered_info(icon, cstr(card.title.clone()), artist, core::ptr::null(), 0);
            layout.split_footer(
                cstr(String::from("Back")),
                TOKEN_BACK,
                cstr(pager_label(1)),
                TOKEN_PAGER,
            );
        }
        Page::Back => {
            // The back envelope, as navigable "settings rows": one fact per row,
            // its label on the left and value inlined toward the right, ending in
            // a chevron that is the tap affordance into the fact's sub-page. The
            // repeated left (i) is gone: the chevron carries the affordance.
            layout.header(cstr(String::from("Enclave Records")), core::ptr::null());
            // Single-line rows only, and exactly [`BACK_ROWS`] of them: a bar
            // tall enough to stack a value under its label costs the page a row
            // it does not have. The value is right-aligned into a column (see
            // `label_value_row`); "Learn more" is an action row and has none.
            let vfont = ledger_secure_sdk_sys::BAGL_FONT_INTER_SEMIBOLD_28px;
            let row = |label, value| label_value_row(label, value, vfont, ROW_VALUE_RIGHT_PX);
            let rows: [(String, u8); BACK_ROWS] = [
                (row("Number", &number_line), TOKEN_INFO_NUMBER),
                (row("Edition ID", &card.edition_id), TOKEN_INFO_EDITION),
                (row("Device ID", &card.device_id), TOKEN_INFO_DEVICE),
                (String::from("Learn more"), TOKEN_LEARN_MORE),
            ];
            for (text, token) in rows {
                layout.nav_row(cstr(text), &chevron_details, token);
            }
            layout.split_footer(
                cstr(String::from("Back")),
                TOKEN_BACK,
                cstr(pager_label(2)),
                TOKEN_PAGER,
            );
        }
        Page::InfoNumber => {
            layout.header(cstr(String::from("The number")), core::ptr::null());
            let meaning = match card.number {
                Some(_) => format!(
                    "One of {} numbered copies. The master plate is #0; copies run #1 to #{}.",
                    card.edition, card.edition
                ),
                None => format!(
                    "The master plate, numbered #0. It presses the copies #1 to #{}.",
                    card.edition
                ),
            };
            pairs.push(pair(cstr(number_line.clone()), cstr(meaning)));
            pairs.push(pair(
                cstr(String::from("Sealed in silicon")),
                cstr(format!(
                    "A counter inside the chip fixes the edition at {} and only counts down.",
                    card.edition
                )),
            ));
            layout.tag_value_list(&pairs);
            layout.footer(cstr(String::from("Back")), TOKEN_BACK);
        }
        Page::InfoEdition => {
            layout.header(cstr(String::from("Edition ID")), core::ptr::null());
            pairs.push(pair(
                cstr(card.edition_id.clone()),
                cstr(String::from(
                    "A fingerprint of the album key, the same on every copy of this edition.",
                )),
            ));
            pairs.push(pair(
                cstr(String::from("How to verify")),
                cstr(format!(
                    "Confirm {} through the artist's official channels.",
                    card.edition_id
                )),
            ));
            layout.tag_value_list(&pairs);
            layout.footer(cstr(String::from("Back")), TOKEN_BACK);
        }
        Page::InfoDevice => {
            layout.header(cstr(String::from("Device ID")), core::ptr::null());
            pairs.push(pair(
                cstr(card.device_id.clone()),
                cstr(String::from(
                    "The fingerprint of this device, the one that physically holds the record.",
                )),
            ));
            // Where the record came from, beside who holds it now: the two
            // halves of one question, on one page, instead of a fact stranded
            // on an envelope that has no room for it. Always stated, so the
            // page has one shape, and the copy that never moved says so rather
            // than leaving the reader to guess what an absent row means.
            //
            // Two pairs and never more, and this one runs to four lines of
            // value: the last line this page can render starts at y=468, and
            // the fifth would land under the footer. A trail that grows a line
            // per hop passes that, then passes the bottom of the screen, which
            // faults the draw rather than merely clipping it. Hence a count of
            // the earlier holders and not their names: unsigned either way, and
            // the count is the same width however far the copy has travelled.
            let mut origin = match (card.number, card.received_from.is_empty()) {
                (None, _) => String::from("Cut on this device. A master plate is never handed on."),
                (_, true) => String::from("Pressed onto this device from the master."),
                _ => format!("{}, the one handover this device can prove.", card.received_from),
            };
            if card.earlier > 0 {
                origin += &format!(
                    " Before it: {} more, carried with the copy, unproven.",
                    card.earlier
                );
            }
            pairs.push(pair(cstr(String::from("Where it came from")), cstr(origin)));
            layout.tag_value_list(&pairs);
            layout.footer(cstr(String::from("Back")), TOKEN_BACK);
        }
        Page::LearnMore => {
            layout.header(cstr(String::from("Learn more")), core::ptr::null());
            pairs.push(pair(
                cstr(String::from("This device proves")),
                cstr(String::from(
                    "Genuine artwork from this edition, and that this device holds it.",
                )),
            ));
            pairs.push(pair(
                cstr(String::from("It cannot prove")),
                cstr(String::from(
                    "That the album key is the real artist's. Check the Edition ID.",
                )),
            ));
            layout.tag_value_list(&pairs);
            layout.footer(cstr(String::from("Back")), TOKEN_BACK);
        }
    }

    layout.draw();
    drop(cstr);
    Ok(run_event_loop())
}

/// Show a record's card: the cover with its "#N" and the title + artist, the
/// back-envelope info list, and the four sub-pages reached from its (i) rows.
/// Blocks until "Back" from the card or the back, or an incoming APDU.
#[cfg(any(target_os = "stax", target_os = "flex"))]
pub fn show_record_card(kind: RecordKind) -> Result<(), AppSW> {
    use crate::app_ui::library::{Layout, TOKEN_BACK};

    let Some(card) = gather_card(kind)? else {
        // Empty: a single page with a Back footer.
        let mut strings: Vec<CString> = Vec::new();
        let mut cstr = |s: &str| -> *const core::ffi::c_char {
            strings.push(CString::new(s).unwrap_or_default());
            strings[strings.len() - 1].as_ptr()
        };
        let mut layout = Layout::new();
        layout.header(cstr("Enclave Records"), core::ptr::null());
        layout.text(cstr("No records yet"), cstr("Cut a master or receive a pressing."));
        layout.footer(cstr("Back"), TOKEN_BACK);
        layout.draw();
        drop(cstr);
        let _ = crate::app_ui::library::run_event_loop();
        return Ok(());
    };

    card_loop(&card)
}

/// The card's page state machine: front <-> back through the pager, each back
/// (i) row opening its sub-page, Back leaving the card. Shared by the live card
/// and the development card-preview probe.
#[cfg(any(target_os = "stax", target_os = "flex"))]
fn card_loop(card: &CardData) -> Result<(), AppSW> {
    use crate::app_ui::library::{
        Exit, TOKEN_BACK, TOKEN_INFO_DEVICE, TOKEN_INFO_EDITION, TOKEN_INFO_NUMBER,
        TOKEN_LEARN_MORE, TOKEN_PAGER,
    };

    let mut page = Page::Card;
    loop {
        match draw_page(card, page)? {
            Exit::Apdu => return Ok(()),
            Exit::Touched(TOKEN_BACK) => match page {
                // Back from a sub-page returns to the back envelope; Back from
                // the card or the back leaves for the library.
                Page::InfoNumber | Page::InfoEdition | Page::InfoDevice | Page::LearnMore => {
                    page = Page::Back
                }
                _ => return Ok(()),
            },
            Exit::Touched(TOKEN_INFO_NUMBER) => page = Page::InfoNumber,
            Exit::Touched(TOKEN_INFO_EDITION) => page = Page::InfoEdition,
            Exit::Touched(TOKEN_INFO_DEVICE) => page = Page::InfoDevice,
            Exit::Touched(TOKEN_LEARN_MORE) => page = Page::LearnMore,
            Exit::Touched(TOKEN_PAGER) => {
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
        names_m.push(String::from("Number"));
        values_m.push(format!("#0 of {}", nvm.edition));
        names_m.push(String::from("Left to press"));
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
        names_h.push(String::from("Number"));
        values_h.push(format!("#{} of {}", pressing.number, pressing.edition));
        names_h.push(String::from("Edition ID"));
        values_h.push(edition_id(&album.albpub)?);
        review = review.add_content(fields_page(&names_h, &values_h));
    }

    if !any {
        names_m.push(String::from("No records yet"));
        values_m.push(String::from("Cut a master or receive a pressing."));
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
    /// The "Quit" footer: exit the app, like the standard home does.
    Quit,
    /// A record row was tapped; open its card.
    OpenMaster,
    OpenPressing,
    /// A swipe or an unmapped tap: just redraw the library.
    Redraw,
}

/// The library: the app's landing screen. An iTunes-style list of the records
/// this device holds, each row a decimated sleeve with its title and status,
/// over a "Quit" footer that really exits.
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
    ///
    /// One row per record held, always, whatever the count. The layout does not
    /// change shape with the number of records: a single record reads the same
    /// as two, and the row is the tap target in both cases. The full-size sleeve
    /// lives on the record card, one tap away.
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

        // One list row: the decimated sleeve as a thumbnail, the title fitted to
        // a single line, and the status indented to share the title's left edge.
        let row = |arena: &mut ScreenArena,
                   layout: &mut Layout,
                   cstr: &mut dyn FnMut(String) -> *const core::ffi::c_char,
                   canonical: alloc::vec::Vec<u8>,
                   title: &str,
                   status: String,
                   token: u8| {
            let thumb = crate::sleeve::to_display(&crate::sleeve::decimate(&canonical, n));
            let icon = arena.icon(thumb, half as u16, half as u16, ledger_secure_sdk_sys::NBGL_BPP_1);
            let title_fit = fit_one_line(
                title,
                ledger_secure_sdk_sys::BAGL_FONT_INTER_SEMIBOLD_28px,
                ROW_TITLE_MAX_PX,
            );
            let status_line = indent_subtext(&status, half as u16 + 16);
            layout.touchable_bar(icon, cstr(title_fit), cstr(status_line), token);
        };

        if nvm.has_master == 1 {
            let title = title_str(&nvm.title, nvm.title_len)?;
            let album_id = crate::crypto::sha256(&[&nvm.alb_pub])?;
            let sleeve_hash = crate::certs::album_cert_sleeve_hash(&nvm.album_cert);
            let canonical = album_sleeve(&sleeve_hash, &album_id, crate::state::SLOT_MASTER);
            // "Master" (not "Master record") so the label plus "N of M left" fits
            // the indented text column on one line.
            let status = if nvm.counter == 0 {
                format!("Master{}Sold out", ROW_SEP)
            } else {
                format!("Master{}{} of {} left", ROW_SEP, nvm.counter, nvm.edition)
            };
            row(&mut arena, &mut layout, &mut cstr, canonical, title, status, TOKEN_MASTER);
            has_any = true;
        }

        if nvm.has_pressing == 1 {
            let album = parse_album_cert(&nvm.pressing_album_cert)?;
            let title = title_str(&album.title, album.title_len)?;
            let pressing = crate::certs::parse_pressing_cert(&nvm.pressing_cert, &album.albpub)?;
            let canonical = album_sleeve(
                &album.sleeve_hash,
                &pressing.album_id,
                crate::state::SLOT_PRESSING,
            );
            // A promised copy is still on the shelf and still recoverable:
            // only re-running the ceremony with that one recipient finishes
            // the handover. So the row states the unfinished act and names the
            // device to reconnect, rather than a handover that ended, which is
            // what would stop the owner from ever trying. The recipient comes
            // from `committed_to`, SHA256 of its devpub, the same preimage
            // every other fingerprint on screen derives from, so naming it
            // stores nothing and sends nothing. Promised and flown read the
            // same here on purpose: the owner's next move is identical in both
            // (find that device), and only the cancel path cares which it is.
            let status = if nvm.committed != 0 {
                let mut fp = [0u8; 4];
                fp.copy_from_slice(&nvm.committed_to[..4]);
                format!(
                    "#{} of {}{}promised, reconnect {}",
                    pressing.number,
                    pressing.edition,
                    ROW_SEP,
                    fingerprint_str(&fp)
                )
            } else {
                format!("#{} of {}", pressing.number, pressing.edition)
            };
            row(&mut arena, &mut layout, &mut cstr, canonical, title, status, TOKEN_PRESSING);
            has_any = true;
        }

        if !has_any {
            // An empty shelf reads two ways, and the device knows which one it
            // is: a giver who just watched a copy leave needs that confirmed on
            // screen, not the newcomer's invitation.
            let (head, body) = if nvm.has_given == 1 {
                ("No records here", "You gave your copy away.")
            } else {
                ("No records yet", "Cut a master or receive a pressing.")
            };
            // ...and it says who it is. A device holding a record shows its
            // Device ID on the back of the card, but an empty-handed one has no
            // card to open, and that is exactly the device a giver has to
            // identify: a committed row says "reconnect XXXXXXXX", and matching
            // it against the candidate in front of you is otherwise impossible.
            layout.text(
                cstr(String::from(head)),
                cstr(format!("{}\n\nDevice ID {}", body, device_id(&nvm.dev_pub)?)),
            );
        }

        // Every row is its own tap target, so the footer carries only "Quit".
        layout.footer(cstr(String::from("Quit")), TOKEN_QUIT);
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
#[cfg(all(feature = "artprobe", any(target_os = "stax", target_os = "flex")))]
pub fn handler_art_test(command: Command<'_>, stage: u8) -> Result<CommandResponse<'_>, AppSW> {
    use crate::app_ui::library::{run_event_loop, Screen, ScreenArena, SCREEN_W};
    use crate::state::{Art, ART_W};

    const N: usize = ART_W;
    let mut arena = ScreenArena::new();
    let icon = if stage == 1 {
        arena.icon_static(
            Art::get(crate::state::SLOT_MASTER),
            N as u16,
            N as u16,
            ledger_secure_sdk_sys::NBGL_BPP_1,
        )
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

/// LIBRARY_PREVIEW: a development probe that draws the library as a list of `P1`
/// synthetic rows. The NVM data model holds at most one master and one pressing,
/// so a three-or-more-record library never occurs in production; this renders
/// one through the same touchable-bar list the two-record library uses, so the
/// list layout can be verified (and captured for the film) at three rows and up.
/// Draws nothing that touches or changes NVM.
#[cfg(all(feature = "uiprobe", any(target_os = "stax", target_os = "flex")))]
pub fn handler_library_preview(command: Command<'_>, count: u8) -> Result<CommandResponse<'_>, AppSW> {
    use crate::app_ui::library::{run_event_loop, Layout, ScreenArena, TOKEN_PRESSING, TOKEN_QUIT};

    let n = crate::state::ART_W;
    let half = n / 2;
    let rows = (count as usize).clamp(1, 6);

    let mut arena = ScreenArena::new();
    let mut strings: Vec<CString> = Vec::new();
    let mut cstr = |s: String| -> *const core::ffi::c_char {
        let owned = CString::new(s.replace('\0', " ")).unwrap_or_default();
        strings.push(owned);
        strings[strings.len() - 1].as_ptr()
    };

    let mut layout = Layout::new();
    layout.header(cstr(String::from("Enclave Records")), core::ptr::null());

    // The row-0 (master) title may be overridden by the APDU data, so the
    // capture harness can exercise the real album names and a deliberately long
    // title against the single-line truncation. Empty data keeps the default.
    let override_title = core::str::from_utf8(command.get_data()).ok().filter(|s| !s.is_empty());

    // Short, single-line titles and status lines: a row whose title or status
    // wraps to two lines is tall enough that three no longer clear the footer.
    let titles = ["Nocturne", "Monolith", "Discovery", "Homework", "Eclipse", "Transit"];
    for i in 0..rows {
        let album_id = [((i as u8).wrapping_mul(53)).wrapping_add(7); 32];
        let canonical = crate::sleeve::fallback_sleeve(n, &album_id);
        let thumb = crate::sleeve::to_display(&crate::sleeve::decimate(&canonical, n));
        let icon = arena.icon(thumb, half as u16, half as u16, ledger_secure_sdk_sys::NBGL_BPP_1);
        let sub = if i == 0 {
            format!("Master{}5 of 5 left", ROW_SEP)
        } else {
            format!("#{} of 12", i)
        };
        let raw_title = match (i, override_title) {
            (0, Some(t)) => t,
            _ => titles[i % titles.len()],
        };
        let title_fit = fit_one_line(
            raw_title,
            ledger_secure_sdk_sys::BAGL_FONT_INTER_SEMIBOLD_28px,
            ROW_TITLE_MAX_PX,
        );
        let sub = indent_subtext(&sub, half as u16 + 16);
        layout.touchable_bar(icon, cstr(title_fit), cstr(sub), TOKEN_PRESSING);
    }
    layout.footer(cstr(String::from("Quit")), TOKEN_QUIT);
    layout.draw();
    drop(cstr);
    let _ = run_event_loop();

    let response = command.into_response();
    Ok(response)
}

/// CARD_PREVIEW: a development probe that renders the record card for an
/// arbitrary `number`/`edition` (data = number(2 LE) || edition(2 LE)) over a
/// synthetic cover. A pressing numbered `#1000 of 1000` is unreachable by
/// ceremony (it would take 999 real presses), so this is how the large-number
/// layout is verified and captured. Touches no NVM.
#[cfg(all(feature = "uiprobe", any(target_os = "stax", target_os = "flex")))]
pub fn handler_card_preview(command: Command<'_>) -> Result<CommandResponse<'_>, AppSW> {
    let data = command.get_data();
    if data.len() != 4 {
        return Err(AppSW::WrongApduLength);
    }
    let number = u16::from_le_bytes([data[0], data[1]]);
    let edition = u16::from_le_bytes([data[2], data[3]]);

    let card = CardData {
        title: String::from("Random Access Memories"),
        artist: String::from("Daft Punk"),
        edition,
        // number 0 renders the master (#0); any other renders that pressing.
        number: if number == 0 { None } else { Some(number) },
        edition_id: String::from("A1B2C3D4"),
        device_id: String::from("3FC2A9B1"),
        album_id: [0x42u8; 32],
        sleeve_verified: false,
        slot: crate::state::SLOT_MASTER,
        // A synthetic card is never a transfer, so its Device ID page reads
        // as a copy that came straight from a press.
        received_from: String::new(),
        earlier: 0,
    };
    card_loop(&card)?;

    let response = command.into_response();
    Ok(response)
}
