# The reader

How a book gets into the app, onto the screen, and back out of it.

Nothing here is built yet. Every number below is either measured (with its
source named) or marked as an estimate with the measurement
that would settle it. The distinction is load-bearing: this app has already
designed around a size ceiling that turned out to be an emulator bug
(section 8.3), and plausible-looking arithmetic is how that detour started.

## 0. What changes, and what does not

The artifact becomes a book. The device holds the text in the clear, in the app
binary, and holds ownership in NVM. Installing the app gives you the text.
Owning a numbered copy is a separate fact, carried by the certificate and the
bearer key exactly as it is today.

One book, one app. There is no book selector, no library of works, no download
path. `The Great Gatsby` is compiled in; a second book is a second app with a
different `static`, and the two coexist on the device because the OS gives each
app its own partition.

No encryption and no DRM. The text is public domain and the object is what is
scarce. A reader that decrypts is a reader that can fail to decrypt, and the
failure mode of a book that will not open is worse than the failure mode of a
book anyone can read.

The ceremony machinery (pairing, press, give, the three-state commitment, the
provenance chain) is not redesigned. Section 7 adds one instruction pair to the
phase that is free to abandon, and touches nothing else.

## 1. Ground truth

Everything in this section was read off the SDK this app builds against, or off
the current build. Sources are given so a future reader can re-derive rather
than trust.

### 1.1 The link

`ledger_secure_sdk_sys-1.16.2/devices/flex/flex_layout.ld`, complete:

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
400 KiB is the whole budget and not a code half of it.

The sections order in `link.ld` is the fact that decides this entire design:

```
.text        -> _text = _nvram_start
.rel_flash   -> ends at _nvram_data
.rodata      -> after _nvram_data
.data        -> empty, ASSERTed empty
.nvm_data    -> ends at _envram_data
```

`cargo-ledger-1.14.0/src/utils.rs:103` computes what the loader is told:

```rust
infos.data_size = envram_data - nvram_data;
```

**`data_size` = `.rodata` + `.nvm_data`.** A plain `static` in Rust lands in
`.rodata`, so a 270 KB book declared the obvious way adds 270 KB to `data_size`,
which is 18432 in the current build.

That number has no ceiling of its own. Both halves are carved out of the same
400 KiB, so a book costs the same flash whichever section holds it, and the only
hard limit is the region. What `data_size` does buy is a signal: kept small and
stable, any movement in it means something landed in `.rodata` or `.nvm_data`
that was not meant to, which is a cheap regression check to have on a build that
is about to grow by a factor of four. A 286 KB `data_size` would drown it.

`.text` is the other half of flash, immutable, with no relocations applied to
it. The instrument for growing it on purpose is a ballast static:

```rust
#[cfg(feature = "ballast_text")]
#[used]
#[no_mangle]
#[link_section = ".text"]
pub static BALLAST_TEXT: [u8; TEXT_BYTES] = [0xA5; TEXT_BYTES];
```

Filler there grows the immutable half of flash and leaves the mutable half
where it was, which is what makes a size sweep mean anything. **No such file is
in the tree**: `device-app/src/` has no `ballast.rs` and `Cargo.toml` has no
`ballast_text` feature, so step 1 of section 9 writes it before it can sweep.

**The book goes in `.text`, via `#[link_section = ".text"]`.** It keeps the book
out of the one number worth watching, and section 8.1 explains what it does not
yet prove.

### 1.2 The current build

Measured off `device-app/target/flex/release/presse` (ELF section headers and
symbol table) on 2026-07-31:

| section | addr | size |
|---|---|---|
| `.text` | `0xc0de0000` | 60928 |
| `.rel_flash` | `0xc0deee00` | 2352 |
| `.rodata` | `0xc0def800` | 11264 |
| `.nvm_data` | `0xc0df2400` | 7582 |
| `.bss` | `0xda7a0000` | 36864 |

`arm-none-eabi-size` reports `text` = 74544 (`.text` + `.rel_flash` +
`.rodata`), and the symbols give `data_size` = `_envram_data - _nvram_data` =
`0xc0df4000 - 0xc0def800` = 18432, confirmed against the `createApp` frame in
`presse.apdu`.

Two derived figures matter more than either of those.

**The image is `_text` to `_nvram_end`**, `0xc0de0000` to `0xc0df419e`, **82334
bytes: 20.1% of the 400 KiB region.** That is the app's real footprint, and the
400 KiB is the only ceiling there is.

**The load size is `_erodata - _text` = 74752**, the `p_filesz` Speculos sizes
its mapping from. It runs 208 bytes above `text`, because the linker pads
`.rel_flash` from 2352 up to 2560, and that padding moves with the relocation
count. 74752 mod 4096 = 1024, so this build clears the emulator notch described
in section 8.3.

Headroom, 400 KiB minus the 80.4 KiB image: **about 320 KiB** for text, index
and reader code together.

### 1.3 The fonts

Flex compiles in exactly three faces, each in a 4bpp and a 1bpp variant.
`ledger_secure_sdk_sys-1.16.2/devices/flex/c_sdk_build_flex.defines:24-26`:

```
#define HAVE_BAGL_FONT_INTER_REGULAR_28PX
#define HAVE_BAGL_FONT_INTER_SEMIBOLD_28PX
#define HAVE_BAGL_FONT_INTER_MEDIUM_36PX
```

Metrics decoded from the shipped `libnbgl_shared_screenshots_flex.a`, section
`._nbgl_fonts_`:

| font | height | line_height | kerning | first..last char |
|---|---|---|---|---|
| `INTER_REGULAR_28px` | 36 | **36** | 0 | 0x20..0x7E |
| `INTER_SEMIBOLD_28px` | 36 | **36** | 0 | 0x20..0x7E |
| `INTER_MEDIUM_36px` | 44 | **40** | 0 | 0x20..0x7E |

The names lie about line height. A budget computed from "28" or "36" is wrong by
a fifth to a third. Advance widths, decoded from the same archive: space 8,
`i` 7, `n` 16, `m` 24 in Inter Regular 28px; 10 / 9 / 22 / 32 in Inter Medium
36px. `char_kerning` is 0 for all six, so a string's width is the plain sum of
its characters.

`first_char = 0x20, last_char = 0x7E` on every Flex font. **Printable ASCII and
nothing else.** No accented characters, no em dash, no curly quotes, no
ellipsis character. The unicode mechanism exists as a type system but is not
enabled for Flex: `HAVE_UNICODE_SUPPORT` appears nowhere in the SDK, the real
gate `HAVE_LANGUAGE_PACK` is absent from the Flex defines, and of
`nbgl_getUnicodeFont`, `nbgl_getUnicodeFontCharacter`,
`nbgl_getUnicodeFontCharacterByteCount` and `nbgl_popUnicodeChar` not one has a
trampoline in `nbgl_stubs.h`, so an app calling them will not link.

