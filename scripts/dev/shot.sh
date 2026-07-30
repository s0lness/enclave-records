#!/bin/bash
# shot.sh <out.png> [apdu-hex ...] - boot a fresh Flex, send each APDU, and
# save a screenshot of whatever is on screen at the end.
source "$(dirname "$0")/../env.sh"
OUT="$1"; shift
pkill -f "speculos.*5001" 2>/dev/null; sleep 1
nohup speculos --model flex --display headless --api-port 5001 --apdu-port 0 \
  "$APP_ELF" >/tmp/shot.log 2>&1 &
sleep 6
for apdu in "$@"; do
  echo "-> $apdu"
  curl -s --max-time 10 http://127.0.0.1:5001/apdu -d "{\"data\":\"$apdu\"}" | head -c 160; echo
done
sleep 1
curl -s http://127.0.0.1:5001/screenshot -o "$OUT"
echo "saved $OUT"
pkill -f "speculos.*5001" 2>/dev/null
