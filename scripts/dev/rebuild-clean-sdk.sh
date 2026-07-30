#!/bin/bash
# Force the ledger_device_sdk build script to rerun (it caches the app's
# install parameters, incl. the dashboard name, and doesn't watch our
# Cargo.toml), then rebuild and print the embedded app name.
set -e
source "$(dirname "$0")/../env.sh"
cd "$APP_DIR"
cargo clean -p ledger_device_sdk 2>/dev/null || rm -rf target/flex/release/build/ledger_device_sdk-*
cargo ledger build flex 2>&1 | grep -E "Retrieved ELF infos|full hash"