There is no Inter unicode font anywhere in the tree.

The SDK's own rule of thumb for average character width is a hardcoded 16 px
(`nbgl_use_case.c:3162-3163`, comment `// 16 for average char width`), which at
`AVAILABLE_WIDTH` = 416 gives 26 characters per line. That is deliberately
pessimistic; real English mixes in a lot of 7 px letters and 8 px spaces.

### 1.4 The geometry

`nbgl_types.h`, Flex block: `SCREEN_WIDTH 480`, `SCREEN_HEIGHT 600`.
`nbgl_obj.h:80-82`: `BORDER_MARGIN 32`. `nbgl_layout.h:134`:

```c
#define AVAILABLE_WIDTH (SCREEN_WIDTH - 2 * BORDER_MARGIN)
```

**416 px.** This is the width to pass to every measuring call.

`nbgl_layout.h`, Flex block: `TOUCHABLE_HEADER_BAR_HEIGHT 96`,
`SIMPLE_FOOTER_HEIGHT 96`, `LIST_ITEM_MIN_TEXT_HEIGHT 40`,
`LIST_ITEM_PRE_HEADING 26`. A header plus a footer leaves 600 - 96 - 96 = **408
px** of body, which is the number AGENTS.md already records and the tests
already assert against (`FOOTER_RULE_Y = 504` in `tests/presse_client.py:123`).

### 1.5 The measuring API

All of these are OS-side and reached through a trampoline. The authoritative
list of what links is `nbgl_stubs.h`; being declared in `bindings.rs` proves
nothing. The ones that matter here all have trampolines:

```c
uint16_t nbgl_getSingleLineTextWidth(nbgl_font_id_e fontId, const char *text);   // 0xaf
uint16_t nbgl_getTextNbLinesInWidth(fontId, text, maxWidth, wrapping);           // 0xb4
uint8_t  nbgl_getTextNbPagesInWidth(fontId, text, nbLinesPerPage, maxWidth);     // 0xb5
bool     nbgl_getTextMaxLenInNbLines(fontId, text, maxWidth, maxNbLines,
                                     uint16_t *len, bool wrapping);              // 0xb7
uint8_t  nbgl_getFontLineHeight(nbgl_font_id_e fontId);                          // 0xae
```

`nbgl_getTextMaxLenInNbLines` is the pagination primitive: given a font, a
pointer into the text, a pixel width and a line budget, it writes into `*len`
the number of bytes that fit. Advance the pointer by `len` and you have the next
page. Nothing else is needed to break a book into pages.

Do not build on `nbgl_getTextNbLines`, `nbgl_getTextLength` or
`nbgl_getTextMaxLenAndWidthFromEnd`: they are in the header and in the bindings
and have no trampoline.

Wrapping is documented rather than readable. `nbgl_fonts.c` is not in the tree
at all (the whole font and draw layer moved into the OS at API level 26, and the
SDK clone is shallow), so the exact break semantics are known only from
`nbgl_obj.h:431`:

```c
uint8_t wrapping : 1;  ///< if set to true, break lines on ' ' when possible
```

Word-preserving when true, mid-word at maximum fitting length when false. No
hyphenation anywhere: the string "hyphen" does not occur in `lib_nbgl`.
Truncation is a literal `"..."`. `'\n'` is very probably a hard break (the SDK
ships multi-line literals and `nbgl_getTextNbLines` takes no font or width) but
the code proving it is not readable here.

### 1.6 What the app already does

Every production screen is built from raw `nbgl_layout` through one wrapper,
`src/app_ui/library.rs`. The object API (`nbgl_objPoolGet`, `nbgl_screenPush`)
is compiled out of shipping builds and lives only behind the `artprobe` feature.
Screens never update in place: a state change drops the whole `Layout` (its
`Drop` calls `nbgl_layoutRelease`) and builds a new one. Touch arrives as a
token through a single `onActionCallback` into an `AtomicU16`, read and cleared
by `touch_result_take()`. The ticker is all-zero everywhere.

`run_event_loop` (`library.rs:601-610`) is the whole concurrency model:

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

`nbgl_next_event_ahead()` blocks on an SE event; a finger event is pumped
through NBGL's hit testing as a side effect and fires the token callback, and an
APDU is stashed for replay rather than consumed. One pump, two exits.

There is exactly one call to `nbgl_refresh()` in production code
(`library.rs:580`, inside `Layout::draw`) and **zero** calls to
`nbgl_refreshSpecial*` anywhere in the tree. The app has never selected a
refresh mode.

`nbgl_layoutAddSwipe` is called nowhere. `Exit::SwipedLeft` / `SwipedRight`
exist but are produced only on the artprobe path.

Every `Store::put` today is APDU-driven. No gesture has ever written flash, with
one exception worth knowing: `Store::get()` lazily generates the device keypair
and writes NVM on first call, and `Library::draw()` calls it, so the first paint
on a virgin device is a flash write.

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

`include_bytes!` occurs nowhere in the repo today; the existing precedent for
baked-in binary data is `include_gif!` for glyphs, which exists because "a
runtime heap icon faults under PIC relocation on this target" (`build.rs:49-51`).
A byte array has no pointers inside it and needs no relocation of its contents,
so it is a simpler case than a glyph.

The blob ends in a single `NUL`. That NUL is what makes
`nbgl_getTextMaxLenInNbLines` terminate at the end of the book instead of
running off the array, and it is the only NUL in the file.

### 2.2 Format: raw ASCII

The book ships as plain 7-bit ASCII with `\n` between paragraphs and no other
control character. No packing in the first build.

The Great Gatsby is about 47,000 words. English averages roughly 4.7 letters per
word plus a separator, so **250 to 280 KB** raw. The exact figure falls out of
the first run of the pipeline and should be written into this document when it
does. Against the 320 KiB of headroom in section 1.2 that fits with 40 to 70 KB
of slack, which also has to cover the reader's code.

The alternatives, and why they lose:

**Five-bit packing** (a Z-machine-style three-characters-per-word scheme with
shift codes for capitals and punctuation) reaches roughly 0.7 bytes per
character, so about 190 KB. It saves 80 KB for a decoder of roughly 100 to 150
bytes, and it would roughly double the slack above. It is a real win, and it
should still be held in reserve until the raw build exists: a packed blob means
the reader cannot hand NBGL a pointer into flash, which forces every experiment
through a decoder that is itself unproven, on top of a `.text` read path that
has never been exercised either (section 8.1). Two unproven things at once, to
buy room the raw estimate already has. Design the read path behind a single
function

