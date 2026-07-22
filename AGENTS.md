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

## Vocabulary (use this, not crypto jargon)

master (the artist's plate, holds the press counter), pressing (numbered copy, "4/5"),
cut (create a master), press (issue a copy onto a device), sold out (counter at 0),
album (the work), sleeve (the cover art, a 1bpp square hashed into the album cert).
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
- Sleeves must be uploaded (SET_ART) **before** the cut: the cut hashes the art into
  the album cert's `sleeve_hash`. No separate seal step. `scripts/sleeve.py` packs a
  cover into the exact 1bpp bytes; the device inverts polarity at render time only
  (canonical bytes are white-on-black and are what the hash covers). See docs/protocol.md.
