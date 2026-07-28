#!/bin/bash
# Build the Lot 1 app of THIS worktree (presse-video) for Flex, without sideloading.
# Do NOT source scripts/env.sh here: it pins APP_DIR to the ../presse checkout, and
# build.rs resolves its metadata from the crate directory, so the build must run in
# tree. Sideload the result with scripts/load-video.sh (one device attached).
set -e
source ~/.cargo/env
export FLEX_SDK=~/ledger-secure-sdk
export PATH=~/venv-ledger/bin:$PATH
export APP_DIR=/mnt/c/Users/sylve/projects/presse-video/device-app
export APP_ELF=$APP_DIR/target/flex/release/presse
cd "$APP_DIR"
cargo ledger build flex "$@"