```rust
fn page_bytes(from: u32, max_len: usize, out: &mut [u8]) -> usize;
```

and packing becomes a one-file change later, invisible to everything above it.

**Word-dictionary packing** does better still: a 47,000-token novel has perhaps
6,500 distinct forms, so a 13-bit token stream costs about 76 KB and a
dictionary about 45 KB, around 120 KB total, with a decoder that is a lookup and
a copy. Capitalisation and punctuation need escape tokens and the build tooling
grows a real vocabulary pass. This is the right second move if the first build
runs out of room, and it beats 5-bit packing.

**General-purpose compression does not fit, and this is a measurement rather
than an opinion.** Deflate needs a 32 KiB sliding window in RAM. The whole SRAM
region is 36 KiB, of which `.bss` already consumes all 36864 bytes including the
8 KiB heap and the stack, and the practical working budget is about 24 KB.
Deflate's window alone exceeds it. LZ4 needs the block or a ring buffer of
comparable size. Heatshrink is the only member of the family that fits (1 to 2
KiB window, roughly 1 KB of code), and at that window size it gets perhaps 30 to
40 percent on English, which a word dictionary beats outright while decoding
faster and allowing random access. There is no `no_std` decompressor in the
dependency tree today; `flate2` and `miniz_oxide` are in `Cargo.lock` but reach
the tree only through `png` and the `include_gif` proc macro, both host-side.

So the brief's "5 to 10 KB of decoder and probably eats its own gain" is
directionally right but understates it: the failure is RAM, not code size, and
it is absolute.

### 2.3 Finding a page: the crux

Two ways to know where page 412 begins.

**Precomputed index.** A host program breaks the book at build time and emits an
array of byte offsets. At roughly 300 characters per page (section 3 derives
this) a 265 KB book is about 870 pages, so `[u32; 871]` is **3484 bytes**, or
2613 with 24-bit offsets. Random access is O(1). The total page count is free.
The table costs about 1.3 percent of the text.

The real cost lies elsewhere. The host program must reproduce NBGL's
line-breaking exactly, and NBGL's line-breaking is not readable: `nbgl_fonts.c`
lives in the OS and the SDK clone is shallow. The width tables can be decoded
out of `libnbgl_shared_screenshots_flex.a` (offsets and the bitfield layout are
in section 1.3), so character widths can be exact. The break *rule* cannot be,
and the unknowns are exactly the ones that bite: what happens to a trailing
space at a break, whether a `'\n'` landing at a wrap boundary consumes a line,
what happens when a single token exceeds 416 px with `wrapping = true`. A
one-line disagreement is a silent screen overrun (section 8.2), and it is silent
in both Speculos and on hardware.

Worse, the fonts belong to the OS. An OS update that changes a single advance
width repaginates the entire book and invalidates every offset in the binary,
with no signal.

**Runtime pagination with device-computed anchors.** The device walks the book
once with `nbgl_getTextMaxLenInNbLines`, the same function the renderer's
geometry is derived from, recording a byte offset every `stride` pages into a
small NVM array. Reaching page N is a binary search over the anchors followed by
at most `stride - 1` forward calls. Forward paging is one call. Backward paging
is a walk from the previous anchor.

Cost in flash: zero. Cost in NVM: 32 anchors of 4 bytes plus a stride and a
fingerprint, 140 bytes (section 5). Cost in time: one walk of about 870 calls at
first open, and up to 31 calls on a jump or a resume.

Drift is impossible by construction, because the thing that computes the breaks
and the thing that renders them are the same OS function reading the same bytes.
An OS font change is handled by storing a fingerprint of the metrics
(`nbgl_getFontLineHeight`, and the measured width of a probe string) alongside
the anchors and re-walking when it changes.

**Recommendation: runtime pagination with cached anchors.** It wins on flash, on
correctness, and on build-pipeline complexity (no font table to decode at build
time, which was the shakiest part of that pipeline). It concentrates risk in one
undocumented OS function, and that risk is cheap to retire: step 1 of section 9
is a probe APDU that calls `nbgl_getTextMaxLenInNbLines` on real book bytes and
returns the answer. If it misbehaves, fall back to a build-time index with a
conservative line budget, and section 8.7 says how to make that safe.

The one thing the runtime scheme cannot give for free is the total page count
before the walk. Show "page 412" without a denominator until the walk finishes,
and run the walk once at first open behind a "Preparing" screen, or on the first
page turn. Measure the walk: 870 trampoline calls each scanning about 300 bytes
is well under a second by any reasonable estimate, but that is an estimate and
the settling measurement is a timed probe on Speculos and on a real Flex.

### 2.4 Font size changes everything, so store an offset

The reading position is a **byte offset into the book**, never a page number.

Page numbers are a function of (text, font, width, line budget). An offset is a
function of the text alone. Changing font size, re-walking after an OS update,
and handing the position to another device all become the same operation:
translate the offset into whatever pagination is current by binary-searching the
anchors and walking forward. The page number is derived and displayed, never
stored as truth.

Switching size re-walks and re-anchors. The offset is preserved exactly; the
page number changes and the top of the screen lands on the same word.

## 3. The reading screen

### 3.1 Layout

```
y=0                       +--------------------------------------+
                          |  <     412 / 870              [list] |   header, 96
y=96   ------------------ +--------------------------------------+
                          |                                      |
                          |  In my younger and more vulnerable   |
                          |  years my father gave me some        |
                          |  advice that I've been turning over   |
       body, 408 px       |  in my mind ever since.              |
                          |                                      |
                          |                                      |
y=504  ------------------ +-------------------+------------------+
                          |         <         |         >        |   footer, 96
y=600                     +-------------------+------------------+
```

Built the way every other screen in this app is built:

- `nbgl_layoutAddHeader` with `HEADER_EXTENDED_BACK` (back key, centred text,
  touchable key on the right). Height is `TOUCHABLE_HEADER_BAR_HEIGHT`, a fixed
  96, unlike `HEADER_TITLE` whose height follows its text. Back key returns to
  the library; the centred text is the position indicator; the right key opens
  the table of contents.
- `nbgl_layoutAddText(handle, ptr::null(), page)` for the body. The main-text
  slot is documented optional; passing NULL puts the page in the sub-text slot,
  which NBGL renders in `SMALL_REGULAR_FONT` = `BAGL_FONT_INTER_REGULAR_28px`
  with `wrapping = true`.
- `nbgl_layoutAddSplitFooter(handle, "<", TOKEN_PREV, ">", TOKEN_NEXT, 0)`.

