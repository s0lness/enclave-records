//! Screens drawn from raw NBGL objects.
//!
//! The Rust wrapper only exposes fixed page templates (a centered info block,
//! a tag/value list, a review flow). None of them can put a sleeve grid on
//! screen, or draw a title *on top of* an image, so this module drops to the
//! `ledger_secure_sdk_sys` object API.
//!
//! ## The working recipe
//!
//! Getting a raw object tree to render from an app took bisecting, because
//! every wrong step is a segfault with no diagnostic (`log::debug!` is
//! compiled out in a release build, and a crash produces no APDU reply). The
//! sequence that works, in order:
//!
//! 1. `nbgl_screenPush` for a layer of our own. Reconfiguring the current top
//!    screen with `nbgl_screenSet` instead leaves the wrapper's layout state
//!    pointing at objects we recycled, and the redraw walks into it.
//! 2. Give the pushed screen a geometry. It is created with none, and the
//!    refresh pass computes the region to flush from it.
//! 3. Zero each pooled object. `nbgl_objPoolGet` returns *recycled* memory,
//!    so a stale `onDrawCallback` is a wild function pointer that the draw
//!    will call. Only `type_` and `objId` survive the wipe.
//! 4. Draw with `nbgl_objAllowDrawing` + `nbgl_objDraw` per object.
//!    `nbgl_screenRedraw` crashes here whichever way the screen was created.
//! 5. `nbgl_refresh`, then pop the layer when leaving, or the wrapper's next
//!    `show_and_return` draws onto our orphaned screen and dies.
//!
//! Two further constraints the API imposes:
//!
//! * NBGL keeps *pointers* into everything: the bitmap, the icon descriptor,
//!   every string. They must outlive the draw, hence [`ScreenArena`].
//! * The event loop is ours to run, and it must yield to the host. See
//!   [`run_event_loop`].

use alloc::boxed::Box;
use alloc::ffi::CString;
use alloc::vec::Vec;
use core::ptr;
use ledger_device_sdk::nbgl::nbgl_next_event_ahead;
use ledger_secure_sdk_sys::*;

#[cfg(feature = "artprobe")]
pub const SCREEN_W: i16 = SCREEN_WIDTH as i16;
#[allow(dead_code)]
pub const SCREEN_H: i16 = SCREEN_HEIGHT as i16;

/// Owns every allocation an on-screen object points at. NBGL holds raw
/// pointers into these, so the arena must outlive the screen: build it,
/// draw, run the loop, and only then drop it.
#[derive(Default)]
pub struct ScreenArena {
    /// Only the development probes intern strings through the arena; every
    /// production screen owns its own `Vec<CString>` beside the layout.
    #[cfg(feature = "artprobe")]
    strings: Vec<CString>,
    bitmaps: Vec<Box<[u8]>>,
    icons: Vec<Box<nbgl_icon_details_t>>,
}

impl ScreenArena {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern a string and hand back a pointer NBGL can keep.
    #[cfg(feature = "artprobe")]
    pub fn text(&mut self, s: &str) -> *const core::ffi::c_char {
        // A NUL inside a label would truncate the C string; replace rather
        // than fail, since this is display-only.
        let owned = CString::new(s.replace('\0', " ")).unwrap_or_default();
        self.strings.push(owned);
        self.strings[self.strings.len() - 1].as_ptr()
    }

    /// Intern an icon descriptor over a bitmap computed at runtime.
    pub fn icon(
        &mut self,
        bitmap: Vec<u8>,
        w: u16,
        h: u16,
        bpp: nbgl_bpp_t,
    ) -> *const nbgl_icon_details_t {
        self.bitmaps.push(bitmap.into_boxed_slice());
        let bits = self.bitmaps[self.bitmaps.len() - 1].as_ptr();
        self.icon_at(bits, w, h, bpp)
    }

    /// Intern an icon descriptor over a bitmap that already lives forever
    /// (the sleeve in NVM): no copy, NBGL reads straight out of flash.
    #[cfg(feature = "artprobe")]
    pub fn icon_static(
        &mut self,
        bitmap: &'static [u8],
        w: u16,
        h: u16,
        bpp: nbgl_bpp_t,
    ) -> *const nbgl_icon_details_t {
        self.icon_at(bitmap.as_ptr(), w, h, bpp)
    }

