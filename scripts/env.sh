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