`nbgl_layoutAddPageIndicator` does not exist; the search for it turns up
`nbgl_layoutAddProgressIndicator`, which is marked `@deprecated` and whose
header type `HEADER_BACK_AND_PROGRESS` is documented "only on Stax". The Flex
mechanism is `nbgl_layoutNavigationBar_t.withPageIndicator`, whose header
comment says "this widget is incompatible with a footer". Since the footer is
the page-turn affordance, the indicator goes in the header as plain centred
text. Format it as `412 / 870`, or `412` alone until the walk has produced a
denominator.

### 3.2 Lines per page

`nbgl_layoutAddText` goes through `addListItem`, which sets the container to

```c
container->obj.area.height
    = LIST_ITEM_MIN_TEXT_HEIGHT + 2 * LIST_ITEM_PRE_HEADING;   // 40 + 52 = 92
```

and then sets the text area's own height to `MAX(40, nbgl_getTextHeightInWidth(...))`.
Reading the two together, the padding above and below is 52 px and the text gets
what is left. So the usable text height is 408 - 52 = **356 px**, giving

- `INTER_REGULAR_28px`, line height 36: `floor(356 / 36)` = **9 lines**
- `INTER_MEDIUM_36px`, line height 40: `floor(356 / 40)` = **8 lines**

These are derived from constants, not measured on a screen. The measurement that
settles them is step 2 of section 9: draw a page of 12 known lines and read the
`y` coordinates out of Speculos `/events`.

Recovering the 52 px is possible: build the body as a raw `nbgl_text_area_t`
through `nbgl_objPoolGet` and place it at y=96 with height 408, which buys one
more line at the small font. That path is compiled out of production today
(`#[cfg(feature = "artprobe")]` on `struct Screen`) and the module's own comment
records that its draw "never paints" without a specific sequence. One extra line
out of nine is not worth taking an unproven draw path into the shipping binary
in the first build. Revisit if the page turns out to feel thin.

### 3.3 Characters per line, and the page count

Advance widths are exact and kerning is zero, so a line's width is the sum of
its characters. What is not exact is the frequency mix of English.

The SDK's own constant is 16 px average, giving `416 / 16` = **26 characters per
line**. That equals the width of `n` and is clearly a conservative round number:
`i`, `l`, `t`, `f`, `r`, `s` and the space at 8 px are all far narrower, and
they are most of English. A realistic average lands somewhere between 12 and 14
px, giving 30 to 34 characters.

Taking 9 lines and a 26-to-34 character band:

| chars/line | chars/page | pages for 265 KB |
|---|---|---|
| 26 | 234 | 1130 |
| 30 | 270 | 980 |
| 34 | 306 | 866 |

**Expect 850 to 1150 pages at the small font.** At the large font (8 lines, and
glyphs about 37 percent wider, so roughly 22 to 25 characters) expect 1300 to
1600. These are estimates. The measurement that settles them is the first
on-device walk, which produces the exact number as a side effect, and it should
be recorded here once it exists.

A Flex page is about a fifth of a paperback page. Gatsby's 180 paperback pages
becoming 900 device pages is the correct order of magnitude and not a sign that
something is wrong.

### 3.4 Two font sizes

The SDK offers three faces at two sizes on Flex, and the second size is Inter
Medium 36px: heavier weight, line height 40 against 36, and glyphs roughly 37
percent wider. The line count barely moves (9 to 8) but the character count per
page drops by about a third. It is a genuine large-print mode and worth
shipping.

It is not worth a per-gesture toggle. Put it on the table of contents page as a
single row that flips between "Text size: normal" and "Text size: large". A
toggle costs a re-walk (section 2.4) and an NVM write, and neither belongs on a
gesture a thumb makes by accident while turning pages.

If flash gets tight, dropping the large size saves the toggle and nothing else,
because the anchors are recomputed rather than shipped.

### 3.5 Refresh

The app has never called `nbgl_refreshSpecial`. `Layout::draw()` calls plain
`nbgl_refresh()`. The modes available (`nbgl_types.h:333-345`):

```c
FULL_COLOR_REFRESH,          ///< to be used for normal refresh
FULL_COLOR_PARTIAL_REFRESH,  ///< to be used for small partial refresh
FULL_COLOR_CLEAN_REFRESH,    ///< lock screen display (cleaner but longer)
BLACK_AND_WHITE_REFRESH,     ///< pure B&W area, when contrast is important
BLACK_AND_WHITE_FAST_REFRESH,///< pure B&W area, when contrast is not priority
INIT_REFRESH,                ///< fully clear the screen in white
```

The word "ghost" does not appear anywhere in `lib_nbgl` or `include/`. There is
no documented ghosting characterisation to design against.

The plan: `BLACK_AND_WHITE_FAST_REFRESH` for a page turn, since the page is pure
black text on white and speed is what a page turn needs, and
`FULL_COLOR_CLEAN_REFRESH` every twelfth turn and on every entry into the
reader. Twelve is a guess; the settling measurement is a human looking at a real
Flex, because Speculos renders a clean framebuffer and will never show ghosting.

Do not use `POST_REFRESH_FORCE_POWER_ON_WITH_PIPELINE`. Its documented
constraint is that "successive draws & refreshes areas must not overlap", and
consecutive pages of a book overlap completely.

## 4. The build pipeline

Three stages, all host-side, all run by `build.rs`.

### 4.1 Source

Project Gutenberg's Great Gatsby, UTF-8. Checked into the repo under
`device-app/book/gatsby.txt` so the build is reproducible offline and so a diff
shows exactly what the device will say. Gutenberg's licence header and footer
are stripped by hand, once, and the stripped file is what is committed.

### 4.2 Preparation

A small Rust build-dependency, or a Python script invoked by `build.rs`, does:

1. **Transliterate to ASCII.** This is mandatory, not cosmetic: the fonts cover
   0x20..0x7E and nothing else (section 1.3). Gatsby's Gutenberg text is full of
   characters that do not exist on this device.

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

3. **Assert.** Every byte is in `0x20..=0x7E` or is `\n`. Fail the build
   otherwise, loudly, naming the offending byte and its offset. This assertion
   is the whole defence against section 8.5 and it costs three lines.

4. **Assert no oversized token.** Split on whitespace, sum advance widths from a
   width table checked into the repo, and fail if any token exceeds 416 px. The
   width table is 95 bytes per font and is decoded once, by hand, out of
   `libnbgl_shared_screenshots_flex.a` (section 1.3 gives the symbol offsets and
   the bitfield layout: `width = (word2 >> 1) & 0x3F`). Checking it in rather
   than reading the archive at build time keeps WSL paths out of the build.