    fn icon_at(
        &mut self,
        bitmap: *const u8,
        w: u16,
        h: u16,
        bpp: nbgl_bpp_t,
    ) -> *const nbgl_icon_details_t {
        self.icons.push(Box::new(nbgl_icon_details_t {
            width: w,
            height: h,
            bpp,
            isFile: false,
            bitmap,
        }));
        &*self.icons[self.icons.len() - 1] as *const nbgl_icon_details_t
    }
}

/// What ended the event loop.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Exit {
    /// An APDU arrived: the screen must get out of the way so the main loop
    /// can serve the host.
    Apdu,
    /// The user touched an object carrying this token.
    Touched(u8),
    SwipedLeft,
    SwipedRight,
}

/// Set by the NBGL touch callback, drained by the event loop.
///
/// Encoded into an integer rather than held as an `Option<Exit>`: this target
/// forbids a `.data` section, so every static must be zero-initialised, and
/// `None` is not all-zero. 0 means "nothing pending".
static TOUCH_RESULT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0);

const TOUCH_NONE: u16 = 0;
const TOUCH_SWIPED_LEFT: u16 = 1;
const TOUCH_SWIPED_RIGHT: u16 = 2;
const TOUCH_TOUCHED_BASE: u16 = 0x100;

fn touch_result_take() -> Option<Exit> {
    match TOUCH_RESULT.swap(TOUCH_NONE, core::sync::atomic::Ordering::Relaxed) {
        TOUCH_NONE => None,
        TOUCH_SWIPED_LEFT => Some(Exit::SwipedLeft),
        TOUCH_SWIPED_RIGHT => Some(Exit::SwipedRight),
        encoded => Some(Exit::Touched(encoded.wrapping_sub(TOUCH_TOUCHED_BASE) as u8)),
    }
}

fn touch_result_set(exit: Exit) {
    let encoded = match exit {
        Exit::SwipedLeft => TOUCH_SWIPED_LEFT,
        Exit::SwipedRight => TOUCH_SWIPED_RIGHT,
        Exit::Touched(token) => TOUCH_TOUCHED_BASE + token as u16,
        Exit::Apdu => return,
    };
    TOUCH_RESULT.store(encoded, core::sync::atomic::Ordering::Relaxed);
}

#[cfg(feature = "artprobe")]
unsafe extern "C" fn touch_callback(obj: *mut core::ffi::c_void, event: nbgl_touchType_t) {
    let result = match event {
        SWIPED_LEFT => Some(Exit::SwipedLeft),
        SWIPED_RIGHT => Some(Exit::SwipedRight),
        TOUCHED if !obj.is_null() => {
            // Every touchable object carries its meaning in `touchId`.
            Some(Exit::Touched(unsafe { (*(obj as *mut nbgl_obj_t)).touchId }))
        }
        _ => None,
    };
    if let Some(exit) = result {
        touch_result_set(exit);
    }
}

/// Static empty label for the background object. A `c""` literal is fine
/// here: it is never dereferenced for content, only for its terminating NUL.
#[cfg(feature = "artprobe")]
static EMPTY_LABEL: &core::ffi::CStr = c"";

/// An all-zero ticker: these screens want no periodic callback.
static NO_TICKER: nbgl_screenTickerConfiguration_t = nbgl_screenTickerConfiguration_t {
    tickerCallback: None,
    tickerValue: 0,
    tickerIntervale: 0,
};

/// A screen of our own, on its own NBGL layer. Dropping it pops the layer.
///
/// The raw object/screen path is kept for the development probes only (see the
/// module notes: it composes but never paints from an app). Production screens
/// all go through [`Layout`], so this is compiled in under `artprobe` alone.
#[cfg(feature = "artprobe")]
pub struct Screen {
    children: *mut *mut nbgl_obj_t,
    capacity: u8,
    layer: u8,
    objects: Vec<*mut nbgl_obj_t>,
}

