# The reader

The architecture of the book inside the app: where the text lives, how a page is
found, what persists, how it meets the ceremony code, and what an engineer has to
know before changing any of it. The case for the idea is in
[`docs/ledger-as-reader.md`](ledger-as-reader.md).

**No reader code exists yet.** What exists is a measured foundation: the flash
ceiling, the size model, and a working read path out of `.text`, all established
against the current build. Every number below is labelled **measured** (with the
file it came off), **derived** (arithmetic over SDK constants), or **inferred**
(an estimate, with the measurement that would settle it). This app already spent
weeks designing around a size ceiling that turned out to be an emulator defect
(section 8.3). Plausible-looking arithmetic is how that detour started, so the
labels are load-bearing.

## 0. The architecture

### 0.1 The shape of it

**One work, one app.** `The Great Gatsby` is compiled into the binary as a plain
byte array in `.text`, in the clear. There is no book selector, no library of
works, no download path. A second book is a second app with a different `static`,
and the two coexist because BOLOS gives each app its own flash region.

**The text is free, the copy is scarce.** Installing the app gives you the whole
book. Owning numbered copy 4 of 50 is a separate fact carried by the certificate
and the bearer key, exactly as today. Nothing about reading is gated on holding a
copy. No encryption, no DRM: a reader that decrypts is a reader that can fail to
decrypt, and the text is public domain anyway.

**Pagination is an OS call, made at runtime.** The device asks
`nbgl_getTextMaxLenInNbLines` how many bytes fit in nine lines at 416 px, and
advances a byte cursor by that much. No page table is compiled in. The function
that decides the breaks and the function that draws them are the same OS code
reading the same bytes, so the two cannot disagree.

**The position is a byte offset.** `ReaderNvm.pos` is an offset into the book.
Page numbers are derived for display and recomputed whenever the font, the OS
fonts, or the line budget change.

**The reader is a screen, and screens in this app are cheap.** It is built from
the same `Layout` wrapper as the library and the record card, drops itself
whenever an APDU arrives, and rebuilds from scratch on every state change.

### 0.2 What it inherits, what it adds, what it must not touch

The reader is a new screen bolted onto machinery that works. It reuses:

| existing | file | used for |
|---|---|---|
| `Layout` / `Drop` -> `nbgl_layoutRelease` | `app_ui/library.rs` | the whole screen lifecycle |
| `Layout::text` -> `nbgl_layoutAddText` | `app_ui/library.rs` | the page body |
| `Layout::split_footer` | `app_ui/library.rs` | the `<` / `>` page turns |
| `ScreenArena` | `app_ui/library.rs` | C string and icon lifetimes |
| `run_event_loop` | `app_ui/library.rs` | touch and APDU, one pump |
| `warrants_library_redraw` | `main.rs` | the pattern its mirror follows |
| an own `.nvm_data` object | `state.rs`, `ART_MASTER` / `ART_PRESSING` | reading state outside `PresseNvm` |
| `assert_page_fits` | `tests/presse_client.py` | the overrun check, per turn |

It adds exactly four things to the existing UI layer:

1. a `Layout::header_back` variant using `HEADER_EXTENDED_BACK`. The current
   `Layout::header` is hardcoded to `HEADER_TITLE` (`app_ui/library.rs`), whose
   height follows its text. The reader needs the fixed 96 px form with a back key
   and a right-hand key (section 3.1);
2. a `Layout::title_text` wrapper over `nbgl_layoutAddTextContent`, the only
   route to the large face (section 3.4);
3. `ReaderNvm`, its own `.nvm_data` object (section 5);
4. `evicts_reader(ins)` in `main.rs`, the mirror of `warrants_library_redraw`
   (section 8.10).

The ceremony machinery is not redesigned. Section 7 adds one instruction pair to
the phase that is free to abandon and touches nothing else.

### 0.3 The invariants

Six things hold this design together. Breaking any of them produces a bug that
does not announce itself.

1. **The book lives in `.text`, and only in `.text`.** `data_size` stays at
   18432. It is the one number that reports an accidental `.rodata` or
   `.nvm_data` growth, and a 280 KB book folded into it would drown that signal
   forever (sections 1.1, 8.1).
2. **A page's line budget is an input.** It is handed to
   `nbgl_getTextMaxLenInNbLines` as `maxNbLines`. A page one line too tall is
   drawn under the footer with no error anywhere (sections 3.2, 8.2).
3. **The stored position is a byte offset.** Page numbers are derived. An offset
   survives a font change, an OS font change, and a transfer to another device
   (sections 2.4, 5.2).
4. **The reader never calls `Store::get()`.** That function writes NVM on a
   virgin device (`state.rs`, `initialized == 0`) and copies 1176 bytes of
   `PresseNvm`, including the bearer scalar, onto the stack. A thousand page
   turns must not put a bearer key on the stack a thousand times, past the
   `crypto::scrub` call sites that exist to keep it off there (sections 5.1,
   8.11).
5. **The give ceremony's phase 2 is untouched.** `GIVE_OFFER`'s sealed reply
   stays 64 bytes, the three-state `committed` byte keeps its meaning, and the
   chain fold at `TAKE_HANDOVER` is not extended. The bookmark rides in phase 1,
   which is free to abandon (section 7).
6. **The page-turn handler fails closed at both ends.** No wrap, no silent clamp
   (section 8.9).

## 1. Ground truth

Everything in this section was read off the SDK this app builds against, or off
the current build. Sources are named so a future reader can re-derive them.

### 1.1 The link, and why the book goes in `.text`

`ledger_secure_sdk_sys-1.16.2/devices/flex/flex_layout.ld`, complete (measured):

```
MEMORY
{
  FLASH   (rx)  : ORIGIN = 0xc0de0000, LENGTH = 400K
  DATA    (r)   : ORIGIN = 0xc0de0000, LENGTH = 400K
  SRAM    (rwx) : ORIGIN = 0xda7a0000, LENGTH = 36K
}

PAGE_SIZE      = 512;
STACK_MIN_SIZE = 1500;
END_STACK      = ORIGIN(SRAM) + LENGTH(SRAM);
```

400 KiB total app image, 512-byte flash pages, 36 KiB of SRAM with the stack
growing down from its top. `FLASH` and `DATA` overlap at the same origin, so
400 KiB is the whole budget.

The section order in the same crate's `link.ld` decides the placement (measured):

```
.text        -> _text = _nvram_start
.rel_flash   -> ends at _nvram_data
.rodata      -> after _nvram_data
.data        -> empty, ASSERTed empty
.nvm_data    -> ends at _envram_data
```

`cargo-ledger-1.14.0/src/utils.rs:105` computes what the loader is told
(measured):

```rust
infos.data_size = envram_data - nvram_data;
```

So **`data_size` = `.rodata` + `.nvm_data`**. A plain `static` in Rust lands in
`.rodata`, so a 280 KB book declared the obvious way adds 280 KB to a figure that
reads 18432 today.

That figure has no ceiling of its own. Both halves are carved out of the same
400 KiB, so a book costs the same flash whichever section holds it. What
`data_size` buys is a signal: kept small and stable, any movement in it means
something landed in `.rodata` or `.nvm_data` that was not meant to. That is the
cheapest regression check available on a build about to grow by a factor of four,
and a 298 KB `data_size` would destroy it.

**The book goes in `.text`, via `#[link_section = ".text"]`.** `data_size` was
measured unmoved at 18432 across 256 KiB of payload (section 1.3), which is the
proof that the placement works.

### 1.2 The current build

Measured off `device-app/target/flex/release/presse` on 2026-07-31, ELF section
headers and symbol table:

| section | addr | size |
|---|---|---|
| `.text` | `0xc0de0000` | 61440 |
| `.rel_flash` | `0xc0def000` | 2352 |
| `.rodata` | `0xc0defa00` | 11264 |
| `.nvm_data` | `0xc0df2600` | 7582 |
| `.bss` | `0xda7a0000` | 36864 |

`arm-none-eabi-size` reports `text` = 75056 (`.text` + `.rel_flash` +
`.rodata`), and the symbols give `data_size` = `_envram_data - _nvram_data` =
`0xc0df4200 - 0xc0defa00` = **18432**.

