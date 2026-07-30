#!/bin/bash
# Build this checkout's device app for Flex (Linux, macOS or WSL).
set -e
source "$(dirname "$0")/env.sh"
cd "$APP_DIR"
cargo ledger build flex "$@"
