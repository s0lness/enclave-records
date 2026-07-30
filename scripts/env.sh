#!/bin/bash
# presse WSL build environment. Source me. (LF endings, run inside WSL Ubuntu.)
source ~/.cargo/env
export FLEX_SDK=~/ledger-secure-sdk
export PATH=~/venv-ledger/bin:$PATH   # python3 -> venv (ledgerblue, speculos)
# Defaults, NOT overrides: a caller that already exported APP_DIR/APP_ELF keeps them.
# This file used to assign unconditionally, which silently pointed every script that
# sources it at the sibling `presse` checkout, whatever worktree it was run from. That
# is what made cockpit.sh, emu-up.sh and test.sh unusable here, and it once let a boot
# check pass on the wrong binary.
export APP_DIR=${APP_DIR:-/mnt/c/Users/sylve/projects/presse/device-app}
export APP_ELF=${APP_ELF:-$APP_DIR/target/flex/release/presse}
