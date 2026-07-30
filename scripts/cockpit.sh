#!/bin/bash
# Serve the two-device demo cockpit on http://localhost:5050 (emu-up first).
source "$(dirname "$0")/env.sh"
cd "$PRESSE_ROOT"
exec python3 relay/cockpit.py
