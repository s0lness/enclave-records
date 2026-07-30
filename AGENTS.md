# presse

Silicon-enforced finite editions, demoed on two Ledger Flex devices. An artist device
"cuts a master" of an album (edition size fixed in the secure element), then "presses"
numbered copies onto other devices through a ceremony relayed by an untrusted laptop.
Offline verification via certificate chain + challenge-response. Lab for the cartridge
thesis (physical editions of digital works, no chain, no server).

## Layout

- `device-app/` - Rust Ledger app (fork of LedgerHQ/app-boilerplate-rust), targets Flex
  (+ Stax/Nano S+ builds for free). `#![no_std]`, NBGL UI. Has its own CLAUDE.md with
  Ledger's embedded rules; read it before touching Rust.
- `tests/` - pytest driving one or two Speculos instances via the Speculos REST/TCP API
  (deliberately NOT Ragger: it assumes a single device). The dual-instance ceremony
  tests are the project's benchmark (M3/M4).
- `verifier/` - independent TypeScript verifier (@noble/curves), used by tests and demo.
  Must share no code with the device app: it is the adversarial check.
- `relay/` - dumb APDU shuttle between the two devices (TCP to Speculos, HID to real
  Flexes). Holds no secrets; the protocol assumes it is hostile.
- `docs/protocol.md` - the ceremony protocol (commit-reveal ECDH pairing, SAS words,
  press certificates, threat model). Keep in sync with the code.
- `CEREMONIE-VIDEO.md` - runbook for the filmed ceremony on the two real Flexes with
  the Lot 1 build (French). Driven by `scripts/preflight.sh` (read-only pre-flight),
  `scripts/ceremony.sh` (the live relay) and `scripts/rehearse-emu.sh` (Speculos
  rehearsal). Any APDU-format change must be reflected there or the live cut fails.

## Vocabulary (use this, not crypto jargon)

master (the artist's plate, holds the press counter), pressing (numbered copy, "4/5"),
cut (create a master), press (issue a copy onto a device), give (hand a held copy
on to another device), sold out (counter at 0), album (the work), sleeve (the cover
art, a 1bpp square hashed into the album cert), bearer key (the secp256k1 scalar a
copy is bound to; holding it IS holding the copy).
Never "mint/child/mother/token".

## Build & run (everything in WSL Ubuntu, aarch64)

- Toolchain: rustup (nightly pinned by device-app/rust-toolchain.toml), cargo-ledger,
  clang, gcc-arm-none-eabi. Installed under WSL root user.
- Build: `wsl -d Ubuntu -- bash -lc 'source ~/.cargo/env && cd /mnt/c/Users/sylve/projects/presse/device-app && CARGO_TARGET_DIR=~/target-presse cargo ledger build flex'`
  CARGO_TARGET_DIR stays on ext4 (WSL home): building on /mnt/c is slow.
- Speculos + pytest live in `~/venv-ledger` inside WSL.
- Windows-side Python/Bun are NOT used for device work (win-arm64 native-module swamp);
  USB goes to WSL via usbipd when loading real devices.

## Gotchas

- This laptop is Windows-on-ARM: no Docker, x86_64 Ledger containers unusable. Native
  aarch64 WSL toolchain works; GitHub Actions x86 runners are the fallback CI.
- Two secrets rules from the device app: album/device keys are TRNG-generated and live
  only in app NVRAM (never seed-derived: the owner knows their 24 words and could
  re-press off-device). Losing the master = plates destroyed, by design.
- Speculos OCR (`/events`) is how tests read the screens; SAS word equality across the
  two instances is asserted through it.
- The app opens on a **library** (the landing screen), not a home button; it is built
  from raw `nbgl_layout` and yields to APDUs so a ceremony works with it on screen.
- A copy is bound to a **bearer key**, not to a device, and a give is a **two-phase
  commit**: `GIVE_OFFER` atomically commits the copy to one named recipient (still
  in flash, but silent and un-offerable elsewhere), and only that recipient's
  MACed receipt makes `GIVE_FINISH` erase it. Never collapse this back into one
  write: erasing as the key leaves means a dropped cable destroys a copy. The
  invariant to preserve is *one recipient, never widened*. `INS_CHALLENGE` signs
  with the bearer key and fails closed on a device that holds no copy or has
  committed the one it holds.
- `committed` is a **3-state byte**: 0 free, 1 promised (`GIVE_OFFER p1=0`, the
  gated write, key never sent), 2 flown (`GIVE_OFFER p1=1`, written *before* the
  sealed key reaches the wire). `GIVE_CANCEL` (0x7D, UI-gated, unpaired) takes a
  promise back at 1 only and answers `KeyFlown` 0xB10A at 2. Never let a cancel
  reach state 2 and never write the flown byte after the send: either is a
  double-spend primitive. The library row says `promised, reconnect XXXXXXXX` for
  both, deliberately.
- Development screen probes are off by default, behind **two** cargo features:
  `artprobe` (ART_TEST and the raw `Screen`/`nbgl_screenPush` path behind it,
  which `scripts/art-test.sh` and `show-sleeve.sh` drive) and `uiprobe`
  (LIBRARY_PREVIEW, CARD_PREVIEW; capture only). Two and not one because
  `--features artprobe,uiprobe` lands at `text` 77104, outside the boot window;
  each alone boots (76080 and 75568). Build with `build-video.sh -- --features
  artprobe`.
- `.nvm_data` is nearly full: `data_size` is 18944 and the app stops booting
  somewhere between 18432 and 19456. Re-run the boot check after *any* NVM struct
  change, or the app installs and dies without a message.
- From the code side it is a **window, not a ceiling**, and this is the single
  most misleading thing about this app. With the current NVM structs the app
  boots for a `text` anywhere in **74032..76080** and dies outside it *in both
  directions*: 76592 fails, and so does 73520. Measured by sweeping an inert
  `#[used]` ballast array in 512-byte steps (`text` moves in 512s, `.rodata` is
  512-aligned), each point boot-checked 3x. So **deleting code can break the
  app**, and a diet that frees more than about 2 KB has to be paid back or it
  lands under the floor. `data_size` is not an independent knob here: it is
  derived from the same link layout (`_envram_data - _nvram_data`) and moves with
  `text` -- 18432 at 74032, 18944 at 76080 -- so the old "the ceiling is at
  `data_size` 18944" was this same phenomenon seen from the other side. The
  failure is silent either way: the app installs, panics before its first APDU
  (`exiting_panic` -> `exit_app(0)`, which the Speculos log shows as
  `exit called (0)`) and answers nothing. Read `text` from the build output and
  boot-check every change that moves it.
- **Screens do not scroll, and overrunning one is silent.** Flex is 480x600 with
  the header's rule at y=96 and the footer's at y=504, so a page has 408px: four
  92px touchable bars, or a tag/value list whose last renderable line starts at
  y=468. Past that, NBGL draws the row anyway — under the footer, showing only
  the tops of its glyphs — and past y=600 Speculos faults the draw outright. No
  error, and the text is still in `/events`, so only a coordinate assertion sees
  it (`assert_page_fits` in tests/presse_client.py). Anything whose height
  follows device state (a row per fact held, a list of every previous holder) is
  the shape of this bug: give the page a fixed row count and bound every value.
- Sleeves must be uploaded (SET_ART) **before** the cut: the cut hashes the art into
  the album cert's `sleeve_hash`. No separate seal step. `scripts/sleeve.py` packs a
  cover into the exact 1bpp bytes; the device inverts polarity at render time only
  (canonical bytes are white-on-black and are what the hash covers). See docs/protocol.md.