#[cfg(feature = "artprobe")]
impl Screen {
    /// Push a fresh layer with room for `capacity` children. `swipeable` asks
    /// the screen itself to report swipes, which is how NBGL delivers a swipe
    /// that does not land on a particular object.
    pub fn push(capacity: u8, swipeable: bool) -> Screen {
        touch_result_take();
        unsafe {
            let mut children: *mut *mut nbgl_obj_t = ptr::null_mut();
            let layer = nbgl_screenPush(&mut children, capacity, &NO_TICKER, Some(touch_callback));
            let top = nbgl_screenGetTop();
            if !top.is_null() {
                (*top).area.x0 = 0;
                (*top).area.y0 = 0;
                (*top).area.width = SCREEN_WIDTH as u16;
                (*top).area.height = SCREEN_HEIGHT as u16;
                (*top).area.backgroundColor = WHITE;
                if swipeable {
                    (*top).touchMask = (1 << SWIPED_LEFT) | (1 << SWIPED_RIGHT);
                }
            }
            let mut screen = Screen {
                children,
                capacity: capacity + 1,
                layer: layer.max(0) as u8,
                objects: Vec::new(),
            };
            // A pushed layer does not erase what was underneath, and drawing
            // the screen object itself crashes. So the first child is an
            // empty full-screen text area, drawn only for its background:
            // without it the previous screen shows through the gaps.
            screen.text(
                EMPTY_LABEL.as_ptr(),
                0,
                0,
                SCREEN_WIDTH as u16,
                SCREEN_HEIGHT as u16,
                BAGL_FONT_INTER_REGULAR_24px,
                WHITE,
                CENTER,
                None,
            );
            screen
        }
    }

    /// Take an object from the pool and scrub the recycled contents. Returns
    /// null when the screen is full or the pool is exhausted, and every
    /// caller must tolerate that: a missing object is a blank area, never a
    /// crash.
    fn alloc(&mut self, obj_type: nbgl_obj_type_t, size: usize) -> *mut nbgl_obj_t {
        if self.objects.len() >= self.capacity as usize {
            return ptr::null_mut();
        }
        unsafe {
            let obj = nbgl_objPoolGet(obj_type, self.layer);
            if obj.is_null() {
                return obj;
            }
            let saved_type = (*obj).type_;
            let saved_id = (*obj).objId;
            ptr::write_bytes(obj as *mut u8, 0, size);
            (*obj).type_ = saved_type;
            (*obj).objId = saved_id;
            (*obj).area.backgroundColor = WHITE;
            (*obj).alignment = NO_ALIGNMENT;
            // The draw resolves an object's absolute position through its
            // parent; left null it computes nothing and paints nothing.
            (*obj).parent = nbgl_screenGetTop();
            *self.children.add(self.objects.len()) = obj;
            self.objects.push(obj);
            obj
        }
    }

    /// Place an image at an absolute position. `touch_token`, when given,
    /// comes back from the event loop as [`Exit::Touched`].
    pub fn image(
        &mut self,
        icon: *const nbgl_icon_details_t,
        w: u16,
        h: u16,
        x: i16,
        y: i16,
        touch_token: Option<u8>,
    ) {
        let obj = self.alloc(IMAGE, core::mem::size_of::<nbgl_image_t>());
        if obj.is_null() {
            return;
        }
        unsafe {
            let img = obj as *mut nbgl_image_t;
            (*img).buffer = icon;
            (*img).foregroundColor = BLACK;
            (*img).obj.area.width = w;
            (*img).obj.area.height = h;
            (*img).obj.area.x0 = x;
            (*img).obj.area.y0 = y;
            if let Some(token) = touch_token {
                (*img).obj.touchMask = 1 << TOUCHED;
                (*img).obj.touchId = token;
            }
        }
    }

    /// Place a text area at an absolute position. `text` must come from the
    /// [`ScreenArena`] that outlives this screen.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        text: *const core::ffi::c_char,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        font: nbgl_font_id_e,
        color: color_t,
        align: nbgl_aligment_t,
        touch_token: Option<u8>,
    ) {
        let obj = self.alloc(TEXT_AREA, core::mem::size_of::<nbgl_text_area_t>());
        if obj.is_null() {
            return;
        }
        unsafe {
            let ta = obj as *mut nbgl_text_area_t;
            (*ta).text = text;
            (*ta).textColor = color;
            (*ta).textAlignment = align;
            (*ta).fontId = font;
            (*ta).style = NO_STYLE;
            (*ta).nbMaxLines = 1;
            (*ta).onDrawCallback = None;
            (*ta).obj.area.width = w;
            (*ta).obj.area.height = h;
            (*ta).obj.area.x0 = x;
            (*ta).obj.area.y0 = y;
            if let Some(token) = touch_token {
                (*ta).obj.touchMask = 1 << TOUCHED;
                (*ta).obj.touchId = token;
            }
        }
    }

    /// Paint every object placed so far.
    pub fn draw(&self) {
        unsafe {
            nbgl_objAllowDrawing(true);
            for obj in &self.objects {
                nbgl_objDraw(*obj);
            }
            nbgl_refresh();
        }
    }
}

