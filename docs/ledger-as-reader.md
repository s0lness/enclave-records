# Ledger as an e-ink reader

The case for putting a book inside the app: what the idea is, why it belongs to
this project, and what has been measured. For how to build it (layout,
pagination, NVM, build pipeline, failure modes, step order), see
[`docs/reader.md`](reader.md).

Nothing described here has run on a physical device. Every number below is
labelled **measured**, **inferred** or **untested**, and every measured figure
names the file it came off so it can be re-derived.

## 1. What the device already is

A Ledger Flex is a 480x600 e-ink touchscreen with a secure element behind it
(measured: `nbgl_types.h`, Flex block, `SCREEN_WIDTH 480` / `SCREEN_HEIGHT
600`). The screen exists so a human can read a transaction and press a key. Every
app written for it uses the screen that way: an address, an amount, a confirm.

The hardware description of a Flex is a small e-ink reader with unforgeable
storage attached. Nobody has used it as one.

## 2. The proposal

One work, one app. The book is compiled into the binary as a plain byte array,
in the clear. Anyone can install the app and read the whole book, with no
copy, no key, no account.

A numbered copy is a separate object. It lives in NVM as a certificate and a
bearer key, it moves from one secure element to the next through the ceremony
this repo already implements, it verifies offline, and it is never in two places
at once.

That pair is how a first edition works next to a paperback. The text of Gatsby
is everywhere. A signed first edition is one object, and it has a location.

## 3. The object does not protect the text

The text ships in the clear on purpose. Encryption was considered and rejected:
a reader that decrypts is a reader that can fail to decrypt, and DRM subtracts
from what the object is worth.

So the app makes no claim about the text. The text is public. The scarcity sits
entirely in the numbered copy, which is the thing the secure element enforces
and the only thing it enforces. An installed app with no copy in NVM reads the
book from the first page to the last.

Everything else in this document rests on that split.

## 4. Why this project, specifically

Enclave presses numbered copies of an album. What a copy actually holds today is
a certificate chain and a 2 KB 1bpp cover square (measured: `state.rs:211-212`,
`ART_W = 128`, `ART_LEN = ART_W * ART_W / 8 = 2048`). The work itself lives
nowhere. The device proves you own copy 4 of 50 of something the device cannot
show you.

An object you own and cannot use is a token with a good story attached. A book
you actually read on the device closes that gap: the thing the certificate names
is the thing on the screen.

And the ceremony changes meaning without changing code. `give` moves a copy from
one device to another, once, with nothing left behind. Applied to a book, that
operation is lending. Ebooks removed lending, and nothing has given it back.

## 5. Does a book fit

All figures in this section were measured on 2026-07-31 against the Enclave
build in `device-app/target/flex/release/presse` and its linker scripts.

### 5.1 The region

```
FLASH (rx) : ORIGIN = 0xc0de0000, LENGTH = 400K
```

400 KiB, 409600 bytes, per app (measured:
`ledger_secure_sdk_sys-1.16.2/devices/flex/flex_layout.ld`, and its generated
copy under `device-app/target/flex/release/build/ledger_secure_sdk_sys-*/out/`).
The same 400 KiB appears on every modern Ledger target. This is a link-time
region; whether the OS would accept an image larger than it is **untested**.

Enclave occupies 82846 bytes of that region today, 20.2 percent (measured:
`_nvram_end - _text` off the ELF symbol table; the figure moves by a few hundred
bytes per commit, and `docs/reader.md` section 1.2 records 82334 for a slightly
earlier build).

### 5.2 The ceiling

A byte array was added in `#[link_section = ".text"]` and grown by bisection
(measured). The largest payload that links is **326986 bytes**. At 326987 the
link fails with

```
.nvm_data will not fit in region FLASH
```

The binding constraint is the NVM section being pushed past the end of the
region. There are 98 bytes of slack left at that ceiling.

The sizes follow an exact model, zero error at eight sampled points (measured):

```
load_size = 512 * ceil((payload + 74934) / 512)
text      = load_size - 208
the link fails once load_size > 401920
```

74934 is this build's own size and moves as the app changes; the 512 is the
flash page size and the 401920 is 409600 minus the current 7582-byte
`.nvm_data`, rounded down to a page.

`data_size` (`_envram_data - _nvram_data`) stays at **18432 across 256 KiB of
payload** (measured). A book declared as an ordinary `static` would land in
`.rodata` and inflate that figure one for one, because the Rust SDK places
`.rodata` after `_nvram_data`. Keeping it at 18432 preserves the one number that
signals an accidental section change.

### 5.3 The read path

A `static` in `#[link_section = ".text"]` **reads back correctly** at 64 KiB, at
256 KiB and at the 326986-byte ceiling, at fourteen offsets including the final
sixteen bytes, through two independent access paths, byte-identical to the ELF
and present verbatim in `presse.hex` (measured). The references emitted are
`R_ARM_REL32`, resolved at link time, so the position-independent base this
target uses does not break them.

