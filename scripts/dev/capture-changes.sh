#!/bin/bash
# Capture the three screens changed this turn, from a clean state:
#   docs/screens/confirmation-no-disc.png  - a ceremony confirmation, disc gone
#   docs/screens/library-home.png          - the library landing screen
#   docs/screens/receiver-provenance.png   - the receiver's page-2 provenance
# Runs the whole ceremony with art on both sides so the receiver's sleeve
# reads "Verified".
set -e
source "$(dirname "$0")/../env.sh"
cd "$PRESSE_ROOT"
OUT=docs/screens
RAM=docs/art/ram-cover.bin
snap() { curl -s "http://127.0.0.1:$1/screenshot" -o "$OUT/$2.png"; }
tap() { bash scripts/dev/tap.sh "$1" "$2" >/dev/null; }
upload() {
  python3 - "$1" "$2" <<'EOF'
import sys, struct, requests
port, path = sys.argv[1], sys.argv[2]
data = open(path, "rb").read()
for off in range(0, len(data), 64):
    p = struct.pack("<H", off) + data[off:off + 64]
    apdu = bytes([0xB5, 0x62, 0, 0, len(p)]) + p
    requests.post(f"http://127.0.0.1:{port}/apdu", json={"data": apdu.hex()}, timeout=10)
EOF
}

pkill -f "speculos.*--api-port 500[12]" 2>/dev/null || true
sleep 1
rm -rf /tmp/presse-relay
nohup speculos --model flex --display headless --api-port 5001 --apdu-port 0 "$APP_ELF" >/tmp/cc-a.log 2>&1 &
nohup speculos --model flex --display headless --api-port 5002 --apdu-port 0 "$APP_ELF" >/tmp/cc-b.log 2>&1 &
for port in 5001 5002; do
  for _ in $(seq 1 40); do curl -s -o /dev/null "http://127.0.0.1:$port/events" && break; sleep 0.3; done
done
sleep 2

# CUT on A (art first), capturing the disc-less confirmation.
upload 5001 "$RAM"
python3 relay/demo_steps.py cut "Random Access Memories" 5 >/dev/null 2>&1 &
PID=$!; sleep 2.5
snap 5001 "confirmation-no-disc"
tap 5001 "Cut the master"; wait $PID
sleep 1
snap 5001 "library-home"

# PAIR + PRESS so B holds a pressing.
python3 relay/demo_steps.py pair >/dev/null 2>&1 &
PID=$!; sleep 3.5; tap 5001 "Words match"; tap 5002 "Words match"; wait $PID
python3 relay/demo_steps.py press >/dev/null 2>&1 &
PID=$!; sleep 2.5; tap 5001 "Press this copy"; sleep 2.5; tap 5002 "Receive it"; wait $PID
sleep 1

# Carry the sleeve to B so its provenance reads "Verified", open the record,
# swipe to the detail (page 2), capture.
upload 5002 "$RAM"
python3 relay/demo_steps.py collection b >/dev/null 2>&1 &
PID=$!
sleep 1.5
curl -s -X POST http://127.0.0.1:5002/finger -H 'Content-Type: application/json' \
  -d '{"x":430,"y":550,"action":"press-and-release"}' >/dev/null   # forward chevron
sleep 1.5
snap 5002 "receiver-provenance"
tap 5002 "Back"; wait $PID

echo "saved confirmation-no-disc / library-home / receiver-provenance in $OUT"
pkill -f "speculos.*--api-port 500[12]" 2>/dev/null || true