#[cfg(feature = "artprobe")]
impl Drop for Screen {
    fn drop(&mut self) {
        unsafe {
            nbgl_screenPop(self.layer);
        }
    }
}

/// Callback for layout-built screens: NBGL reports the token of whatever was
/// touched, which is exactly the vocabulary [`Exit::Touched`] wants.
unsafe extern "C" fn layout_touch_callback(token: core::ffi::c_int, _index: u8) {
    touch_result_set(Exit::Touched(token as u8));
}

/// Row tokens for the library: one per held record, small and dense so they
/// never collide with the control tokens below.
pub const TOKEN_MASTER: u8 = 0;
pub const TOKEN_PRESSING: u8 = 1;
/// Back-envelope (i) rows, one per fact, each opening its own sub-page. Kept
/// clear of both the row range below and the control range above.
pub const TOKEN_INFO_NUMBER: u8 = 240;
pub const TOKEN_INFO_EDITION: u8 = 241;
pub const TOKEN_INFO_DEVICE: u8 = 242;
pub const TOKEN_LEARN_MORE: u8 = 243;
/// The Device ID page's right-hand footer action, opening the history page. It
/// is a footer and not a fifth row for a reason: the Device ID page's list area
/// is already full at two pairs, and the footer's bar exists whatever is on it.
pub const TOKEN_INFO_HISTORY: u8 = 244;
/// Control tokens, kept well clear of the row range.
pub const TOKEN_INFO: u8 = 250;
pub const TOKEN_QUIT: u8 = 251;
/// Record-card controls: "Back" (footer left) and the pager (footer right).
pub const TOKEN_BACK: u8 = 252;
pub const TOKEN_PAGER: u8 = 253;

/// The record card's page pager as a generated label, e.g. "< 1 of 2 >". The
/// card is a two-page front/back, so the total is fixed at 2; the current page
/// index is the only variable. No literal pager strings live in the pages.
pub const CARD_PAGES: u8 = 2;
pub fn pager_label(page: u8) -> alloc::string::String {
    alloc::format!("< {} of {} >", page, CARD_PAGES)
}

/// A screen built through the app-side `nbgl_layout` API.
///
/// This is the layer that actually works from an app. The object/screen API
/// above it (`nbgl_objPoolGet`, `nbgl_screenPush`) links and can be driven up
/// to a point, but its draw never paints: the object linkage the OS-side
/// draw walks cannot be constructed safely from here, and every attempt to
/// complete it crashes. `nbgl_layout` is compiled *into* the app, does that
/// plumbing itself, and is the supported way to compose a custom screen.
pub struct Layout {
    handle: *mut nbgl_layout_t,
}

impl Layout {
    pub fn new() -> Layout {
        let description = nbgl_layoutDescription_t {
            modal: false,
            withLeftBorder: false,
            tapActionText: ptr::null(),
            tapActionToken: 0,
            tapTuneId: 0,
            onActionCallback: Some(layout_touch_callback),
            ticker: NO_TICKER,
        };
        touch_result_take();
        Layout {
            handle: unsafe { nbgl_layoutGet(&description) },
        }
    }

    /// An icon with up to three lines of text under it.
    pub fn centered_info(
        &mut self,
        icon: *const nbgl_icon_details_t,
        text1: *const core::ffi::c_char,
        text2: *const core::ffi::c_char,
        text3: *const core::ffi::c_char,
        offset_y: i16,
    ) {
        let info = nbgl_layoutCenteredInfo_t {
            text1,
            text2,
            text3,
            icon,
            onTop: false,
            style: LARGE_CASE_BOLD_INFO,
            offsetY: offset_y,
        };
        unsafe {
            nbgl_layoutAddCenteredInfo(self.handle, &info);
        }
    }

    /// A title bar with an optional (i) affordance in the top-right corner.
    /// The icon must be caller-provided (from a [`ScreenArena`]): the OS's own
    /// info glyph is declared by the bindings but is not linkable from an app.
    /// The info button reports [`TOKEN_INFO`] when tapped.
    pub fn header(&mut self, title: *const core::ffi::c_char, info_icon: *const nbgl_icon_details_t) {
        let header = nbgl_layoutHeader_t {
            type_: HEADER_TITLE,
            separationLine: true,
            __bindgen_anon_1: nbgl_layoutHeader_t__bindgen_ty_1 {
                title: nbgl_layoutHeader_t__bindgen_ty_1__bindgen_ty_4 { text: title },
            },
        };
        unsafe {
            nbgl_layoutAddHeader(self.handle, &header);
            if !info_icon.is_null() {
                nbgl_layoutAddTopRightButton(self.handle, info_icon, TOKEN_INFO, 0);
            }
        }
    }

