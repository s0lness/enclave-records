#!/bin/bash
# Delete the stale predecessor apps, keeping only "Enclave Records".
# Each delete prompts for approval on the device screen.
source "$(dirname "$0")/env.sh"
for app in "Presse" "Rust Boilerplate"; do
  echo ">>> deleting: $app (approve on the device)"
  ledgerctl delete "$app" 2>&1 | tail -3 || echo "   (not present or skipped)"
done
echo "--- remaining apps:"
bash "$(dirname "$0")/list-apps.sh" 2>&1 | grep "APP:"