Two derived figures matter more than either of those.

**The image is `_text` to `_nvram_end`**, `0xc0de0000` to `0xc0df439e`, **82846
bytes, 20.2% of the 400 KiB region.**

**The load size is `_erodata - _text` = 75264**, the `p_filesz` Speculos sizes
its mapping from. It runs 208 bytes above `text`, because the linker pads
`.rel_flash` from 2352 up to 2560, and that padding moves with the relocation
count. 75264 mod 4096 = 1536, so this build clears the emulator notch of section
8.3.

These figures move by a few hundred bytes on any commit that changes code
(AGENTS.md records 83870 for the image, this document recorded 82334, and both
were correct when written). `.nvm_data` at 7582 and `data_size` at 18432 are the
two that should stay put, and a change in either is news. Re-derive the rest with
`presse_load_size` (`scripts/env.sh`) and `arm-none-eabi-readelf -SW`.

### 1.3 The ceiling, and the exact size model

A `[u8; N]` in `#[link_section = ".text"]` was grown by bisection against the
build above (measured). **The largest payload that links is 326986 bytes.** At
326987 the link fails with

```
.nvm_data will not fit in region FLASH
```

The binding constraint is the NVM section pushed past the end of the region.
**98 bytes of slack remain at that ceiling** (409600 minus 401920 of load size
minus 7582 of `.nvm_data`).

The sizes follow an exact model, zero error at eight sampled points (measured):

```
load_size = 512 * ceil((payload + 74934) / 512)
text      = load_size - 208
the link fails once load_size > 401920
```

74934 is the current build's own contribution and moves as the app changes. 512
is the flash page size. 401920 is 409600 minus the 7582-byte `.nvm_data` rounded
up to a page. The model reproduces today's zero-payload build exactly:
`512 * ceil(74934 / 512)` = 75264, the measured load size.

`data_size` was measured at **18432 across 256 KiB of payload**, unmoved. That is
the placement invariant of section 0.3 holding under load.

Three consequences an engineer should carry.

**A maximum-size book freezes the app.** At the ceiling there are 98 bytes of
slack. Every later feature comes out of the text.

**One payload size in eight is unbuildable on a stock emulator.** `load_size`
moves in 512-byte steps, and the Speculos notch fires when it is a multiple of
4096 (section 8.3). `presse_check_load_size` in `scripts/env.sh` refuses those
builds, `ALLOW_NVM_NOTCH=1` overrides. **The 512-byte band immediately below the
ceiling is one of them**: 401920 mod 4096 = 512, so the top band is clear, and
the band under it lands on 401408 = 98 * 4096 and is not. A book sized to the last
byte of the region will build; a book 512 bytes smaller will be refused.

**Nothing here was measured against the OS.** The 400 KiB is a link-time region.
Whether BOLOS would accept an image larger than it is untested, and irrelevant
while the linker refuses first.

### 1.4 The read path out of `.text`

This was the design's largest open risk and it is closed at every level the tree
can reach.

A `static` in `#[link_section = ".text"]` **reads back correctly** at 64 KiB, at
256 KiB and at the 326986-byte ceiling; at fourteen offsets including the final
sixteen bytes; through two independent access paths; byte-identical to the ELF
contents and present verbatim in `presse.hex` (measured). The references the
compiler emits into it are `R_ARM_REL32`, resolved at link time, so the
`ropi-rwpi` relocation model this target uses (`devices/flex/flex.json`,
`"relocation-model": "ropi-rwpi"`) does not break them.

**No physical device has executed a single byte of it.** The hex proves the bytes
are written to the device's flash at the right addresses. Hardware behaviour is
inferred from that, and the settling measurement is a probe APDU on a real Flex.

The residual worry was that `.text` is declared as needing no relocations while
the SDK's `pic()` relocates pointers over `_nvram_start.._nvram_end`, which covers
`.text`, and that the app already has one documented case of a runtime pointer
faulting under PIC (`build.rs`, on heap icons, which is why glyphs are compiled in
with `include_gif!`). A byte array holds no pointers, which is why it is the
easier case, and the measurement agrees.

### 1.5 The fonts

Flex compiles in exactly three faces, each in a 4bpp and a 1bpp variant
(measured, `ledger_secure_sdk_sys-1.16.2/devices/flex/c_sdk_build_flex.defines`
lines 24-26):

```
#define HAVE_BAGL_FONT_INTER_REGULAR_28PX
#define HAVE_BAGL_FONT_INTER_SEMIBOLD_28PX
#define HAVE_BAGL_FONT_INTER_MEDIUM_36PX
```

Metrics decoded from the shipped
`tests/screenshots/shared_libs/libnbgl_shared_screenshots_flex.a`, object
`nbgl_fonts.c.obj`, section `._nbgl_fonts_` (measured, re-derived 2026-07-31):

| font | id | height | line_height | kerning | first..last char |
|---|---|---|---|---|---|
| `INTER_REGULAR_28px` | 11 | 36 | **36** | 0 | 0x20..0x7E |
| `INTER_SEMIBOLD_28px` | 12 | 36 | **36** | 0 | 0x20..0x7E |
| `INTER_MEDIUM_36px` | 13 | 44 | **40** | 0 | 0x20..0x7E |

The names understate the line height by a fifth to a third. A budget computed
from "28" or "36" is wrong.

Advance widths from the same decode: in Inter Regular 28px, space 8, `i` 7,
`n` 16, `m` 24, `M` 25, `W` 27; in Inter Medium 36px, 10 / 9 / 22 / 32 / 32 / 35.
`char_kerning` is 0 on all six faces, so a string's width is the plain sum of its
characters. The unweighted mean over all 95 glyphs is 15.58 px for Inter Regular
28px, which is where the SDK's hardcoded 16 comes from.

`first_char = 0x20, last_char = 0x7E` on every Flex font. **Printable ASCII and
nothing else.** No accented characters, no em dash, no curly quotes, no ellipsis
character. Three independent facts close the unicode escape hatch:
`HAVE_UNICODE_SUPPORT` appears nowhere in the SDK; the real gate
`HAVE_LANGUAGE_PACK` is absent from the Flex defines; and of
`nbgl_getUnicodeFont`, `nbgl_getUnicodeFontCharacter`,
`nbgl_getUnicodeFontCharacterByteCount` and `nbgl_popUnicodeChar` not one has a
trampoline in `nbgl_stubs.h`, so an app calling them does not link. There is no
Inter unicode font anywhere in the tree. Section 4.2 transliterates.

### 1.6 The geometry

`nbgl_types.h`, Flex block: `SCREEN_WIDTH 480`, `SCREEN_HEIGHT 600`,
`SMALL_ICON_SIZE 40`. `nbgl_obj.h:81`: `BORDER_MARGIN 32`. `nbgl_layout.h:134`
(all measured):

```c
#define AVAILABLE_WIDTH (SCREEN_WIDTH - 2 * BORDER_MARGIN)
```

**416 px.** This is the width to pass to every measuring call.

`nbgl_layout.h`, Flex block: `TOUCHABLE_HEADER_BAR_HEIGHT 96`,
`SIMPLE_FOOTER_HEIGHT 96`, `LIST_ITEM_MIN_TEXT_HEIGHT` = `SMALL_ICON_SIZE` = 40,
`LIST_ITEM_PRE_HEADING 26`. `nbgl_layout.c`, Flex block: `SUB_HEADER_MARGIN 28`,
`PRE_TITLE_MARGIN 16`.

A header plus a footer leaves 600 - 96 - 96 = **408 px** of body. AGENTS.md
already records that number and `tests/presse_client.py` already asserts against
it (`FOOTER_RULE_Y = 504`, line 122).

### 1.7 The measuring API

All of these are OS-side and reached through a trampoline. The authoritative list
of what links is `nbgl_stubs.h`; being declared in the generated `bindings.rs`
proves nothing. The ones that matter here all have trampolines (measured):

