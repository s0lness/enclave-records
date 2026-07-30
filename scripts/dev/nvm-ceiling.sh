#!/bin/bash
# Pin the exact app-NVRAM ceiling by growing the art region itself (live code,
# so nothing gets garbage-collected by the linker). Argument = art bytes,
# which must be a multiple of ART_CHUNK (64). Reports data_size + boot.
source "$(dirname "$0")/../env.sh"
STATE="$APP_DIR/src/state.rs"
cp "$STATE" /tmp/state.rs.ceil.bak
restore() { cp /tmp/state.rs.ceil.bak "$STATE"; }
trap restore EXIT

for BYTES in "$@"; do
  restore
  sed -i "s|^pub const ART_LEN: usize = .*|pub const ART_LEN: usize = $BYTES;|" "$STATE"
  BUILD=$(cd "$APP_DIR" && cargo ledger build flex 2>&1)
  SIZE=$(echo "$BUILD" | grep -oE "data_size: [0-9]+" | head -1)
  if ! echo "$BUILD" | grep -q "Application full hash"; then
    printf 'art=%-6s BUILD FAILED\n' "$BYTES"
    echo "$BUILD" | grep -iE "^error" | head -3
    continue
  fi
  pkill -f "speculos.*5001" 2>/dev/null; sleep 1
  nohup speculos --model flex --display headless --api-port 5001 --apdu-port 0 \
    "$APP_ELF" >/tmp/ceil.log 2>&1 &
  sleep 6
  INFO=$(curl -s --max-time 5 http://127.0.0.1:5001/apdu -d '{"data":"b501000000"}')
  pkill -f "speculos.*5001" 2>/dev/null
  if [ -n "$INFO" ]; then
    printf 'art=%-6s %-18s BOOT OK\n' "$BYTES" "$SIZE"
  else
    printf 'art=%-6s %-18s BOOT FAIL\n' "$BYTES" "$SIZE"
  fi
done
