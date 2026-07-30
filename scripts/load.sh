#!/bin/bash
# Build and sideload this checkout's app onto a connected Flex.
# Prereqs: the device is visible to the host (on Windows that means attached to
# WSL with scripts/windows/attach-usb.ps1; on Linux or macOS it already is),
# unlocked, on the dashboard, and the custom CA installed (scripts/install-ca.sh).
set -e
source "$(dirname "$0")/env.sh"
cd "$APP_DIR"
cargo ledger build flex -l
