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
  press certificates, wire formats, APDU map). Keep in sync with the code.
- `docs/threat-model.md` - what the two promises rest on, every way a copy can be
  lost, why attestation is not implemented, what is out of scope. Attestation IS
  possible on a Ledger (issuer certificate + device certificate + a scheme-1
  signature over the running app's hash); it is declined for its cost and for
  Ledger-as-trust-root. Never restore the claim that a Ledger app cannot prove it
  is a Ledger. The README carries only a
  summary and links here.
- `docs/speculos-nvm-loading.md` - the upstream bug report for the Speculos
  loader defect described under Gotchas, with the measurement, the mechanism
  and the fix this repo vendors. Nothing has been filed on GitHub.
- `CEREMONIE-VIDEO.md` - runbook for the filmed ceremony on the two real Flexes
  (French), with the header table naming the build actually flashed. Driven by
  `scripts/preflight.sh` (read-only pre-flight), `scripts/ceremony.sh` (the live
  relay), `scripts/give.sh` (the cession) and `scripts/rehearse-emu.sh` (Speculos
  rehearsal). Any APDU-format change must be reflected there or the live cut fails.
- `scripts/` - only what gets run: `env.sh` (sourced by the rest, derives the repo
  root from its own path, so every script acts on the checkout it lives in),
  `build.sh`, `load.sh`, `test.sh`, `boottest.sh`, `emu-up.sh`/`emu-down.sh`,
  `cockpit.sh`, `rehearse-emu.sh`, `ceremony.sh`, `give.sh`, `preflight.sh`,
  `install-ca.sh`, `list-apps.sh`, `sleeve.py`, plus `patch-speculos.sh` and
  `speculos-nvm-data.patch`, which fix the emulator (see Gotchas).
  `build-video.sh` and
  `load-video.sh` are aliases of `build.sh`/`load.sh`, kept because the runbook
  calls them by those names. `scripts/dev/` is development archaeology (NVM
  ceiling and art sweeps, SDK symbol dumps, screen captures, `tap.sh`,
  `provision.sh`); `scripts/windows/` is the two usbipd PowerShell helpers, which
  exist only because Windows has to forward USB into WSL. No script hardcodes a
  path any more: pass `APP_DIR`/`APP_ELF`/`FLEX_SDK` to override a default.

## Vocabulary (use this, not crypto jargon)

master (the artist's plate, holds the press counter), pressing (numbered copy, "4/5"),
cut (create a master), press (issue a copy onto a device), give (hand a held copy
on to another device), sold out (counter at 0), album (the work), sleeve (the cover
art, a 1bpp square hashed into the album cert), bearer key (the secp256k1 scalar a
copy is bound to; holding it IS holding the copy).
Never "mint/child/mother/token".

Two ids on screen, each naming a different thing: **Device ID** is
`SHA256(devpub)[:8]`, one device's own name (the back of its card, its empty
library, and every screen naming the other side of a ceremony); **Edition ID** is
`SHA256(albpub)[:8]`, the edition's name, identical on every copy of it and the
one a buyer checks against the artist's channel. Never "Collection ID": the
value fingerprints a device's key, and the label has to say so.

## Build & run (on this machine: WSL Ubuntu, aarch64)

- Toolchain: rustup (nightly pinned by device-app/rust-toolchain.toml), cargo-ledger,
  clang, gcc-arm-none-eabi. Installed under WSL root user.
- Build: `wsl -d Ubuntu -- bash <repo>/scripts/build.sh` (it sources `env.sh`,
  which derives the repo root from its own location, so it builds the checkout it
  lives in). For a faster rebuild, export `CARGO_TARGET_DIR=~/target-presse`:
  keeping the target dir on ext4 beats /mnt/c.
- Speculos + pytest live in `~/venv-ledger` inside WSL; `env.sh` puts that on PATH
  when it exists and skips it when it does not.
- Windows-side Python/Bun are NOT used for device work (win-arm64 native-module swamp);
  USB goes to WSL via usbipd (`scripts/windows/`) when loading real devices. On
  Linux or macOS that step does not exist and neither script is needed.

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
- A copy's history is a **32-byte rolling hash**, not a list: the giver signs the
  head it received into the handover record, and the taker folds that record into
  a new head. Constant size, constant verification cost, nothing ever dropped.
  The point is evidence, not prevention: forged, head-substituted, replayed and
  grafted links are refused at TAKE, but a *modified* giver can still truncate,
  because a receiver holds a digest and not the witnesses (one signature per hop
  is 72 bytes and does not fit). Never present the chain as making duplication
  impossible: it makes a circulating duplicate fork, and the fork names the
  device that split it. Nor as an attestation: one chain on its own proves
  nothing, a clone's head being equally valid and grown from the same public
  root. It is comparative evidence, and the comparison is a verifier's job, off
  the device. What the screen carries is the head itself, as **eight words**
  (its first 8 bytes through the 256-word SAS list, two to a line) on a
  `History` sub-page under `Device ID`, for a pressing only: 64 bits is what
  survives a forger grinding a fabricated history against a shown prefix, and a
  master's head is the all-zero sentinel. The same rule renders off-device from
  `GET_BUNDLE p1=2`, so both sides of a comparison derive the words instead of
  trusting them. See docs/protocol.md, "The provenance chain", and
  docs/threat-model.md, "A lone chain proves nothing".
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
- Development screen probes are off by default, behind their own cargo features:
  `artprobe` (ART_TEST and the raw `Screen`/`nbgl_screenPush` path behind it,
  which `scripts/dev/art-test.sh` and `dev/show-sleeve.sh` drive) and `uiprobe`
  (LIBRARY_PREVIEW, CARD_PREVIEW; capture only). Two features and not one
  because they drive different things and a capture usually wants one of them.
  The reason previously recorded here, that `--features artprobe,uiprobe`
  landed outside a boot window, was the Speculos loader bug and holds no more:
  build them together freely. Build with `build.sh -- --features artprobe`.
  A third, `factprobe`, probes the OS rather than a screen: FACTORY_PROBE
  (0x69), one APDU onto the `os_factory_setting_get` syscall, whose id space is
  documented nowhere public. It reads only and answers verbatim, with the output
  buffer poisoned to 0xA5 first (Speculos does not implement that syscall: it
  answers 0 and leaves the buffer alone, so without the poison every id looks
  like a successful run of zeros) and the call wrapped in its own BOLOS try
  context (an id the OS refuses throws, and an uncaught throw exits the app,
  which on hardware costs a physical relaunch). `id 0xDEADBEEF` throws on
  purpose to prove the catch works before a sweep leans on it. Boot-checked 6x.
  Driver: `scripts/dev/factory-sweep.py`.
- **Space is not a constraint on this app.** The per-app flash region is
  **400 KiB** (`$FLEX_SDK/target/flex/script.ld`: `FLASH (rx) : ORIGIN =
  0xc0de0000, LENGTH = 400K`). Today's build spans `_text` to `_nvram_end`,
  83870 bytes, about **20%** of it, of which `.nvm_data` is 7582. Add code, grow
  an NVM struct, cut 20 KB: none of it approaches an edge. The `data_size`
  cargo-ledger prints is `_envram_data - _nvram_data`, and `_nvram_data` sits at
  the start of `.rodata`, so that number folds read-only data into what reads
  like an NVM figure. The NVM usage to look at is the size of `.nvm_data`.
- **The Speculos loader drops most of `.nvm_data`, and that is what every old
  "boot ceiling" number was measuring.** Stock Speculos sizes the app's mapping
  from the `PT_LOAD` that holds `.text` (`speculos/main.py:92`,
  `ei.text_size = text_seg['p_filesz']`) and `src/launcher.c:361` maps one spare
  page past it. `.nvm_data` is a separate `PT_LOAD`, so the bytes of it that
  reach emulated memory are `4096 + ((-load_size) mod 4096)`, between 4096 and
  7680, against a `.nvm_data` of 7582. `presse::state::DATA` sits at offset
  **4096** (`ART_MASTER` and `ART_PRESSING`, 2048 each, link ahead of it), so
  when `load_size` is a multiple of 4096 both `SafeStorage` 0xa5 flags land
  outside the mapping, read 0, and `AtomicStorage::which()` panics
  (`ledger_device_sdk-1.36.0/src/nvm.rs:226`, reached from
  `device-app/src/state.rs`). The app installs, exits before its first APDU
  (`exiting_panic` -> `exit_app(0)`, `exit called (0)` in the log) and answers
  nothing. **Physical Flexes are unaffected**: `presse.hex` covers the whole
  region including both flags.
  - `load_size` is the `p_filesz` of that segment, `_erodata - _text`. It is
    **not** the `text` cargo-ledger prints, which omits the padding the linker
    puts between `.rel_flash` and `.rodata` (208 bytes today, since `.rel_flash`
    pads 2352 up to 2560). That padding moves with the relocation count, so the
    failing sizes move too. Gate on `load_size`, never on `text`.
  - `scripts/patch-speculos.sh` fixes the emulator, applying
    `scripts/speculos-nvm-data.patch` to the speculos in `~/venv-ledger`. It is
    idempotent, `env.sh` re-applies it on every source, and `pip install -U
    speculos` is the one thing that puts the bug back. `--check` reports,
    `--revert` undoes.
  - `presse_check_load_size` (in `env.sh`, called by `build.sh` and
    `boottest.sh`) fails the build when `load_size % 4096 == 0`, so a binary
    that would die on someone else's stock emulator never leaves quietly.
    `ALLOW_NVM_NOTCH=1` to override.
- **`storage_b` has never run, in any test, ever.** Even at sizes that boot,
  stock Speculos loads only part of `.nvm_data`. When `load_size mod 4096` is
  3584, just 4608 bytes arrive, so the second `SafeStorage` flag at offset 5376
  reads 0 and the app runs on `storage_a` alone, with the tearing-recovery copy
  silently absent. `install_parameters`, at offset 7168, is missing from seven
  residues out of eight for the same reason. Everything past the boundary is
  zero, so nothing shows. The redundancy behind "a power cut burns a number and never duplicates
  one" has therefore never been exercised: **any test that believes it is
  covering the B storage is currently testing nothing.** The loader patch is
  what makes such a test possible; writing one is still open work.
- Someone will propose **emitting `DATA` first inside `.nvm_data`** as a cheaper
  fix than patching the loader. The price is high and the hazard survives it.
  The current order (`ART_MASTER`, `ART_PRESSING`, `DATA`) is chosen by compiler
  emission and is the reverse of the source order, so controlling it needs a
  distinct input section, a split of the `*(.nvm_data*)` glob in `link.ld`
  (which lives in the `ledger_secure_sdk_sys` crate, so vendor or patch that
  too), a build-time assertion that the offset really is 0, and the art tests
  re-run at all eight residues. It also only holds while `DATA` stays under
  4096 bytes. Worth having as defence in depth one day, at that price.
- **Numbers in the history to disbelieve.** Commit messages, `docs/art/README.md`
  and a comment in `device-app/src/state.rs` still carry these, and every one is
  an artifact of the loader bug above: a boot "window" of `text` 74032..76080
  that killed the app in both directions; an `.nvm_data` ceiling somewhere
  between 18432 and 19456; "deleting code can break the app"; two 160-wide
  sleeves dying in every arrangement tried; `--features artprobe,uiprobe` being
  unbuildable. What failed was every size whose `load_size` fell on a 4096-byte
  boundary. That is also why the edges looked soft and why `text` 76592, once
  recorded as failing, boots 3/3 today: 208 bytes of linker padding move with
  the relocation count, so the same `text` maps onto a different `load_size`
  from one build to the next. Do not re-derive any of it from the history.
- **Screens do not scroll, and overrunning one is silent.** Flex is 480x600 with
  the header's rule at y=96 and the footer's at y=504, so a page has 408px: four
  92px touchable bars, or a tag/value list whose last renderable line starts at
  y=468. Past that, NBGL draws the row anyway (under the footer, showing only
  the tops of its glyphs), and past y=600 Speculos faults the draw outright. No
  error, and the text is still in `/events`, so only a coordinate assertion sees
  it (`assert_page_fits` in tests/presse_client.py). Anything whose height
  follows device state (a row per fact held, a list of every previous holder) is
  the shape of this bug: give the page a fixed row count and bound every value.
- Sleeves must be uploaded (SET_ART) **before** the cut: the cut hashes the art into
  the album cert's `sleeve_hash`. No separate seal step. `scripts/sleeve.py` packs a
  cover into the exact 1bpp bytes; the device inverts polarity at render time only
  (canonical bytes are white-on-black and are what the hash covers). See docs/protocol.md.
- **`ART_W = 128` stands on the cell rule alone.** The art is written in
  64-byte cells, so `ART_W * ART_W / 8` has to divide by 64; that is what rules
  out every width between 128 and 160, and it is a real constraint (a width
  that fails it leaves `ART_CELLS * ART_CHUNK` short of `ART_LEN` and every
  `Art::get` runs off the end). The second reason on record in
  `device-app/src/state.rs` and `docs/art/README.md`, that two 160-wide slots
  produce an app that exits before its first APDU "in every arrangement tried",
  was the Speculos loader: 160-wide slots push `DATA` to offset 6400 in
  `.nvm_data`, past most of the 4096..7680 the emulator maps, so its flags fall
  outside for most load sizes whatever the arrangement, and the second flag
  falls outside for all of them. Flash has
  room for 160 and always did. Keep 128 unless something asks for more, and if
  something does, re-run the art tests and a boot check on a patched emulator
  rather than treating the old failure as evidence.
