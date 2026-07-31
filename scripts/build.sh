#!/bin/bash
# Build this checkout's device app for Flex (Linux, macOS or WSL).
set -e
source "$(dirname "$0")/env.sh"
cd "$APP_DIR"
cargo ledger build flex "$@"
# A build that lands on a page boundary installs and dies without a message on
# stock Speculos. Refuse to hand one over silently.
presse_check_load_size
