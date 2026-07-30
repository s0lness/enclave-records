#!/bin/bash
# Capture the record card's DETAIL page (page 2), reached by swiping the card.
# Boots one fresh Flex, uploads+seals the RAM sleeve, cuts a master, opens the
# record, then navigates to the detail page and screenshots it.
source "$(dirname "$0")/../env.sh"
cd "$PRESSE_ROOT"
OUT="${1:-docs/screens/dev/detail-page.png}"
RAM=docs/art/ram-cover.bin

pkill -f "speculos.*5001" 2>/dev/null; sleep 1
rm -rf /tmp/presse-relay
nohup speculos --model flex --display headless --api-port 5001 --apdu-port 0 "$APP_ELF" >/tmp/detail.log 2>&1 &
for _ in $(seq 1 40); do curl -s -o /dev/null "http://127.0.0.1:5001/events" && break; sleep 0.3; done
sleep 2

python3 - "$RAM" <<'EOF'
import sys, struct, requests
data = open(sys.argv[1], "rb").read()
for off in range(0, len(data), 64):
    p = struct.pack("<H", off) + data[off:off + 64]
    apdu = bytes([0xB5, 0x62, 0, 0, len(p)]) + p
    requests.post("http://127.0.0.1:5001/apdu", json={"data": apdu.hex()}, timeout=10)
print("uploaded sleeve")
EOF

python3 relay/demo_steps.py cut "Random Access Memories" 5 >/dev/null 2>&1 &
PID=$!
sleep 2.5
bash scripts/dev/tap.sh 5001 "Cut the master" >/dev/null
wait $PID
sleep 1

# Open the record from the library, then swipe to the detail page.
bash scripts/dev/tap.sh 5001 "Random Access" >/dev/null
sleep 1.5
# The card shows "1 of 2"; the forward chevron is bottom-right.
curl -s -X POST http://127.0.0.1:5001/finger -H 'Content-Type: application/json' \
  -d '{"x":430,"y":550,"action":"press-and-release"}' >/dev/null
sleep 1.5
curl -s http://127.0.0.1:5001/screenshot -o "$OUT"
echo "saved $OUT"
pkill -f "speculos.*5001" 2>/dev/null