```c
uint8_t  nbgl_getFontLineHeight(nbgl_font_id_e fontId);                          // 0xae
uint16_t nbgl_getSingleLineTextWidth(nbgl_font_id_e fontId, const char *text);   // 0xaf
uint16_t nbgl_getTextHeightInWidth(fontId, text, maxWidth, wrapping);            // 0xb2
uint16_t nbgl_getTextNbLinesInWidth(fontId, text, maxWidth, wrapping);           // 0xb4
uint8_t  nbgl_getTextNbPagesInWidth(fontId, text, nbLinesPerPage, maxWidth);     // 0xb5
bool     nbgl_getTextMaxLenInNbLines(fontId, text, maxWidth, maxNbLines,
                                     uint16_t *len, bool wrapping);              // 0xb7
```

`nbgl_getTextMaxLenInNbLines` is the pagination primitive: given a font, a
pointer into the text, a pixel width and a line budget, it writes into `*len` the
number of bytes that fit. Advance the pointer by `len` and you have the next
page. Nothing else is needed to break a book into pages, and its `maxNbLines`
parameter is what makes invariant 2 structural.

`nbgl_getTextNbLines`, `nbgl_getTextLength` and
`nbgl_getTextMaxLenAndWidthFromEnd` are in the header and in the bindings and have
**no trampoline**. Building on them costs a link error at best.

Wrapping is documented and its source is unavailable. `nbgl_fonts.c` is not in
the tree (the font and draw layer moved into the OS at API level 26, and the SDK
clone is shallow), though its compiled object ships inside
`libnbgl_shared_screenshots_flex.a` for host screenshot builds. The break
semantics are known only from `nbgl_obj.h:431`:

```c
uint8_t wrapping : 1;  ///< if set to true, break lines on ' ' when possible
```

Word-preserving when true, mid-word at maximum fitting length when false. No
hyphenation anywhere: the string "hyphen" does not occur in `lib_nbgl`.
Truncation is a literal `"..."`. `'\n'` is very probably a hard break (the SDK
ships multi-line literals and `nbgl_getTextNbLines` takes no font or width), and
the code proving it is not readable here. Step 1 of section 9 measures all of it
on real bytes.

### 1.8 What the app already does

Every production screen is built from raw `nbgl_layout` through one wrapper,
`src/app_ui/library.rs`. The object API (`nbgl_objPoolGet`, `nbgl_screenPush`) is
compiled out of shipping builds and lives behind the `artprobe` feature. Screens
never update in place: a state change drops the whole `Layout` (its `Drop` calls
`nbgl_layoutRelease`) and builds a new one. Touch arrives as a token through a
single `onActionCallback` into an `AtomicU16`, read and cleared by
`touch_result_take()`. The ticker is all-zero everywhere.

`run_event_loop` (`app_ui/library.rs:601-610`) is the whole concurrency model:

```rust
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
```

`nbgl_next_event_ahead()` blocks on an SE event; a finger event is pumped through
NBGL's hit testing as a side effect and fires the token callback, and an APDU is
stashed for replay. One pump, two exits.

There is exactly one call to `nbgl_refresh()` in production code
(`app_ui/library.rs:580`, inside `Layout::draw`) and **zero** calls to
`nbgl_refreshSpecial*` anywhere in the tree. The app has never selected a refresh
mode.

`nbgl_layoutAddSwipe` is called nowhere. `Exit::SwipedLeft` / `SwipedRight` exist
and are produced only on the artprobe path.

Every `Store::put` today is APDU-driven. No gesture has ever written flash, with
one exception worth knowing: `Store::get()` lazily generates the device keypair
and writes NVM on first call, and `Library::draw()` calls it
(`handlers/collection.rs`), so the first paint on a virgin device is a flash
write.

`PresseNvm` is 1176 bytes and is copied onto the stack by value on every screen
draw.

## 2. The text in flash

### 2.1 Placement

```rust
/// The book, ASCII, NUL-terminated, one contiguous blob.
///
/// `.text`, so that `data_size` (`_envram_data - _nvram_data`, 18432 today)
/// stays a legible number: `.rodata` sits inside that window, so a `static`
/// declared the obvious way would fold the whole book into it and any later
/// movement in the figure would say nothing.
#[used]
#[link_section = ".text"]
pub static BOOK: [u8; BOOK_LEN] = *include_bytes!(concat!(env!("OUT_DIR"), "/book.bin"));
```

`include_bytes!` occurs nowhere in the repo today. The existing precedent for
baked-in binary data is `include_gif!` for glyphs, which exists because "a runtime
heap icon faults under PIC relocation on this target" (`build.rs`). A byte array
holds no pointers and needs no relocation of its contents, and section 1.4
measured that it reads back.

The blob ends in a single `NUL`. That NUL is what makes
`nbgl_getTextMaxLenInNbLines` stop at the end of the book and stay inside the
array. It is the only NUL in the file.

`BOOK_LEN` has a hard ceiling of **326986** (section 1.3), and a 512-byte band
just under it that the build guard refuses.

### 2.2 Format: raw ASCII, and what packing would buy

The book ships as plain 7-bit ASCII with `\n` between paragraphs and no other
control character. **No packing in the first build.**

The Great Gatsby is about 47,000 words. English averages roughly 4.7 letters per
word plus a separator, so about **280 KB** raw (inferred). Against the measured
326986-byte ceiling that leaves roughly 47,000 bytes, **about 14 percent margin**
(inferred). The exact figure falls out of the first run of the pipeline and
belongs in this document when it does.

Packing is deferred behind one seam. Design the read path as

```rust
fn page_bytes(from: u32, max_len: usize, out: &mut [u8]) -> usize;
```

and any decoder becomes a one-file change, invisible to everything above it. The
options, in the order they would be reached (all figures inferred):

| scheme | Gatsby | decoder | cost |
|---|---|---|---|
| raw ASCII | 280 KB | none | the reader hands NBGL a pointer into flash |
| 5-bit, Z-machine style | ~190 KB | 100 to 150 bytes | every page goes through an unproven decoder |
| word dictionary, 13-bit tokens | ~120 KB (76 KB stream, 45 KB dictionary) | a lookup and a copy | a build-time vocabulary pass, escape tokens for case and punctuation |

The word dictionary is the right second move and beats 5-bit packing outright.
Neither is worth taking in the first build, because a packed blob means the
reader cannot hand NBGL a pointer into flash, and the raw estimate already has
the room.

**General-purpose compression does not fit, and the reason is RAM, absolutely.**
Deflate needs a 32 KiB sliding window. The whole SRAM region is 36 KiB, `.bss`
already consumes all 36864 bytes of it including the 8 KiB heap and the stack,
and the practical working budget is about 24 KB (measured, section 8.4).
Deflate's window alone exceeds it, and LZ4 needs a block or ring buffer of
comparable size. Heatshrink is the only member of the family that fits (1 to 2
KiB window, roughly 1 KB of code) and at that window gets perhaps 30 to 40
percent on English, which a word dictionary beats while decoding faster and
allowing random access. There is no `no_std` decompressor in the dependency tree;
`flate2` and `miniz_oxide` are in `Cargo.lock` and reach it only through `png`
and the `include_gif` proc macro, both host-side.

### 2.3 Finding a page: runtime pagination with cached anchors

**The device paginates at runtime and caches anchors in NVM.** It walks the book
once with `nbgl_getTextMaxLenInNbLines`, recording a byte offset every `stride`
pages into a 32-entry NVM array. Reaching page N is a binary search over the
anchors followed by at most `stride - 1` forward calls. Forward paging is one
call. Backward paging is a walk from the previous anchor.

Cost in flash: zero. Cost in NVM: 32 anchors of 4 bytes plus a stride and a
fingerprint, 144 bytes, one flash page (section 5.2). Cost in time: one walk of
roughly 1000 calls at first open, and up to `stride - 1` calls on a jump or a
resume.

**Drift is impossible by construction.** The thing that computes the breaks and
the thing that renders them are the same OS function reading the same bytes. An OS
font change is handled by folding a fingerprint of the metrics
(`nbgl_getFontLineHeight`, and the measured width of a probe string) into the
stored value and re-walking when it moves.

