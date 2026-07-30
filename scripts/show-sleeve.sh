#!/bin/bash
# show-sleeve.sh <file.bin> <out.png> - upload a packed sleeve into a fresh
# emulator and render it full size, so a stored asset can be checked against
# its host-side preview.
#
# The full-size render is ART_TEST, compiled in only under the `artprobe` cargo
# feature, so APP_ELF must point at such a build:
#   scripts/build-video.sh -- --features artprobe
# Without it the device answers 6D00 and the screenshot is of the library.
source "$(dirname "$0")/env.sh"
SRC="$1"
OUT="$2"
pkill -f "speculos.*5001" 2>/dev/null; sleep 1
nohup speculos --model flex --display headless --api-port 5001 --apdu-port 0 \
  "$APP_ELF" >/tmp/sleeve.log 2>&1 &
sleep 6
python3 - "$SRC" <<'EOF'
import sys, requests
data = open(sys.argv[1], "rb").read()
url = "http://127.0.0.1:5001/apdu"
CHUNK = 64
for off in range(0, len(data), CHUNK):
    payload = off.to_bytes(2, "little") + data[off:off + CHUNK]
    apdu = bytes([0xB5, 0x62, 0, 0, len(payload)]) + payload
    r = requests.post(url, json={"data": apdu.hex()}, timeout=10).json()
    if not r.get("data", "").endswith("9000"):
        sys.exit(f"SET_ART failed at offset {off}: {r}")
print(f"uploaded {len(data)} bytes")
EOF
# P1=1 renders whatever sleeve is in NVM.
curl -s --max-time 10 http://127.0.0.1:5001/apdu -d '{"data":"b561010000"}' >/dev/null
sleep 1
curl -s http://127.0.0.1:5001/screenshot -o "$OUT"
echo "saved $OUT"
pkill -f "speculos.*5001" 2>/dev/null
