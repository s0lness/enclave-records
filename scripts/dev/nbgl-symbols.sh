#!/bin/bash
# What NBGL surface can an app actually link against? Two layers matter:
# the OS-side object/screen API (stubs) and the app-side layout API. Anything
# not listed here is declared by bindgen but will not link.
source "$(dirname "$0")/../env.sh"
OUT=$(ls -d "$APP_DIR"/target/flex/release/build/ledger_secure_sdk_sys-*/out | head -1)
echo "=== OS-side stubs (nbgl_stubs.o) ==="
nm -g --defined-only "$(find "$OUT" -name '*nbgl_stubs.o' | head -1)" 2>/dev/null \
  | awk '{print $NF}' | grep -E "^nbgl_" | tr '\n' ' '; echo
echo
echo "=== any nbgl_layout* / nbgl_useCase* compiled into the app ==="
find "$OUT" -name "*.o" | xargs -r nm -g --defined-only 2>/dev/null \
  | awk '{print $NF}' | grep -E "^nbgl_(layout|useCase|page|flow)" | sort -u | tr '\n' ' '; echo
echo
echo "=== C files the sys crate compiles ==="
find "$OUT" -name "*.o" -printf "%f\n" | sed 's/^[0-9a-f]*-//' | sort | tr '\n' ' '; echo