The rejected alternative was a **build-time index**: a host program breaks the
book and emits `[u32; ~1000]`, about 4 KB, O(1) random access, free total page
count. Two things sank it. The host program must reproduce NBGL's line-breaking
exactly, and `nbgl_fonts.c` lives in the OS, so the break *rule* is not readable;
the unknowns are the ones that bite (a trailing space at a break, a `'\n'` landing
at a wrap boundary, a token wider than 416 px with `wrapping = true`), and a
one-line disagreement is a silent screen overrun (section 8.2). And the fonts
belong to the OS, so an OS update that changes one advance width repaginates the
whole book and invalidates every offset in the binary with no signal.

Runtime pagination concentrates the risk in one undocumented OS function, and
that risk retires in an afternoon: step 1 of section 9 is a probe APDU that calls
it on real book bytes. If it misbehaves, the fallback is a build-time index with a
line budget one below what the screen renders (section 8.7).

The one thing the runtime scheme cannot give for free is the total page count
before the walk. Show `412` without a denominator until the walk finishes, and run
the walk once at first open behind a "Preparing" screen. A thousand trampoline
calls each scanning about 280 bytes is well under a second by any reasonable
estimate, and that is an estimate; the settling measurement is a timed probe on
Speculos and on a real Flex.

### 2.4 The position is a byte offset

`ReaderNvm.pos` is a **byte offset into the book**. The page number is derived for
display. Nothing stores it as truth.

Page numbers are a function of (text, font, width, line budget). An offset is a
function of the text alone. Changing font size, re-walking after an OS update, and
handing the position to another device all become the same operation: translate
the offset into whatever pagination is current by binary-searching the anchors and
walking forward. Switching size re-walks and re-anchors; the offset is preserved
exactly and the top of the screen lands on the same word.

## 3. The reading screen

### 3.1 Layout

```
y=0                       +--------------------------------------+
                          |  <     412 / 970              [list] |   header, 96
y=96   ------------------ +--------------------------------------+
                          |                                      |
                          |  In my younger and more vulnerable   |
                          |  years my father gave me some        |
                          |  advice that I've been turning over  |
       body, 408 px       |  in my mind ever since.              |
                          |                                      |
                          |                                      |
y=504  ------------------ +-------------------+------------------+
                          |         <         |         >        |   footer, 96
y=600                     +-------------------+------------------+
```

Built the way every other screen in this app is built:

- `nbgl_layoutAddHeader` with `HEADER_EXTENDED_BACK` (back key, centred text,
  touchable key on the right). Its height is `TOUCHABLE_HEADER_BAR_HEIGHT`, a
  fixed 96. The existing `Layout::header` wrapper uses `HEADER_TITLE`, whose
  height follows its text, so the reader needs the new variant of section 0.2.
  The back key returns to the library; the centred text is the position
  indicator; the right key opens the table of contents.
- `nbgl_layoutAddText(handle, ptr::null(), page)` for the body, through the
  existing `Layout::text`. Passing NULL for the main text puts the page in the
  sub-text slot, which NBGL renders in `SMALL_REGULAR_FONT` =
  `BAGL_FONT_INTER_REGULAR_28px` with `wrapping = true` (measured,
  `nbgl_layout.c`, `addListItem`).
- `nbgl_layoutAddSplitFooter(handle, "<", TOKEN_PREV, ">", TOKEN_NEXT, 0)`,
  through the existing `Layout::split_footer`.

The page indicator goes in the header as plain centred text.
`nbgl_layoutAddPageIndicator` does not exist; `nbgl_layoutAddProgressIndicator` is
marked `@deprecated` and its header type `HEADER_BACK_AND_PROGRESS` is documented
"only on Stax"; and the Flex mechanism,
`nbgl_layoutNavigationBar_t.withPageIndicator`, carries a header comment saying it
"is incompatible with a footer". The footer is the page-turn affordance, so the
header carries the text. Format it as `412 / 970`, or `412` alone until the walk
has produced a denominator.

### 3.2 Lines per page

`nbgl_layoutAddText` goes through `addListItem` (measured, `nbgl_layout.c:1456`
and `:729`). With a NULL main text and a non-NULL sub text, the container height
is computed twice, and the second computation **replaces** the first:

```c
container->obj.area.height
    = LIST_ITEM_MIN_TEXT_HEIGHT + 2 * LIST_ITEM_PRE_HEADING;   // 40 + 52 = 92
...
else {                                     // itemDesc->text == NULL
    subTextArea->obj.alignmentMarginY = SUB_HEADER_MARGIN;     // 28
    container->obj.area.height        = SUB_HEADER_MARGIN;     // assignment
}
...
container->obj.area.height
    += subTextArea->obj.area.height + subTextArea->obj.alignmentMarginY;
```

So the container ends at `28 + textHeight + 28`. **The padding is 56 px** and the
usable text height is 408 - 56 = **352 px**, which gives

- `INTER_REGULAR_28px`, line height 36: `floor(352 / 36)` = **9 lines** (324 px)
- `INTER_MEDIUM_36px`, line height 40: `floor(352 / 40)` = 8 lines, and see 3.4

These are derived from SDK constants, untested on a screen. The measurement that
settles them is step 3 of section 9: draw a page of 12 known lines and read the
`y` coordinates out of Speculos `/events`.

Two things follow from that code and they matter more than the arithmetic.

**The container height follows the text.** Nothing in `addListItem` clamps it.
Handing NBGL a ten-line page produces a ten-line container drawn under the footer,
silently. The line budget has to be enforced upstream, which is exactly what
`maxNbLines` does (invariant 2).

**Recovering the 56 px is possible and is not worth it in the first build.**
Building the body as a raw `nbgl_text_area_t` through `nbgl_objPoolGet` at y=96
with height 408 buys one more line at the small font. That path is compiled out of
production today (`#[cfg(feature = "artprobe")]` on `struct Screen`) and its own
comment records that its draw "never paints" without a specific sequence. One line
out of nine does not justify an unproven draw path in a shipping binary.

### 3.3 Characters per line, and the page count

Advance widths are exact and kerning is zero, so a line's width is the sum of its
characters. The frequency mix of English is the only unknown.

Weighting the measured advance table by the character frequencies of an English
prose sample (`docs/protocol.md`, ASCII only) gives a **mean advance of 13.23 px**
in Inter Regular 28px, so **31 characters per line** at 416 px (measured table,
inferred mix). Inter Medium 36px gives 17.26 px and 24 characters.

The SDK's own hardcoded 16 px (`nbgl_use_case.c:3163`, comment
`// 16 for average char width`) matches the unweighted mean over all 95 glyphs and
is deliberately pessimistic for running text.

At 9 lines:

| chars/line | chars/page | pages for 280 KB |
|---|---|---|
| 26 (SDK constant) | 234 | 1200 |
| 31 (frequency-weighted) | 279 | 1000 |
| 34 (optimistic) | 306 | 915 |

**Expect 900 to 1200 pages for Gatsby at the small font** (inferred). At the large
font expect 1300 to 1600. The measurement that settles this is the first on-device
walk, which produces the exact number as a side effect and should be recorded
here.

A Flex page holds about a fifth of a paperback page. Gatsby's 180 paperback pages
becoming 1000 device pages is the correct order of magnitude.

### 3.4 The large face, and where it comes from

Inter Medium 36px is a genuine large-print mode: line height 40 against 36, and
glyphs roughly 30 percent wider, so about a third fewer characters per page.

**`nbgl_layoutAddText` cannot render it.** Its sub-text slot is hardcoded to
`SMALL_REGULAR_FONT` and its main-text slot to `SMALL_BOLD_FONT` (measured,
`nbgl_layout.c`, `addListItem`), which on Flex are `BAGL_FONT_INTER_REGULAR_28px`
and `BAGL_FONT_INTER_SEMIBOLD_28px` (measured, `nbgl_fonts.h`, Flex block). No
parameter reaches the font id.