5. **Find the chapters.** Gatsby has nine, marked by a line that is a roman
   numeral alone. Emit `CHAPTERS: [u32; 9]` of byte offsets plus a 9-entry table
   of one-line titles. About 60 bytes in `.rodata`, which is small enough not to
   matter there.

### 4.3 Emission

`build.rs` writes into `OUT_DIR`, not into the source tree. The existing glyph
generation writes into `glyphs/` in-tree, which is fine for three small PNGs and
wrong for a 270 KB blob.

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
...))` shape for install parameters (`vendor/ledger_device_sdk/src/app_info.rs:23-24`),
so the pattern is proven in this build.

`BOOK_HASH` is the SHA-256 of `book.bin`, exposed by a development APDU. It is
how a host test proves it is reasoning about the same book the device holds, and
how the anchor fingerprint (section 5) detects that the book itself changed.

Add `println!("cargo:rerun-if-changed=book/gatsby.txt")` to `build.rs`.
`build.rs:119` currently emits `rerun-if-changed=script.ld` for a file that does
not exist in this repo; leave that alone, it belongs to the flash-diet work.

## 5. The NVM state

### 5.1 Nothing goes into `PresseNvm`

`PresseNvm` is 1176 bytes, is wrapped in `AtomicStorage` (two copies in flash),
and is copied by value onto the stack on every screen draw. Adding the reading
position to it would mean:

- every page turn rewrites 1176 bytes, which is three flash pages erased per
  turn instead of one;
- every page turn rewrites the bearer key, the certificates and the provenance
  chain, which is the one region in this app where a bad write is unrecoverable;
- every screen draw carries the reading state on the stack for no reason.

The reading state gets its own `.nvm_data` object, following the precedent of
`ART_MASTER` / `ART_PRESSING` (`state.rs:235-239`), which exist for the same
reason (`state.rs:193-194`: "Kept out of `PresseNvm` because that struct is
copied through the stack on every read").

The atomicity that `PresseNvm` provides is not needed here. A power cut that
leaves a stale reading position next to a cleared pressing costs a reader
nothing: the reader works without a copy anyway (section 6), so a leftover
position simply shows the next person a page they did not choose. That is a mild
privacy wrinkle, addressed by clearing the position in the same handler that
clears the pressing, and it is not a correctness problem. Nothing about
double-spending depends on it.

### 5.2 The struct

```rust
/// Reading state. Its own `.nvm_data` object so a page turn never rewrites the
/// ownership struct, and so the per-turn flash cost is one page and not three.
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
is 288 bytes, and page-aligned in `.nvm_data` that is **512 bytes of
`data_size`**, one flash page.

512 bytes against a 400 KiB region that is 20% used is nothing to pay. Boot-check
it with `scripts/boottest.sh` at 3/3 the moment it lands, on its own, before any
reader code exists, for a different reason: a new object in `.nvm_data` moves the
section's layout, and every offset in it has to stay inside what the emulator
maps (section 8.3). A struct that lands past that boundary reads as zeros with no
error anywhere, which is exactly how a resume feature would look correct and
silently forget the page.

One anchor set, not two. Keeping anchors for both font sizes would cost a second
512-byte page; recomputing on a size change costs a walk the user waits through
once. The walk is the cheaper thing to spend.

### 5.3 How often the position is written

Writing an 8-byte field and writing a 512-byte page cost the same on flash,
because `nvm_write` erases a page either way. So the question is only how many
page erases a reader causes.

A full read of Gatsby is about 900 page turns. If the flash endurance is 100,000
erase cycles per page, that is about 110 complete readings before the page wears
out. Nobody reads one novel 110 times on one device, so **write on every page
turn**. If the real figure is 10,000, it is 11 complete readings, which is not
enough.

The endurance figure is not in the SDK and is not in this tree. The measurement
that settles it is Ledger's or ST's specification for the secure element's
flash. Until somebody has that number in hand, treat per-turn writing as
provisional.

The fallback, if the number comes back low, is ready and cheap: keep the live
position in a `static AtomicU32` and commit it to NVM on leaving the reader, on
Quit, before serving any APDU, and every sixteenth turn as a crash floor. That
weakens "survives power loss" to "survives power loss to within fifteen pages",
which is a real regression and should be taken only if forced.

Two rules that apply either way:

- skip the write when the offset has not changed (a tap on `<` at page 1);
- never call `Store::get()` on the reader path, because it is not a pure read
  (`state.rs:303-309` writes NVM when `initialized == 0`).

## 6. Ownership, or the lack of it

The text is free and the object is scarce. **The reader works fully without a
copy in NVM.** Anyone who installs the app reads the whole book, remembers their
page, changes the font size, and uses the table of contents. Nothing about
reading is gated.

The library grows a permanent row for the book, because the book lives in the
app and not in NVM:

| state | row 1 | row 2 | row 3 |
|---|---|---|---|
| nothing held | The Great Gatsby / *Reading copy* | | |
| holds a pressing | The Great Gatsby / *No. 4 of 50* | Pressing 4/50 (card) | |
| holds a master | The Great Gatsby / *Reading copy* | Master, 46 left (card) | |
| both | The Great Gatsby / *No. 4 of 50* | Master | Pressing |

