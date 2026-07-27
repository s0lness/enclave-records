#!/bin/bash
# Sideload the Lot 1 build (worktree presse-video, branch lot1-ui-polish) onto the
# single Flex currently attached to WSL. Do NOT source scripts/env.sh here: it pins
# APP_DIR to the ../presse checkout. Prereqs: usbipd attach done, device unlocked
# and on the dashboard, Ledger Live closed. Approve the prompts on the device.
set -e
source ~/.cargo/env
export FLEX_SDK=~/ledger-secure-sdk
export PATH=~/venv-ledger/bin:$PATH
export APP_DIR=/mnt/c/Users/sylve/projects/presse-video/device-app
export APP_ELF=$APP_DIR/target/flex/release/presse
cd "$APP_DIR"
cargo ledger build flex -l
