#!/bin/bash
# Visual emulated demo (two Speculos Flexes viewable in a browser).
# Usage: demo-emu.sh [--auto]
source "$(dirname "$0")/env.sh"
cd /mnt/c/Users/sylve/projects/presse
exec python3 relay/demo_emu.py "$@"