Three rows at 92 px is 276 px, inside the 408 the page has. Four would be exactly
408 and there is no fifth. That bound is already enforced elsewhere in this app
by a typed array length (`BACK_ROWS: usize = 4`, `collection.rs:179`, "so a
fifth one does not compile") and the library's row set should get the same
treatment.

Tapping the book row opens the reader. Tapping a record row opens the existing
card, unchanged.

What an unowned install hides: the record card entirely, the number, the Edition
ID, the Device ID's History sub-page, the give flow, and the challenge response.
`INS_CHALLENGE` already fails closed on a device holding no copy and stays that
way. The empty-library copy ("No records yet" / "You gave your copy away")
survives as the *subtitle of nothing*, since the library is no longer empty; move
it onto the book row's subtitle so the distinction between "nothing yet" and
"nothing any more" is not lost.

What an owned install adds to the reader: the number in the header of the table
of contents page, and nothing on the reading page itself. A page of a novel is
not a place to put a serial number.

## 7. The transfer

The give machinery works, took a day to get right, and has a security fix in it
from today. The change here is deliberately the smallest one that carries the
position.

### 7.1 What travels

The reading offset, four bytes, and nothing else. The page number stays behind
because it is derived; the font size stays behind because it is the new
reader's own preference; the anchors stay behind because they are a cache.

### 7.2 Where it rides

In **phase 1**, the part of the ceremony that is free to abandon because nothing
on either device has changed yet. Alongside `GIVE_CHAIN` / `TAKE_CHAIN`:

```
0x7E GIVE_BOOKMARK  (giver, paired)  -> pos(4) || mac(32)                 [36]
0x7F TAKE_BOOKMARK  (taker, paired)  data = pos(4) || mac(32)  -> ok      [36]
```

`TAKE_BOOKMARK` stages the value exactly as `TAKE_CHAIN` and `TAKE_PRESSING`
stage theirs, and it lands in NVM with the same `Store::put` at `TAKE_ACCEPT`.
Because the reader state is a separate object, that means one extra write beside
the atomic one, and section 5.1 argues why that is acceptable.

Nothing in phase 2 changes. `GIVE_OFFER`'s reply stays `sealed bearerkey(32) ||
mac(32)`, 64 bytes; `TAKE_ACCEPT` still consumes it verbatim; `GIVE_CANCEL`
still refuses at `committed == 2`; the three-state byte is untouched. **Do not
extend the sealed payload.** Its length is load-bearing on both sides, the pad
is keyed to the frame's sequence number, and dispatching on a sealed payload's
length is a parsing surface nobody needs.

Both instructions are MACed like every other ceremony frame
(`HMAC(K, [ins, seq] || payload)`), so a hostile relay cannot rewrite the page
number. A page number is not worth attacking, and it is cheaper to MAC it than
to explain why it is the one frame that is not.

If either side does not support the pair, the ceremony proceeds without it: the
giver's `GIVE_BOOKMARK` returns `InsNotSupported` and the taker starts at page
one. A copy without a bookmark is a book nobody has opened, which is a coherent
thing for it to be.

`CEREMONIE-VIDEO.md` and `docs/protocol.md` both carry the APDU map and both
must gain these two lines, or the live cut fails.

### 7.3 What the giver loses

`clear_pressing()` gains nothing, because the reading state is not in
`PresseNvm`. The `GIVE_FINISH` handler additionally resets `ReaderNvm.pos` to 0
and `page` to 1, in its own write. The giver keeps the book and loses their
place in it, which is exactly what happens when you hand someone a paperback.

### 7.4 What the card shows

The record card's Back page is full: `BACK_ROWS = 4` is a typed array length,
four rows at 92 px is 368 px, and a fifth would land under the footer at y=504.
**A reading row cannot go on the Back page.** Put it on the front of the card
instead, under the number:

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

Bound the string. `page` is a `u16` and `pages` is a `u16`, so the row cannot
grow with device state, which is the shape of bug AGENTS.md warns about.

## 8. What will go wrong

### 8.1 The book is in `.text`, and nothing has ever read `.text`

**How it manifests.** Two separate failures hide here.

The placement failure: a `static` declared without `#[link_section = ".text"]`
lands in `.rodata` and `data_size` becomes about 286 KB. Nothing crashes over
it, since both sections spend the same 400 KiB, and that is what makes it easy
to miss: the build works, the app boots, and the one figure that would have
flagged an accidental `.rodata` or `.nvm_data` growth from then on is buried
under the book.

The access failure: **nothing in this app has ever dereferenced a byte of
`.text` as data.** The ballast of section 1.1 would show that a `[u8; N]` there
links and boots, and it would still show nothing about reading it. The target is
`ropi-rwpi` (`devices/flex/flex.json`), `.text` is declared in `link.ld` as
needing no relocations, and the SDK's `pic()` relocates pointers in
`_nvram_start.._nvram_end` which covers `.text`. That reasoning says it works.
Reasoning is not a measurement, and the app already has one documented case
where a runtime pointer faulted under PIC relocation (`build.rs:49-51`, on
glyphs).

**How to detect it early.** The first experiment, before any reader code:
`#[link_section = ".text"] static PROBE: [u8; 4096]` filled with a known
pattern, plus a development APDU that returns `PROBE[i]` for a caller-supplied
`i`. If it returns the pattern, the whole design stands. If it faults, the book
falls back to `.rodata`, which costs the `data_size` signal and nothing else in
flash; the project survives it.

**What to do about it.** Run that probe on day one. Read `data_size` off every
single build (`cargo ledger build flex | grep -oE "data_size: [0-9]+"`) and
treat any movement as a placement bug until proven otherwise.

### 8.2 The silent screen overrun

**How it manifests.** A page renders one line more than fits. NBGL draws it
anyway, under the footer, showing only the tops of the glyphs. Past y=600
Speculos faults the draw. The text is still in `/events` either way, so a test
that reads screen text passes.

**How to detect it early.** `assert_page_fits(dev)` already exists in
`tests/presse_client.py:147-159` and asserts that nothing except a known footer
label has `y + h > 504`. Call it after every page turn in the reader tests, and
over a sweep: page 1, every 97th page, the last page, and both font sizes.

**What to do about it.** Derive the line budget from a measurement (section 3.2)
rather than from arithmetic, then never let it be a function of anything the
text does. If runtime pagination is in use the budget is an input to
`nbgl_getTextMaxLenInNbLines` and cannot be exceeded by construction, which is
the strongest argument for that scheme.

### 8.3 The emulator's notch, and the 400 KiB edge

**How it manifests.** The app installs, panics before its first APDU
(`exiting_panic` -> `exit_app(0)`), and answers nothing. Under Speculos it has
one known cause, in the emulator itself: stock Speculos sizes the app's mapping
from the `PT_LOAD` holding `.text` plus one page, `.nvm_data` is a separate
`PT_LOAD`, and only 4096 to 7680 bytes of it arrive. At a load size that is a
multiple of 4096 exactly one page arrives, both `AtomicStorage` validity flags
fall outside it, and `which()` panics. Physical Flexes are unaffected.
`docs/speculos-nvm-loading.md` has the measurement and the fix. Every "boot
window" number this repo once carried, 74032..76080 and the rest, was this.

The other hazard here is the plain one: the app image cannot exceed 400 KiB, and
it stands at 20% of that today.

**How to detect it early.** Three instruments, already in the tree.
`scripts/patch-speculos.sh` fixes the emulator in `~/venv-ledger` and `env.sh`
re-applies it on every source, so the notch stops firing at all; `pip install -U
speculos` puts the bug back. `presse_check_load_size` in `scripts/env.sh` fails
the build when `_erodata - _text` is a multiple of 4096, so a binary that would
die on somebody else's stock emulator never leaves quietly (`ALLOW_NVM_NOTCH=1`
overrides). `scripts/boottest.sh` launches Speculos, polls `b501000000`
(GET_INFO) once a second up to a deadline, and distinguishes `ok` / `panic` /
`slow`, with `slow` retried rather than counted; a point is only usable at 3/3.

