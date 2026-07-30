#!/bin/bash
# Sweep the art region size to find the NVM ceiling: for each width, patch
# ART_W in state.rs, rebuild, boot Speculos, and check the app answers
# GET_INFO (a too-large region dies with "exit called" before any APDU).
# Usage: art-sweep.sh 128 160 192 224
source "$(dirname "$0")/../env.sh"
STATE="$APP_DIR/src/state.rs"
cp "$STATE" /tmp/state.rs.bak

restore() { cp /tmp/state.rs.bak "$STATE"; }
trap restore EXIT

for W in "$@"; do
  sed -i "s/^pub const ART_W: usize = .*/pub const ART_W: usize = $W;/" "$STATE"
  BYTES=$((W * W / 8))
  printf '=== ART_W=%s (1bpp, %s bytes) ===\n' "$W" "$BYTES"

  BUILD=$(cd "$APP_DIR" && cargo ledger build flex 2>&1)
  if ! echo "$BUILD" | grep -q "Application full hash"; then
    echo "BUILD FAILED"
    echo "$BUILD" | grep -iE "^error|cannot|overflow" | head -5
    continue
  fi
  echo "$BUILD" | grep -oE "data_size: [0-9]+"

  pkill -f "speculos.*5001" 2>/dev/null; sleep 1
  nohup speculos --model flex --display headless --api-port 5001 --apdu-port 0 \
    "$APP_ELF" >/tmp/sweep.log 2>&1 &
  sleep 6

  INFO=$(curl -s --max-time 5 http://127.0.0.1:5001/apdu -d '{"data":"b501000000"}')
  # Write the last chunk too: a region the loader accepts but that overruns
  # usable flash only shows up on a real write at the far end.
  LAST=$(( (BYTES / 64 - 1) * 64 ))
  OFF=$(printf '%02x%02x' $((LAST & 255)) $((LAST >> 8)))
  PAYLOAD="$OFF$(python3 -c 'print("a5"*64)')"
  SET=$(curl -s --max-time 5 http://127.0.0.1:5001/apdu \
    -d "{\"data\":\"b562000042$PAYLOAD\"}")
  INFO2=$(curl -s --max-time 5 http://127.0.0.1:5001/apdu -d '{"data":"b501000000"}')

  if [ -n "$INFO" ] && [ -n "$INFO2" ]; then
    echo "BOOT OK   get_info=${INFO:0:20}... set_art_tail=${SET:0:20}... alive_after=yes"
  else
    echo "BOOT FAIL get_info='$INFO' set_art='$SET' after='$INFO2'"
    tail -3 /tmp/sweep.log
  fi
  pkill -f "speculos.*5001" 2>/dev/null
done