The route to the large face is **`nbgl_layoutAddTextContent`'s `title` field**,
hardcoded to `LARGE_MEDIUM_FONT` = `BAGL_FONT_INTER_MEDIUM_36px` (measured,
`nbgl_layout.c:1594`), at full `AVAILABLE_WIDTH` with `PRE_TITLE_MARGIN` = 16 px
above it and nothing below. Usable height is 408 - 16 = 392 px, so
`floor(392 / 40)` = **9 lines** (derived), one more than the sub-text path would
allow. It is in the generated bindings, and it is app-side code with no trampoline
to require, so it links. It needs the `Layout::title_text` wrapper of section 0.2.

Put the toggle on the table of contents page as a single row that flips between
"Text size: normal" and "Text size: large". A toggle costs a re-walk (section 2.4)
and an NVM write, and neither belongs on a gesture a thumb makes by accident while
turning pages.

If flash gets tight, dropping the large size saves the toggle and the wrapper,
because the anchors are recomputed on the device.

### 3.5 Refresh

The app has never called `nbgl_refreshSpecial`. `Layout::draw()` calls plain
`nbgl_refresh()`. The modes available (`nbgl_types.h:337-345`):

```c
FULL_COLOR_REFRESH,          ///< to be used for normal refresh
FULL_COLOR_PARTIAL_REFRESH,  ///< to be used for small partial refresh
FULL_COLOR_CLEAN_REFRESH,    ///< lock screen display (cleaner but longer)
BLACK_AND_WHITE_REFRESH,     ///< pure B&W area, when contrast is important
BLACK_AND_WHITE_FAST_REFRESH,///< pure B&W area, when contrast is not priority
INIT_REFRESH,                ///< fully clear the screen in white
```

The word "ghost" appears nowhere in `lib_nbgl` or `include/`. There is no
documented ghosting characterisation to design against.

The plan: `BLACK_AND_WHITE_FAST_REFRESH` for a page turn, since the page is pure
black text on white and speed is what a page turn needs, and
`FULL_COLOR_CLEAN_REFRESH` every twelfth turn and on every entry into the reader.
Twelve is a guess; the settling measurement is a human looking at a real Flex,
because Speculos renders a clean framebuffer and will never show ghosting.

Do not use `POST_REFRESH_FORCE_POWER_ON_WITH_PIPELINE`. Its documented constraint
is that "successive draws & refreshes areas must not overlap", and consecutive
pages of a book overlap completely.

## 4. The build pipeline

Three stages, all host-side, all run by `build.rs`.

### 4.1 Source

Project Gutenberg's Great Gatsby, UTF-8, checked into the repo under
`device-app/book/gatsby.txt` so the build is reproducible offline and a diff shows
exactly what the device will say. Gutenberg's licence header and footer are
stripped by hand, once, and the stripped file is what is committed.

### 4.2 Preparation

A small Rust build-dependency, or a Python script invoked by `build.rs`:

1. **Transliterate to ASCII.** Mandatory, from section 1.5: the fonts cover
   0x20..0x7E and nothing else, and Gatsby's Gutenberg text is full of characters
   that do not exist on this device.

   | in | out |
   |---|---|
   | `—` U+2014 em dash | ` -- ` |
   | `–` U+2013 en dash | `-` |
   | `‘` `’` U+2018/2019 | `'` |
   | `“` `”` U+201C/201D | `"` |
   | `…` U+2026 | `...` |
   | `\u{a0}` nbsp | space |
   | `æ` `œ` and any accented letter | ASCII fold |

   Note the spaces around the em dash replacement. A bare `--` glues two words
   into one unbreakable token, and NBGL has no hyphenation to fall back on
   (section 8.6).

2. **Normalise whitespace.** Collapse runs of spaces, strip trailing spaces,
   convert CRLF to LF, collapse blank-line runs to a single `\n\n`. Preserve
   paragraph breaks; they are the only structure the reader has.

3. **Assert the alphabet.** Every byte is in `0x20..=0x7E` or is `\n`. Fail the
   build otherwise, loudly, naming the offending byte and its offset. This is the
   whole defence against section 8.5 and it costs three lines.

4. **Assert no oversized token.** Split on whitespace, sum advance widths from a
   width table checked into the repo, fail if any token exceeds 416 px, run it
   against both fonts. The width table is 95 bytes per font and is decoded once,
   by hand, out of `libnbgl_shared_screenshots_flex.a` (section 1.5 gives the
   object, the section and the bitfield layout: `width = (word2 >> 1) & 0x3F`).
   It is checked in, because reading the archive at build time would drag WSL
   paths into the build.

5. **Assert the size.** `BOOK_LEN <= 326986` (section 1.3), with the error naming
   the ceiling and the linker message it would otherwise produce.

6. **Find the chapters.** Gatsby has nine, marked by a line that is a roman
   numeral alone. Emit `CHAPTERS: [u32; 9]` of byte offsets plus a 9-entry table
   of one-line titles. About 60 bytes in `.rodata`, small enough not to matter
   there.

### 4.3 Emission

`build.rs` writes into `OUT_DIR`, never into the source tree. The existing glyph
generation writes into `glyphs/` in-tree, which is fine for three small PNGs and
wrong for a 280 KB blob.

```
$OUT_DIR/book.bin        the prepared ASCII, NUL-terminated
$OUT_DIR/book_meta.rs    BOOK_LEN, BOOK_HASH, CHAPTERS, CHAPTER_TITLES
```

consumed as

```rust
include!(concat!(env!("OUT_DIR"), "/book_meta.rs"));

#[used]
#[link_section = ".text"]
pub static BOOK: [u8; BOOK_LEN] = *include_bytes!(concat!(env!("OUT_DIR"), "/book.bin"));
```

The vendored SDK already uses exactly this `include!(concat!(env!("OUT_DIR"),
...))` shape for install parameters
(`device-app/vendor/ledger_device_sdk/src/app_info.rs:23-24`), so the pattern is
proven in this build.

`BOOK_HASH` is the SHA-256 of `book.bin`, exposed by a development APDU. It is how
a host test proves it is reasoning about the same book the device holds, and how
the anchor fingerprint (section 5.2) detects that the book itself changed.

Add `println!("cargo:rerun-if-changed=book/gatsby.txt")` to `build.rs`. Line 119
there currently emits `rerun-if-changed=script.ld` for a file that does not exist
in this repo; leave it alone, it belongs to the flash-diet work.

## 5. The NVM state

### 5.1 Nothing goes into `PresseNvm`

`PresseNvm` is 1176 bytes, is wrapped in `AtomicStorage` (two copies in flash),
and is copied by value onto the stack on every screen draw. Putting the reading
position in it would mean:

- every page turn rewrites 1176 bytes, three flash pages erased per turn instead
  of one;
- every page turn rewrites the bearer key, the certificates and the provenance
  chain, the one region in this app where a bad write is unrecoverable;
- every page turn puts `pressing_priv`, the bearer scalar, on the stack. The app
  has a `crypto::scrub` (volatile zeroing, `crypto.rs`) called on exactly those
  slots in `press.rs` and `give.rs` precisely so a scalar does not linger there.
  A reader reading `PresseNvm` per turn drops a bearer key onto the stack a
  thousand times per book, with no scrub in sight.

The reading state gets its **own `.nvm_data` object**, following the precedent of
`ART_MASTER` / `ART_PRESSING` (`state.rs`), which exist for the same reason, and
whose comment says so: "Kept out of `PresseNvm` because that struct is copied
through the stack on every read."

The atomicity `PresseNvm` provides is unnecessary here. A power cut that leaves a
stale reading position beside a cleared pressing costs a reader nothing: the
reader works without a copy anyway (section 6), so a leftover position shows the
next person a page they did not choose. That is a mild privacy wrinkle, addressed
by clearing the position in the same handler that clears the pressing. Nothing
about double-spending depends on it.

### 5.2 The struct