**What to do about it.** The reader multiplies the image by roughly four, from
80 KB to something near 350 KB, which is 85% of the region. Nothing in this repo
has ever run an app that large, and the last 15% has to hold every later
addition. So the sweep stays the first step, now gating on the region's edge:
write the ballast of section 1.1, walk `TEXT_BYTES` up toward 320 KB,
boot-check 3/3 at each point, and read the load size at each point too, since a
sweep in 512-byte steps crosses the notch every eighth point on a stock
emulator and would otherwise read as a band of failures again.

A book that does not fit is answered by packing (section 2.2, 190 KB five-bit or
120 KB dictionary) before it is answered by a shorter work.

### 8.4 RAM

**How it manifests.** Heap exhaustion or a smashed `app_stack_canary`, both of
which surface as a panic and a silent exit.

**The budget.** SRAM is 36 KiB. `.bss` already runs to `END_STACK` (36864
bytes), covering the 8192-byte heap and everything else, and the practical
working figure is about 24 KB. A screen draw already puts 1176 bytes of
`PresseNvm` on the stack, and the record card composes a bitmap on the heap,
which is why `main.rs` drops the library before dispatching an APDU.

**What to do about it.** One `static mut PAGE_BUF: [u8; 1024]` in `.bss`,
zero-initialised (this target forbids `.data`, so every static must be
all-zero, per `library.rs:134-136`). No heap on the reader path, no `format!`
for the body, no `Vec`. The `CString` for the header and the footer labels are
small and can stay. Bound the page buffer with a compile-time check: at 9 lines
and a minimum advance of 7 px, a page cannot exceed 9 * 416/7 = 535 bytes, so
1024 is comfortable, and `nbgl_getTextMaxLenInNbLines` is called with a `maxLen`
the buffer can hold.

Adding 1024 bytes of `.bss` takes 1024 bytes from the stack, since `.bss` runs
to `END_STACK`. `STACK_MIN_SIZE` is 1500 and the link asserts against it, so the
link will complain if it gets desperate, but it will not complain about merely
tight.

### 8.5 Text encoding

**How it manifests.** Bytes above 0x7E index past `last_char` in the font's
95-entry character table. Best case a wrong glyph, worst case a read off the end
of the table and a faulted draw. Gatsby's Gutenberg text is full of them: em
dashes, curly quotes, ellipses.

**How to detect it early.** The build-time assertion in section 4.2 step 3. It
is three lines and it makes this class of bug impossible.

**What to do about it.** Transliterate in the pipeline. Do not attempt a unicode
font: there is no Inter unicode data anywhere in the tree, `HAVE_LANGUAGE_PACK`
is absent from the Flex defines, and four of the five unicode accessors have no
trampoline so an app calling them will not link.

### 8.6 Line breaking and long tokens

**How it manifests.** NBGL breaks on spaces when `wrapping = true` and has no
hyphenation at all. What it does when a single token is wider than 416 px is not
documented and the source is not in the tree. A likely outcome is an overflowing
line, which is 8.2 again.

The realistic source of such a token in Gatsby is a naive em dash replacement:
`word—word` becoming `word--word` is one token of eighteen characters, and at
Inter Medium 36px eighteen average characters is around 400 px, close enough to
416 to matter.

**How to detect it early.** The build-time token width assertion in section 4.2
step 4, run against both fonts.

**What to do about it.** Put spaces around the `--`. If a genuine long token
appears (a URL, a long compound), break it in the source text.

### 8.7 Pagination drift

**How it manifests.** The index says page 412 starts at offset X; the renderer
breaks the text differently and page 412 needs ten lines where nine fit. Silent
overrun, or a page that ends mid-sentence with a gap.

This risk exists only under build-time pagination, which is why section 2.3
recommends against it. If it is adopted anyway:

**How to detect it early.** A development APDU `READER_LINES(page)` that calls
`nbgl_getTextNbLinesInWidth` on that page's slice and returns the count, plus a
host script that sweeps every page and asserts `<= budget`. That converts an
open-ended risk into a one-time check that can run in CI.

**What to do about it.** Generate the index with a budget one line below what
the screen renders, so a one-line disagreement still fits. It costs about 11
percent more pages and about 380 bytes of index.

Under runtime pagination the equivalent risk is the anchors going stale after an
OS update changes a font. The fingerprint in `ReaderNvm` catches it: fold
`nbgl_getFontLineHeight(font)` and `nbgl_getSingleLineTextWidth(font, probe)`
for a fixed probe string into the stored value, and re-walk when it moves.

### 8.8 E-ink ghosting

**How it manifests.** Faint residue of the previous page under the current one,
accumulating over turns until the page is grey.

**How to detect it early.** Only on hardware. Speculos renders a clean
framebuffer and will never show it. Any refresh-mode decision validated only on
Speculos is unvalidated.

**What to do about it.** Section 3.5. Note the app has never called
`nbgl_refreshSpecial` at all, so the first call is itself a change worth
boot-checking.

### 8.9 The last page, and both ends

**How it manifests.** `>` on the last page walks the index off its end and
renders whatever follows the book in flash, or wraps to page 1, or panics.
`<` on page 1 does the mirror.

**What to do about it.** The reader's page-turn handler is the one place in this
design where fail-closed matters. Both bounds are explicit:

```rust
// Not "wrap", not "clamp silently": the button that cannot act does nothing,
// and the page it would have gone to does not exist.
if next_offset >= BOOK_LEN as u32 { return; }
if page == 1 { return; }
```

Consider greying the inert half of the footer. `nbgl_layoutBar_t` has an
`inactive` flag; `nbgl_layoutAddSplitFooter` does not obviously expose one, so
the cheap version is to render the label as `.` or an empty string at the ends.

The last page is also short, which is correct and needs no special case, and the
walk must count it: the anchor walk terminates when
`nbgl_getTextMaxLenInNbLines` returns a `len` that reaches the terminating NUL.

### 8.10 The reader and the APDU loop

**How it manifests.** The reader is on screen. The host sends anything at all,
`run_event_loop` returns `Exit::Apdu`, and the reader tears itself down and
lands back on the library, losing the page it was on. The ceremony scripts poll
`GET_INFO`, so this fires constantly during any bench session.

**What to do about it.** `warrants_library_redraw` (`main.rs:233-252`) already
carries the hand-maintained list of which instructions invalidate the screen,
with the rule "repaint after a UI-gated command, never after a pure data-plane
one". The reader needs the mirror of it: a `evicts_reader(ins)` predicate that
is true for the UI-gated ceremony instructions and false for `GET_INFO`,
`GET_BUNDLE`, `GET_ART`, `CHALLENGE` and the rest of the data plane. On a
non-evicting command, serve it and redraw the same page.

