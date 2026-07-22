#!/bin/bash
# Full in-situ demo capture, reproducible from a clean state.
#
# Boots two fresh Flex emulators (no persisted NVM), uploads the Random Access
# Memories sleeve to A, then walks the whole ceremony -- cut, pair+SAS, press,
# receive -- driving each UI-gated beat by tapping the on-screen button, and
# captures every screen to docs/screens/full-demo/. The library is the landing
# screen throughout, so this run is also the proof that a ceremony still works
# with the library yielding to APDUs.
#
# Run from WSL with nothing else on ports 5001/5002:
#   bash scripts/storyboard-full.sh
set -e
source "$(dirname "$0")/env.sh"
cd /mnt/c/Users/sylve/projects/presse
OUT=docs/screens/full-demo
mkdir -p "$OUT"

RAM=docs/art/ram-cover.bin

snap() { curl -s "http://127.0.0.1:$1/screenshot" -o "$OUT/$2.png"; }
tap() { bash scripts/tap.sh "$1" "$2" >/dev/null; }

# --- fresh emulators (delete any persisted NVM first) --------------------
pkill -f "speculos.*--api-port 500[12]" 2>/dev/null || true
sleep 1
rm -rf /tmp/presse-relay
nohup speculos --model flex --display headless --api-port 5001 --apdu-port 0 "$APP_ELF" >/tmp/sb-a.log 2>&1 &
nohup speculos --model flex --display headless --api-port 5002 --apdu-port 0 "$APP_ELF" >/tmp/sb-b.log 2>&1 &
for port in 5001 5002; do
  for _ in $(seq 1 40); do
    curl -s -o /dev/null "http://127.0.0.1:$port/events" && break
    sleep 0.3
  done
done
sleep 2

# --- sleeve upload + cut on A -------------------------------------------
# The cover must be uploaded and sealed BEFORE the cut, so the master binds a
# sleeve it can render. demo_steps.py `art` uploads+seals to whichever device
# already holds something; here we upload straight, then cut.
python3 - "$RAM" <<'EOF'
import sys, struct, requests
data = open(sys.argv[1], "rb").read()
CHUNK = 64
for off in range(0, len(data), CHUNK):
    payload = struct.pack("<H", off) + data[off:off + CHUNK]
    apdu = bytes([0xB5, 0x62, 0, 0, len(payload)]) + payload
    r = requests.post("http://127.0.0.1:5001/apdu", json={"data": apdu.hex()}, timeout=10).json()
    assert r["data"].endswith("9000"), r
print(f"A: uploaded {len(data)} sleeve bytes")
EOF

python3 relay/demo_steps.py cut "Random Access Memories" 5 > "$OUT/out-cut.txt" 2>&1 &
PID=$!
sleep 2.5
snap 5001 "03-a-cut-review"
tap 5001 "Cut the master"
wait $PID
# Seal the sleeve now that A holds the master (hash binds to the cut edition).
python3 - <<'EOF'
import requests
r = requests.post("http://127.0.0.1:5001/apdu", json={"data": "b563000000"}, timeout=10).json()
assert r["data"].endswith("9000"), r
print("A: sleeve sealed")
EOF
sleep 1
snap 5001 "01-a-home"     # A's library, now holding the RAM record
snap 5002 "02-b-home"     # B's library, still empty

# --- A's record card in situ --------------------------------------------
tap 5001 "Random Access"
sleep 1.5
snap 5001 "09-a-record-card"
tap 5001 "Back"
sleep 1

# --- pairing + SAS -------------------------------------------------------
python3 relay/demo_steps.py pair > "$OUT/out-pair.txt" 2>&1 &
PID=$!
sleep 3.5
snap 5001 "05-a-sas"
snap 5002 "06-b-sas"
tap 5001 "Words match"
tap 5002 "Words match"
wait $PID

# --- press + receive -----------------------------------------------------
python3 relay/demo_steps.py press > "$OUT/out-press.txt" 2>&1 &
PID=$!
sleep 2.5
snap 5001 "07-a-press-offer"
tap 5001 "Press this copy"
sleep 2.5
snap 5002 "08-b-receive"
tap 5002 "Receive it"
wait $PID
sleep 1

# --- B's record card after receiving ------------------------------------
# Carry the sleeve across to B and seal it, so B renders the real cover too.
python3 - "$RAM" <<'EOF'
import sys, struct, requests
data = open(sys.argv[1], "rb").read()
CHUNK = 64
for off in range(0, len(data), CHUNK):
    payload = struct.pack("<H", off) + data[off:off + CHUNK]
    apdu = bytes([0xB5, 0x62, 0, 0, len(payload)]) + payload
    r = requests.post("http://127.0.0.1:5002/apdu", json={"data": apdu.hex()}, timeout=10).json()
    assert r["data"].endswith("9000"), r
r = requests.post("http://127.0.0.1:5002/apdu", json={"data": "b563000000"}, timeout=10).json()
assert r["data"].endswith("9000"), r
print("B: sleeve carried across and sealed")
EOF
tap 5002 "Random Access"
sleep 1.5
snap 5002 "10-b-record-card"
tap 5002 "Back"

# --- verify --------------------------------------------------------------
python3 relay/demo_steps.py verify > "$OUT/out-verify.txt" 2>&1

# --- wire log (cockpit only) --------------------------------------------
if curl -s -o /dev/null --max-time 1 "http://127.0.0.1:5050/log"; then
  curl -s "http://127.0.0.1:5050/log?after=0" -o "$OUT/wire.json"
  echo "wire.json captured"
else
  echo "cockpit not up on :5050 -- skipping wire.json"
fi

echo "--- transcripts ---"
cat "$OUT"/out-*.txt
echo "CAPTURED in $OUT"
pkill -f "speculos.*--api-port 500[12]" 2>/dev/null || true