```rust
/// Reading state. Its own `.nvm_data` object so a page turn never rewrites the
/// ownership struct, so a page turn erases one flash page where `PresseNvm`
/// would erase three, and so no page turn puts the bearer scalar on the stack.
#[derive(Clone, Copy)]
pub struct ReaderNvm {
    /// 0 until the book has ever been opened on this device.
    pub opened: u8,
    /// 0 normal, 1 large. Selects the font and invalidates the anchors.
    pub font: u8,
    /// Pages between two anchors, computed by the walk.
    pub stride: u16,
    /// Byte offset in `BOOK` of the first character of the current page.
    /// The canonical position. Page numbers are derived from it, never stored
    /// as truth, because they change with the font and with an OS update.
    pub pos: u32,
    /// Total pages under the current font, 0 while the walk has not run.
    pub pages: u16,
    /// Current page under the current font, for display. Recomputed on resume.
    pub page: u16,
    /// Validates `anchors`: the book hash folded with the font id, the line
    /// budget, `nbgl_getFontLineHeight` and the measured width of a probe
    /// string. The fonts live in the OS, so an OS update can repaginate the
    /// whole book with no other signal.
    pub fingerprint: u32,
    /// Byte offset of the first page of every `stride`-th page. A binary search
    /// here plus at most `stride - 1` forward steps reaches any page.
    pub anchors: [u32; 32],
}
```

1 + 1 + 2 + 4 + 2 + 2 + 4 + 128 = **144 bytes**. Wrapped in `AtomicStorage` that
is 288, and page-aligned in `.nvm_data` that is **512 bytes of `data_size`**, one
flash page, against a 400 KiB region that is 20.2% used.

Boot-check it with `scripts/boottest.sh` at 3/3 the moment it lands, on its own,
before any reader code exists. A new object in `.nvm_data` moves the section's
layout, and every offset in it has to stay inside what a stock emulator maps
(section 8.3). A struct landing past that boundary reads as zeros with no error
anywhere, which is exactly how a resume feature looks correct while silently
forgetting the page.

One anchor set. Anchors for both font sizes would cost a second 512-byte
page; recomputing on a size change costs a walk the user waits through once. The
walk is the cheaper thing to spend.

### 5.3 How often the position is written

Writing an 8-byte field and writing a 512-byte page cost the same, because
`nvm_write` erases a page either way. The only question is how many page erases a
reader causes.

A full read of Gatsby is about 1000 page turns. At a flash endurance of 100,000
erase cycles per page that is about 100 complete readings before the page wears
out, and nobody reads one novel 100 times on one device, so **write on every page
turn**. At 10,000 cycles it is 10 readings, which is not enough.

The endurance figure is not in the SDK and not in this tree. The measurement that
settles it is Ledger's or ST's specification for the secure element's flash. Until
somebody has that number, per-turn writing is provisional.

The fallback, if the number comes back low, is ready and cheap: keep the live
position in a `static AtomicU32` and commit it to NVM on leaving the reader, on
Quit, before serving any APDU, and every sixteenth turn as a crash floor. That
weakens "survives power loss" to "survives power loss to within fifteen pages", a
real regression to be taken only under duress.

Two rules apply either way:

- skip the write when the offset has not changed (a tap on `<` at page 1);
- never call `Store::get()` on the reader path (invariant 4).

## 6. Ownership, and the reader without a copy

The text is free and the object is scarce. **The reader works fully without a copy
in NVM.** Anyone who installs the app reads the whole book, remembers their page,
changes the font size, and uses the table of contents.

The library grows a permanent row for the book, because the book lives in the app
binary and every install has it:

| state | row 1 | row 2 | row 3 |
|---|---|---|---|
| nothing held | The Great Gatsby / *Reading copy* | | |
| holds a pressing | The Great Gatsby / *No. 4 of 50* | Pressing 4/50 (card) | |
| holds a master | The Great Gatsby / *Reading copy* | Master, 46 left (card) | |
| both | The Great Gatsby / *No. 4 of 50* | Master | Pressing |

