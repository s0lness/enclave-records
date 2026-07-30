#!/bin/bash
# Compile the app and show only the diagnostics (the ledger build wrapper
# buries them under several screens of install-param warnings).
source "$(dirname "$0")/../env.sh"
cd "$APP_DIR"
cargo ledger build flex >/tmp/build.log 2>&1
grep -vE "^warning: ledger_device_sdk@|^\s*$" /tmp/build.log | head -120
