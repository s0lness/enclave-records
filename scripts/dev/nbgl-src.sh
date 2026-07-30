#!/bin/bash
# Print the C implementation of a raw NBGL entry point from the vendored SDK.
source "$(dirname "$0")/../env.sh"
grep -rn "$1" "$FLEX_SDK"/lib_nbgl/src/*.c "$FLEX_SDK"/lib_nbgl/include/*.h 2>/dev/null | head -20
