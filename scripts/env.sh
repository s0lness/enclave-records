#!/bin/bash
# presse build/run environment. Source me. (LF endings; Linux, macOS or WSL.)
#
# The repo root is derived from the location of THIS file, so every script acts on
# the checkout it lives in, with no username and no absolute path anywhere.
# APP_DIR, APP_ELF and FLEX_SDK are defaults, NOT overrides: a caller that already
# exported one keeps it. This file used to pin APP_DIR to a sibling checkout, which
# silently pointed every script that sourced it at the wrong worktree, and once let
# a boot check pass on the wrong binary.
PRESSE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PRESSE_ROOT
export APP_DIR=${APP_DIR:-$PRESSE_ROOT/device-app}
export APP_ELF=${APP_ELF:-$APP_DIR/target/flex/release/presse}
export FLEX_SDK=${FLEX_SDK:-$HOME/ledger-secure-sdk}
# Rust via rustup, and the venv holding ledgerblue + speculos. Both are skipped
# when absent: a machine that already has cargo, speculos and pytest on PATH needs
# neither line.
if [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
if [ -d "$HOME/venv-ledger/bin" ]; then export PATH="$HOME/venv-ledger/bin:$PATH"; fi

# The emulator in that venv is patched, and has to stay patched: stock Speculos
# loads only 4096..7680 bytes of a Rust app's .nvm_data (docs/speculos-nvm-loading.md).
# A `pip install -U speculos` silently puts the stock loader back, so check on
# every source and re-apply. Never fatal here: build.sh does not need Speculos,
# and the load-size guard below catches the fatal case with or without the patch.
for _presse_spec in "$HOME"/venv-ledger/lib/python*/site-packages/speculos/main.py; do
  [ -f "$_presse_spec" ] || continue
  grep -q PRESSE-SPECULOS-NVM-PATCH "$_presse_spec" \
    || "$PRESSE_ROOT/scripts/patch-speculos.sh" --quiet \
    || echo "warning: speculos is unpatched, .nvm_data will load only in part" >&2
  break
done
unset _presse_spec

# The load size Speculos sizes its mapping from: the p_filesz of the PT_LOAD
# holding .text, which the linker script makes `_erodata - _text`. It is NOT the
# `text` figure cargo-ledger prints (that one omits the padding between
# .rel_flash and .rodata, 208 bytes today), and it moves on its own whenever the
# relocation count changes.
presse_load_size() {
  local elf="${1:-$APP_ELF}" readelf_bin
  readelf_bin=$(command -v readelf || command -v arm-none-eabi-readelf) || return 1
  [ -f "$elf" ] || return 1
  local t e
  t=$("$readelf_bin" -sW "$elf" 2>/dev/null | awk '$8=="_text"{print $2; exit}')
  e=$("$readelf_bin" -sW "$elf" 2>/dev/null | awk '$8=="_erodata"{print $2; exit}')
  [ -n "$t" ] && [ -n "$e" ] || return 1
  echo $(( 0x$e - 0x$t ))
}

# One line of arithmetic between a working build and a silent death. When the
# load size is a multiple of the 4096-byte host page, Speculos maps exactly one
# page of .nvm_data, both AtomicStorage validity flags fall outside it, and the
# app panics before its first APDU with no message anywhere.
# Physical devices are unaffected: presse.hex carries the whole region.
# ALLOW_NVM_NOTCH=1 downgrades this to a warning.
presse_check_load_size() {
  local elf="${1:-$APP_ELF}" size
  size=$(presse_load_size "$elf") || { echo "load-size guard skipped (no readelf, or no ELF at $elf)" >&2; return 0; }
  if [ $(( size % 4096 )) -ne 0 ]; then
    echo "load size $size bytes, notch clear ($(( size % 4096 )) past a page boundary)"
    return 0
  fi
  echo "" >&2
  echo "LOAD SIZE $size IS A MULTIPLE OF 4096 - this build cannot boot on stock Speculos." >&2
  echo "  Speculos maps the .text segment plus one page, so exactly 4096 bytes of" >&2
  echo "  .nvm_data arrive: presse::state::DATA sits at offset 4096, both of its" >&2
  echo "  0xa5 validity flags land outside, AtomicStorage::which() panics and the" >&2
  echo "  app exits before answering. Real Flexes boot this build fine." >&2
  echo "  Fix the emulator: scripts/patch-speculos.sh (docs/speculos-nvm-loading.md)." >&2
  echo "  Move the size: any change to code or .rodata shifts it off the boundary." >&2
  echo "  ALLOW_NVM_NOTCH=1 to build anyway." >&2
  echo "" >&2
  [ "${ALLOW_NVM_NOTCH:-0}" = 1 ] && return 0
  return 1
}