All of that was read out of the ELF and the hex. **No physical device has
executed a single byte of it.** Hardware behaviour is inferred.

### 5.4 A novel

At 6 bytes per word, 326986 bytes is about **54,500 words** (inferred, from an
English average of roughly 4.7 letters plus a separator).

The Great Gatsby is about 47,000 words, roughly 280 KB. It fits, with about
45,000 bytes of slack, 14 percent of the ceiling (inferred).

### 5.5 A screen

The body font `INTER_REGULAR_28px` has `line_height` 36 (measured, decoded from
`libnbgl_shared_screenshots_flex.a`; the name understates it). The header rule
sits at y=96 and the footer rule at y=504, leaving 408 px, of which NBGL's list
item spends 56 px on padding, so 352 px of text: **9 lines** (derived from SDK
constants, see `docs/reader.md` section 3.2, and untested on a screen).

Nine lines is about 300 characters, roughly 52 words. Gatsby lands at **850 to
1150 screens** (inferred). A Flex screen holds about a fifth of a paperback page.

### 5.6 The cost of the reader itself

Pagination is an OS syscall that is already linked into this app:
`nbgl_getTextMaxLenInNbLines` (measured: it has a trampoline in
`nbgl_stubs.h`, syscall 0xb7). Given a font, a pointer, a pixel width and a line
budget, it returns how many bytes fit. Forward pagination is a byte cursor and
one call, with no index in the binary at all.

Its `maxNbLines` parameter takes the line budget as an input, which structurally
eliminates the silent screen overrun this project has already been bitten by:
a page cannot be given more lines than it asks for.

Built on the app's existing NBGL layout wrapper, the reading screen costs
roughly **300 to 450 bytes** (inferred).

### 5.7 Several books

A Flex has about 1.02 MiB free for apps once the OS is deducted (inferred, not
verified in this repo). At a full-size book app of around 330 KB, that is three
books on one device.

## 6. What would make this fail

**The text has to be mutilated to be shown.** The Flex fonts cover 0x20..0x7E
and nothing else, on three independent counts (measured): `first_char` /
`last_char` in the font metrics, `HAVE_LANGUAGE_PACK` absent from the Flex
defines, and four of the five unicode accessors having no trampoline, so an app
calling them does not link. Em dashes, curly quotes and ellipses cannot be
rendered at all. Every text must be transliterated to ASCII before it ships, and
Gatsby is full of exactly those characters. That is a real loss on a page, and
it is permanent.

**Nothing has run on hardware.** The link-time and ELF-level results above are
solid. Reading `.text` as data on a live secure element, e-ink ghosting over a
few hundred page turns, flash endurance under one NVM write per page turn: all
three are open, and only the first has a cheap probe.

**A maximum-size book leaves the app nowhere to grow.** At the ceiling there are
98 bytes of slack. A book near the limit freezes the rest of the app at its
current size, and any later feature has to come out of the text.

**Compression does not rescue a book that is too long.** Deflate needs a 32 KiB
sliding window; the entire SRAM region is 36 KiB and `.bss` already claims all
36864 bytes of it, heap and stack included, leaving a practical working budget
near 24 KB (measured). The window does not fit, and that is arithmetic.
Packing to 5 or 6 bits per character is possible and buys
roughly 25 to 30 percent (inferred, `docs/reader.md` section 2.2); a word
dictionary does better, around 55 percent, at the cost of a real build-time
vocabulary pass.

**One payload size in eight is unusable on a stock emulator.** When `load_size`
lands on a multiple of 4096, Speculos maps only one page of `.nvm_data` and the
app panics before its first APDU (measured, and written up in
`docs/speculos-nvm-loading.md`). Payload sizes move `load_size` in 512-byte
steps, so one point in eight hits it. This repo patches the emulator and fails
the build at those sizes on purpose; physical devices are unaffected. Anyone
sweeping sizes on an unpatched Speculos will read a band of failures that is not
theirs.

## 7. What is already known against what is not

| Claim | Status |
|---|---|
| 400 KiB per-app region | measured, link-time; OS acceptance of more untested |
| 326986-byte maximum payload | measured by bisection |
| The size model, zero error at eight points | measured |
| `data_size` unmoved at 18432 | measured over 256 KiB of payload |
| `.text` static reads back correctly | measured in the ELF and the hex; hardware inferred |
| Fonts are ASCII-only | measured, three independent counts |
| 9 lines per screen | derived from SDK constants, untested on a screen |
| Gatsby fits with 14 percent margin | inferred from a word-count estimate |
| 850 to 1150 screens | inferred |
| Reader costs 300 to 450 bytes | inferred |
| Three books per device | inferred, free-space figure unverified here |
| Anything at all on a physical Flex | untested |

## 8. Next

The engineering design is [`docs/reader.md`](reader.md): where the text sits, how
a page is found, what goes in NVM, how the bookmark travels with a given copy,
and a twelve-step build order whose first step is a probe that either confirms
the whole approach or kills it in an afternoon.