Three rows at 92 px is 276 px, inside the 408 the page has. Four would be exactly
408 and there is no fifth. That bound is already enforced elsewhere in this app by
a typed array length (`BACK_ROWS: usize = 4` in `handlers/collection.rs`, "so a
fifth one does not compile"), and the library's row set should get the same
treatment.

Tapping the book row opens the reader. Tapping a record row opens the existing
card, unchanged.

An unowned install hides the record card entirely, the number, the Edition ID, the
Device ID's History sub-page, the give flow, and the challenge response.
`INS_CHALLENGE` already fails closed on a device holding no copy and stays that
way. The empty-library copy ("No records yet" / "You gave your copy away") has
nowhere to live once the library is never empty; move it onto the book row's
subtitle so the distinction between "nothing yet" and "nothing any more" survives.

An owned install adds the number to the header of the table of contents page, and
nothing to the reading page itself. A page of a novel is a poor place for a serial
number.

## 7. The bookmark travels with the copy

The give machinery works and carries a security fix from the same week. The change
here is deliberately the smallest one that carries the position.

### 7.1 What travels

The reading offset, four bytes. The page number stays behind because it is
derived; the font size stays behind because it is the new reader's own preference;
the anchors stay behind because they are a cache.

### 7.2 Where it rides

In **phase 1**, the part of the ceremony that is free to abandon because nothing
on either device has changed yet. Alongside `GIVE_CHAIN` / `TAKE_CHAIN`:

```
0x7E GIVE_BOOKMARK  (giver, paired)  -> pos(4) || mac(32)                 [36]
0x7F TAKE_BOOKMARK  (taker, paired)  data = pos(4) || mac(32)  -> ok      [36]
```

`TAKE_BOOKMARK` stages the value exactly as `TAKE_CHAIN` and `TAKE_PRESSING`
stage theirs, and it lands in NVM at `TAKE_ACCEPT`. Because the reader state is a
separate object, that is one extra write beside the atomic one, and section 5.1
says why that is acceptable.

**Phase 2 does not change** (invariant 5). `GIVE_OFFER`'s reply stays
`sealed bearerkey(32) || mac(32)`, 64 bytes; `TAKE_ACCEPT` still consumes it
verbatim; `GIVE_CANCEL` still refuses at `committed == 2`; the three-state byte is
untouched; the chain fold at `TAKE_HANDOVER` is untouched. **Do not extend the
sealed payload.** Its length is load-bearing on both sides, the pad is keyed to
the frame's sequence number, and dispatching on a sealed payload's length is a
parsing surface nobody needs.

Both instructions are MACed like every other ceremony frame
(`HMAC(K, [ins, seq] || payload)`), so a hostile relay cannot rewrite the page
number. A page number is not worth attacking, and it is cheaper to MAC it than to
explain why it is the one frame that is not.

If either side lacks the pair, the ceremony proceeds without it: the giver's
`GIVE_BOOKMARK` returns `InsNotSupported` and the taker starts at page one. A copy
without a bookmark is a book nobody has opened, which is a coherent thing for it
to be.

`CEREMONIE-VIDEO.md` and `docs/protocol.md` both carry the APDU map and both must
gain these two lines in the same commit, or the live cut fails.

### 7.3 What the giver loses

`clear_pressing()` gains nothing, because the reading state sits outside
`PresseNvm`. The `GIVE_FINISH` handler additionally resets `ReaderNvm.pos` to 0
and `page` to 1, in its own write. The giver keeps the book and loses their place
in it, which is what happens when you hand someone a paperback.

### 7.4 What the card shows

The record card's Back page is full: `BACK_ROWS = 4` is a typed array length, four
rows at 92 px is 368 px, and a fifth would land under the footer at y=504. **A
reading row cannot go on the Back page.** Put it on the front of the card, under
the number:

```
Pressing 4 of 50
Left at page 412 by 3F9A2C10
```

on a received copy, and

```
Pressing 4 of 50
You are on page 412
```

on one that has not changed hands. The giver's fingerprint is already in NVM as
`pressing_from`, and the phrasing is the whole point of the feature: a returned
book is dog-eared at their page.

Bound the string. `page` and `pages` are both `u16`, so the row cannot grow with
device state, which is the shape of bug AGENTS.md warns about.

## 8. Why the code looks like this, and what still bites

Each entry names how it manifests, how it is detected, and what the design does
about it. Where an instrument already exists in the tree, use it before building
another.

### 8.1 The book is in `.text`, and `data_size` is the tripwire

**How it manifests.** A `static` declared without `#[link_section = ".text"]`
lands in `.rodata` and `data_size` becomes about 298 KB. Nothing crashes, since
both sections spend the same 400 KiB, and that is what makes it easy to miss: the
build works, the app boots, and the one figure that would flag an accidental
`.rodata` or `.nvm_data` growth from then on is buried under the book.

**The access half of this risk is closed** at the ELF and hex level (section
1.4); the hardware half is open.

**What to do about it.** Read `data_size` off every single build
(`cargo ledger build flex | grep -oE "data_size: [0-9]+"`) and treat any movement
as a placement bug until proven otherwise. It was measured unmoved at 18432 across
256 KiB of payload, so movement means something real.

### 8.2 The silent screen overrun

**How it manifests.** A page renders one line more than fits. NBGL draws it
anyway, under the footer, showing only the tops of the glyphs. Past y=600 Speculos
faults the draw. The text is in `/events` either way, so a test that reads screen
text passes.

**Why the design looks the way it does.** `addListItem` sizes its container from
the text (section 3.2) and clamps nothing, so the budget has to be enforced before
NBGL sees the string. `nbgl_getTextMaxLenInNbLines` takes the budget as
`maxNbLines`, which makes the overrun impossible by construction on the runtime
path. That is the strongest single argument for runtime pagination.

**How to detect it.** `assert_page_fits(dev)` already exists
(`tests/presse_client.py:147`) and asserts that nothing except a known footer
label has `y + h > 504`. Call it after every page turn in the reader tests, and
over a sweep: page 1, every 97th page, the last page, both font sizes.

### 8.3 The emulator's notch, and the 400 KiB edge

**How it manifests.** The app installs, panics before its first APDU
(`exiting_panic` -> `exit_app(0)`), and answers nothing. Under Speculos it has one
known cause, in the emulator itself: stock Speculos sizes the app's mapping from
the `PT_LOAD` holding `.text` plus one page, `.nvm_data` is a separate `PT_LOAD`,
and only 4096 to 7680 bytes of it arrive. At a load size that is a multiple of
4096 exactly one page arrives, both `AtomicStorage` validity flags fall outside
it, and `which()` panics. Physical Flexes are unaffected.
`docs/speculos-nvm-loading.md` has the measurement and the fix.

**Every "boot window" number this repo once carried was this**, and AGENTS.md
lists them under "Numbers in the history to disbelieve". Do not re-derive any of
them.

**How to detect it.** Three instruments, already in the tree.
`scripts/patch-speculos.sh` fixes the emulator in `~/venv-ledger` and `env.sh`
re-applies it on every source, so the notch stops firing at all; `pip install -U
speculos` puts the bug back. `presse_check_load_size` in `scripts/env.sh` fails
the build when `_erodata - _text` is a multiple of 4096 (`ALLOW_NVM_NOTCH=1`
overrides). `scripts/boottest.sh` launches Speculos, polls `b501000000`
(GET_INFO) once a second up to a deadline, and distinguishes `ok` / `panic` /
`slow`; a `slow` is retried and never counted, and a point is usable at 3/3.

**What the reader has to live with.** The image goes from 82846 bytes to something
near 360 KB, 88% of the region. `load_size` moves in 512-byte steps with the
payload, so one payload size in eight is refused, and the 512-byte band
immediately below the 326986 ceiling is one of them (section 1.3). A book chosen
to fill the region has to land in the top band or step down by 1024 bytes.

A book that does not fit is answered by packing (section 2.2) before it is
answered by a shorter work.

### 8.4 RAM

**How it manifests.** Heap exhaustion or a smashed `app_stack_canary`, both
surfacing as a panic and a silent exit.

**The budget.** SRAM is 36 KiB. `.bss` already runs to `END_STACK` (36864 bytes),
covering the 8192-byte heap and everything else, and the practical working figure
is about 24 KB. A screen draw already puts 1176 bytes of `PresseNvm` on the stack,
and the record card composes a bitmap on the heap, which is why `main.rs` drops
the library before dispatching a UI-gated APDU.

**What the design does.** One `static mut PAGE_BUF: [u8; 1024]` in `.bss`,
zero-initialised (this target forbids `.data`, so every static must be all-zero,
per the comment at `app_ui/library.rs:134`). No heap on the reader path, no
`format!` for the body, no `Vec`. The `CString`s for the header and the footer
labels are small and can stay. The buffer bound is a compile-time check: at 9
lines and a minimum advance of 7 px a page cannot exceed `9 * 416 / 7` = 535
bytes, so 1024 is comfortable, and `nbgl_getTextMaxLenInNbLines` is called with a
`maxLen` the buffer can hold.

Adding 1024 bytes of `.bss` takes 1024 bytes from the stack, since `.bss` runs to
`END_STACK`. `STACK_MIN_SIZE` is 1500 and the link asserts against it, so the link
complains when it gets desperate and stays quiet when it is merely tight.

### 8.5 Text encoding

**How it manifests.** Bytes above 0x7E index past `last_char` in the font's
95-entry character table. Best case a wrong glyph, worst case a read off the end
of the table and a faulted draw. Gatsby's Gutenberg text is full of them: em
dashes, curly quotes, ellipses.

**How to detect it.** The build-time alphabet assertion of section 4.2 step 3. It
is three lines and it makes this class of bug impossible.

**What to do about it.** Transliterate in the pipeline. A unicode font is
unavailable on three counts (section 1.5), and four of the five unicode accessors
have no trampoline, so an app calling them does not link.

### 8.6 Line breaking and long tokens

**How it manifests.** NBGL breaks on spaces when `wrapping = true` and has no
hyphenation. What it does when a single token is wider than 416 px is
undocumented and the source is in the OS. A likely outcome is an overflowing line,
which is 8.2 again.

The realistic source of such a token in Gatsby is a naive em dash replacement:
`word—word` becoming `word--word` is one token of eighteen characters, and
eighteen characters at Inter Medium 36px is around 310 px, close enough to 416 to
matter for longer pairs.

**How to detect it.** The build-time token width assertion of section 4.2 step 4,
run against both fonts.

**What to do about it.** Put spaces around the `--`. Break a genuine long token (a
URL, a long compound) in the source text.

### 8.7 Pagination drift

**How it manifests.** An index says page 412 starts at offset X; the renderer
breaks the text differently and page 412 needs ten lines where nine fit. Silent
overrun, or a page ending mid-sentence with a gap.

**Why the design avoids it.** This risk exists only under build-time pagination,
and section 2.3 chose runtime pagination largely to remove it. Under the runtime
scheme the equivalent risk is the anchors going stale after an OS update changes a
font, and the fingerprint in `ReaderNvm` catches that: fold
`nbgl_getFontLineHeight(font)` and `nbgl_getSingleLineTextWidth(font, probe)` for
a fixed probe string into the stored value, re-walk when it moves.

**If a build-time index is ever adopted**, two things make it safe. A development
APDU `READER_LINES(page)` that calls `nbgl_getTextNbLinesInWidth` on that page's
slice and returns the count, plus a host script sweeping every page and asserting
`<= budget`, converts an open-ended risk into a CI check. And generate the index
with a budget one line below what the screen renders, so a one-line disagreement
still fits, at a cost of about 11 percent more pages and about 380 bytes of index.

### 8.8 E-ink ghosting

**How it manifests.** Faint residue of the previous page under the current one,
accumulating over turns until the page is grey.

**How to detect it.** Only on hardware. Speculos renders a clean framebuffer and
will never show it. Any refresh-mode decision validated only on Speculos is
unvalidated.

**What to do about it.** Section 3.5. The app has never called
`nbgl_refreshSpecial` at all, so the first call is itself a change worth
boot-checking.

### 8.9 Both ends of the book

**How it manifests.** `>` on the last page walks off the end of the array and
renders whatever follows the book in flash, or wraps to page 1, or panics. `<` on
page 1 does the mirror.

**What the design does.** The page-turn handler is the one place here where
fail-closed matters (invariant 6). Both bounds are explicit:

```rust
// The button that cannot act does nothing, and the page it would have gone
// to does not exist. No wrap, no silent clamp.
if next_offset >= BOOK_LEN as u32 { return; }
if page == 1 { return; }
```

Consider greying the inert half of the footer. `nbgl_layoutBar_t` has an
`inactive` flag; `nbgl_layoutAddSplitFooter` does not obviously expose one, so the
cheap version renders the label as `.` or an empty string at the ends.

The last page is short, which is correct and needs no special case, and the walk
must count it: the anchor walk terminates when `nbgl_getTextMaxLenInNbLines`
returns a `len` reaching the terminating NUL.

### 8.10 The reader and the APDU loop

**How it manifests.** The reader is on screen. The host sends anything at all,
`run_event_loop` returns `Exit::Apdu`, and the reader tears itself down and lands
back on the library, losing its page. The ceremony scripts poll `GET_INFO`, so
this fires constantly during any bench session.

**Why the code already has half the answer.** `warrants_library_redraw`
(`main.rs:232`) carries the hand-maintained list of which instructions invalidate
the screen, under the rule "repaint after a UI-gated command, never after a pure
data-plane one". That is what keeps a fifty-chunk sleeve transfer from repainting
fifty times.

**What the reader adds.** `evicts_reader(ins)`, the mirror: true for the UI-gated
ceremony instructions, false for `GET_INFO`, `GET_BUNDLE`, `GET_ART`, `CHALLENGE`
and the rest of the data plane. On a non-evicting command, serve it and redraw the
same page.

A ceremony must be able to take the screen, because a ceremony has a human on both
ends and a book does not.

### 8.11 `Store::get()` writes

**How it manifests.** Nothing, most of the time. `Store::get()` generates the
device keypair and writes NVM when `initialized == 0` (`state.rs`), so a function
everything treats as a read also writes. A reader calling it per page turn writes
flash on a virgin device once, then costs a 1176-byte stack copy on every turn
forever, bearer scalar included.

**What the design does.** The reader never calls `Store::get()` (invariant 4). It
reads `ReaderNvm` directly. The one place ownership matters to the reader is the
number shown on the contents page, read once on entry.

### 8.12 The walk looks like a hang

**How it manifests.** The walk runs at first open and blocks the UI for its
duration with no screen up.

**What to do about it.** Draw a "Preparing the book" screen before starting, with
no ticker and no animation, and refresh once. Measure the walk on Speculos and on
hardware and record the number here. If it exceeds a second or two, chunk it: walk
64 pages per iteration, returning to the event loop between chunks so APDUs are
still served.

## 9. Build order

Each step is testable on its own and each can fail without wasting the next. The
two steps that used to open this list, proving a `.text` static reads and sweeping
the size ceiling, are done (sections 1.3 and 1.4).

**1. Prove the primitive.** A development APDU calling
`nbgl_getTextMaxLenInNbLines(INTER_REGULAR_28px, &PROBE_TEXT[off], 416, 9, &len,
true)` on a chunk of real English, returning `len`. *Passes when successive calls
tile the text with no gaps and no overlaps, and each slice measures at most 9
lines by `nbgl_getTextNbLinesInWidth`.* This is where the runtime-pagination
decision is confirmed or reversed, and where the break semantics of section 1.7
stop being documentation.

**2. The pipeline.** `book/gatsby.txt` in, `book.bin` plus `book_meta.rs` out,
with the alphabet, token-width and size assertions. *Passes when the build
succeeds, the assertions are proven to fire on deliberately corrupted input, and
`BOOK_LEN` is a real number this document can be updated with.* Read `data_size`
and confirm 18432.

**3. One page.** The `HEADER_EXTENDED_BACK` wrapper, body via
`nbgl_layoutAddText(NULL, page)`, split footer, page 1 only, no navigation, no
NVM. *Passes when Speculos shows the first paragraph and `assert_page_fits`
holds.* This is where the line budget stops being derived and becomes measured:
read the `y` coordinates out of `/events` and settle section 3.2.

**4. Turning.** `TOKEN_NEXT` / `TOKEN_PREV` on the footer halves, position in a
RAM `static`, forward via one `nbgl_getTextMaxLenInNbLines` call, backward via a
re-walk from page 1 (slow and correct; anchors arrive in step 6). Both ends inert.
*Passes when 40 pages forward and 40 back land on the same text, and
`assert_page_fits` holds at each.*

**5. Persistence.** `ReaderNvm` as its own `.nvm_data` object, offset written on
every turn. Boot-check 3/3 the moment the struct lands, before any code uses it.
*Passes when Speculos is killed mid-book and relaunches on the same page.*

**6. Anchors and the count.** The walk, the fingerprint, the binary search, the
"Preparing" screen, `page / pages` in the header. *Passes when a jump to a random
page is instant, the count matches a host-side count of the same text, and a
deliberately corrupted fingerprint triggers a re-walk.*

**7. Contents.** Three pages of four chapter rows, reached by the header's right
key, footer split "Back" / "More". A jump sets `pos` to the chapter offset and
recomputes the page. *Passes when every chapter lands on its first line and
`assert_page_fits` holds on all three contents pages.*

**8. Refresh discipline.** `BLACK_AND_WHITE_FAST_REFRESH` per turn,
`FULL_COLOR_CLEAN_REFRESH` every twelfth and on entry. Boot-check. *Passes on a
real Flex, by eye, over fifty consecutive turns.* Speculos cannot judge this.

**9. The library.** The book row, the unowned and owned subtitles, the row set
bound as a typed array. `evicts_reader(ins)`. *Passes when the existing library
and card tests still pass and a `GET_INFO` poll no longer evicts the reader.*

**10. Large text.** The `nbgl_layoutAddTextContent` wrapper, the toggle on the
contents page, re-walk on switch, offset preserved. *Passes when switching size
mid-chapter lands on the same sentence, and `assert_page_fits` holds at the large
font, where the margin is thinner.*

**11. The bookmark travels.** `GIVE_BOOKMARK` / `TAKE_BOOKMARK`,
`docs/protocol.md` and `CEREMONIE-VIDEO.md` updated in the same commit. *Passes
when the dual-Speculos give test shows the taker resuming on the giver's page, and
when an old giver answering `InsNotSupported` still completes a give with the
taker starting at page one.*

**12. Hardware.** Everything above runs on an emulator whose loader this repo
patches. One session on a physical Flex closes section 1.4's remaining half,
section 3.2's derivation, section 3.5's refresh interval and section 8.8 entirely.

## 10. What is settled, and what is not

| Claim | Status |
|---|---|
| 400 KiB per-app region | measured, link-time. OS acceptance of more is untested |
| 326986-byte maximum payload | measured, by bisection |
| `load_size = 512 * ceil((payload + 74934) / 512)`, ceiling 401920 | measured, zero error at eight points |
| `data_size` unmoved at 18432 | measured across 256 KiB of payload |
| A `.text` static reads back correctly | measured in the ELF and in `presse.hex`. Hardware inferred |
| Fonts cover 0x20..0x7E and nothing else | measured, three independent counts |
| Line heights 36 / 36 / 40, kerning 0 | measured, decoded from `libnbgl_shared_screenshots_flex.a` |
| Body padding 56 px, 352 px usable, 9 lines | derived from SDK constants. Untested on a screen (step 3) |
| The large face needs `nbgl_layoutAddTextContent` | measured, `nbgl_layout.c`. Untested on a screen |
| 31 characters per line | measured width table, inferred English mix |
| Gatsby is about 280 KB and fits with 14% margin | inferred from a word-count estimate (step 2) |
| 900 to 1200 pages | inferred (step 6 produces the exact number) |
| Break semantics of `nbgl_getTextMaxLenInNbLines` | unknown. The source is in the OS (step 1) |
| Flash endurance per page | unknown. Ledger or ST documentation. Decides section 5.3 |
| Ghosting, and at what interval | unknown. Human eyes on hardware (step 8) |
| Whether a swipe's direction reaches the app | unknown. `nbgl_layoutAddSwipe` takes one token for all directions and the direction arrives in the callback's `index`, which `layout_touch_callback` discards today |
| Anything at all on a physical Flex | untested |