A ceremony must be able to take the screen, because a ceremony has a human on
both ends and a book does not.

### 8.11 The first-paint NVM write

**How it manifests.** Nothing, most of the time. `Store::get()` generates the
device keypair and writes NVM when `initialized == 0`
(`state.rs:303-309`), so a function everything treats as a read is not one. A
reader that calls it per page turn writes flash on a virgin device, once, and
then costs a 1176-byte stack copy on every turn forever.

**What to do about it.** The reader never calls `Store::get()`. It reads
`ReaderNvm` directly. The one place ownership matters to the reader is the
number shown on the contents page, and that is read once on entry.

### 8.12 The two anchors and the walk

**How it manifests.** The walk runs at first open and blocks the UI for its
duration with no screen up, which looks like a hang.

**What to do about it.** Draw a "Preparing the book" screen before starting,
with no ticker and no animation, and refresh once. Measure the walk on Speculos
and on hardware and record the number here. If it exceeds a second or two,
chunk it: walk 64 pages per iteration, returning to the event loop between
chunks so APDUs are still served.

## 9. What to build, in order

Each step is testable on its own and each one can fail without wasting the next.

**0. Prove the flash.** `#[link_section = ".text"] static PROBE: [u8; 4096]`
with a known pattern, plus a `factprobe`-style development APDU returning
`PROBE[i]`. Boot-check 3/3 on a patched emulator. Read `text` and `data_size`
off the build and confirm `data_size` stayed at 18432. *Passes when the APDU
returns the pattern and `data_size` is unchanged.* This gates everything; if it
fails, stop.

**1. Prove the sweep.** Write `device-app/src/ballast.rs` and its
`ballast_text` feature (section 1.1: neither exists yet) plus a sweep script,
and walk `TEXT_BYTES` from 5120 up to 320 KB in whatever steps patience allows,
boot-checked 3/3 and load-size-logged at each point. *Passes when a 320 KB
`text` boots.* If it does not, the book must shrink or be packed, and the size
question comes back before any code is written.

**2. Prove the primitive.** A development APDU that calls
`nbgl_getTextMaxLenInNbLines(INTER_REGULAR_28px, &PROBE_TEXT[off], 416, 9, &len,
true)` on a chunk of real English and returns `len`. *Passes when successive
calls tile the text without gaps or overlaps and each slice measures at most 9
lines by `nbgl_getTextNbLinesInWidth`.* This is where the runtime-pagination
decision is confirmed or reversed.

**3. The pipeline.** `book/gatsby.txt` in, `book.bin` plus `book_meta.rs` out,
with the ASCII assertion and the token-width assertion. *Passes when the build
succeeds, the assertions are proven to fire on a deliberately corrupted input,
and `BOOK_LEN` is a real number this document can be updated with.*

**4. One page.** Header with `HEADER_EXTENDED_BACK`, body via
`nbgl_layoutAddText(NULL, page)`, split footer, page 1 only, no navigation, no
NVM. *Passes when Speculos shows the first paragraph and `assert_page_fits`
holds.* This is also where the line budget stops being derived and becomes
measured: read the `y` coordinates out of `/events`.

**5. Turning.** `TOKEN_NEXT` / `TOKEN_PREV` on the footer halves, position in a
RAM `static`, forward via one `nbgl_getTextMaxLenInNbLines` call, backward via a
re-walk from page 1 (slow and correct; anchors come in step 7). Both ends inert.
*Passes when 40 pages forward and 40 back land on the same text, and
`assert_page_fits` holds at each.*

**6. Persistence.** `ReaderNvm` as its own `.nvm_data` object, offset written on
every turn. Boot-check 3/3 the moment the struct lands, before any code uses it.
*Passes when Speculos is killed mid-book and relaunches on the same page.*

**7. Anchors and the count.** The walk, the fingerprint, the binary search, the
"Preparing" screen, `page / pages` in the header. *Passes when a jump to a
random page is instant, the count matches a host-side count of the same text,
and a deliberately corrupted fingerprint triggers a re-walk.*

**8. Contents.** Three pages of four chapter rows, reached by the header's right
key, footer split "Back" / "More". A jump sets `pos` to the chapter offset and
recomputes the page. *Passes when every chapter lands on its first line and
`assert_page_fits` holds on all three contents pages.*

**9. Refresh discipline.** `BLACK_AND_WHITE_FAST_REFRESH` per turn,
`FULL_COLOR_CLEAN_REFRESH` every twelfth and on entry. Boot-check. *Passes on a
real Flex, by eye, over fifty consecutive turns.* Speculos cannot judge this.

**10. The library.** The book row, the unowned and owned subtitles, the row
bound as a typed array. `evicts_reader(ins)`. *Passes when the existing library
and card tests still pass and a `GET_INFO` poll no longer evicts the reader.*

**11. Large text.** The second font on the contents page, re-walk on switch,
offset preserved. *Passes when switching size mid-chapter lands on the same
sentence, and `assert_page_fits` holds at the large font, where the margin is
thinner.*

**12. The bookmark travels.** `GIVE_BOOKMARK` / `TAKE_BOOKMARK`, `docs/protocol.md`
and `CEREMONIE-VIDEO.md` updated in the same commit. *Passes when the dual-Speculos
give test shows the taker resuming on the giver's page, and when an old giver
that answers `InsNotSupported` still completes a give with the taker starting at
page one.*

## 10. Open questions, and what would close them

| Question | Closed by |
|---|---|
| Does a `static` in `.text` read correctly under ROPI? | Step 0. |
| Does a 320 KB `text` boot? | Step 1, the ballast sweep. |
| Does packing become mandatory? | Step 1 against step 3: the measured book length against the measured headroom. The 400 KiB region is the only ceiling, and 320 KiB of it is free today. |
| Exact break semantics of `nbgl_getTextMaxLenInNbLines` | Step 2. The source is in the OS and cannot be read. |
| Real usable body height with `nbgl_layoutAddText` | Step 4, `y` coordinates from Speculos `/events`. Derived here as 356 px. |
| Characters per line for real English in Inter Regular 28px | Step 7's walk, as a by-product. Bounded here at 26 to 34. |
| The book's exact byte length | Step 3. Estimated here at 250 to 280 KB. |
| Flash endurance per page | Ledger or ST documentation. Not in this tree. Decides section 5.3. |
| Whether a swipe's direction reaches the app | `nbgl_layoutAddSwipe` takes one token for all directions and the direction arrives in the callback's `index`, which `layout_touch_callback` currently discards. Worth one experiment if swipe is wanted beside the footer. |
| Ghosting at what interval | Human eyes on hardware, step 9. |