    /// A full-width tappable row: an icon on the left, a title, a status line
    /// under it, and a chevron on the right. Reports `token` when tapped.
    pub fn touchable_bar(
        &mut self,
        icon: *const nbgl_icon_details_t,
        text: *const core::ffi::c_char,
        sub_text: *const core::ffi::c_char,
        token: u8,
    ) {
        let bar = nbgl_layoutBar_t {
            iconLeft: icon,
            text,
            iconRight: ptr::null(),
            subText: sub_text,
            large: false,
            token,
            inactive: false,
            centered: false,
            tuneId: 0,
        };
        unsafe {
            nbgl_layoutAddTouchableBar(self.handle, &bar);
        }
    }

    /// A full-width tappable row carrying only a label and a right-hand chevron
    /// (no left icon): the back-of-record "settings row". The chevron is the tap
    /// affordance; NBGL right-aligns it and left-aligns the label, so a label
    /// that embeds its value ("Edition ID   E2547431") reads as label-then-value
    /// with the chevron at the end. Reports `token` when tapped.
    pub fn nav_row(
        &mut self,
        text: *const core::ffi::c_char,
        chevron: *const nbgl_icon_details_t,
        token: u8,
    ) {
        let bar = nbgl_layoutBar_t {
            iconLeft: ptr::null(),
            text,
            iconRight: chevron,
            subText: ptr::null(),
            large: false,
            token,
            inactive: false,
            centered: false,
            tuneId: 0,
        };
        unsafe {
            nbgl_layoutAddTouchableBar(self.handle, &bar);
        }
    }

    /// A block of centered text with an optional smaller second line. Used for
    /// the empty state.
    pub fn text(&mut self, text: *const core::ffi::c_char, sub_text: *const core::ffi::c_char) {
        unsafe {
            nbgl_layoutAddText(self.handle, text, sub_text);
        }
    }

    /// A full-width action bar pinned to the bottom, reporting `token`.
    pub fn footer(&mut self, text: *const core::ffi::c_char, token: u8) {
        unsafe {
            nbgl_layoutAddFooter(self.handle, text, token, 0);
        }
    }

    /// A bottom bar split in two: a left action (e.g. "Back") and a right one
    /// (the record card's "< N of 2 >" pager), each reporting its own token.
    pub fn split_footer(
        &mut self,
        left: *const core::ffi::c_char,
        left_token: u8,
        right: *const core::ffi::c_char,
        right_token: u8,
    ) {
        unsafe {
            nbgl_layoutAddSplitFooter(self.handle, left, left_token, right, right_token, 0);
        }
    }

    /// A tag/value list (small grey tag over its value), the back-of-record
    /// layout. The pairs and their strings must outlive the screen.
    pub fn tag_value_list(&mut self, pairs: &[nbgl_contentTagValue_t]) {
        let list = nbgl_layoutTagValueList_t {
            pairs: pairs.as_ptr(),
            callback: None,
            nbPairs: pairs.len() as u8,
            startIndex: 0,
            hideEndOfLastLine: false,
            nbMaxLinesForValue: 0,
            token: 0,
            smallCaseForValue: true,
            wrapping: true,
            actionCallback: None,
        };
        unsafe {
            nbgl_layoutAddTagValueList(self.handle, &list);
        }
    }

    pub fn draw(&self) {
        unsafe {
            nbgl_layoutDraw(self.handle);
            nbgl_refresh();
        }
    }
}

impl Drop for Layout {
    fn drop(&mut self) {
        unsafe {
            nbgl_layoutRelease(self.handle);
        }
    }
}

/// Run a screen's event loop until the user acts or the host speaks.
///
/// The APDU check is the non-negotiable half. The library is the app's idle
/// screen, so it is what is on display when a ceremony starts; if it only
/// woke on touch, every cut / pair / press would deadlock against a device
/// waiting for a finger. `nbgl_next_event_ahead` reports as soon as an APDU
/// is pending, and control goes straight back to the main loop, which serves
/// the command and redraws.
pub fn run_event_loop() -> Exit {
    loop {
        if nbgl_next_event_ahead() {
            return Exit::Apdu;
        }
        if let Some(exit) = touch_result_take() {
            return exit;
        }
    }
}
