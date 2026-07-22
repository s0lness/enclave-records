#!/bin/bash
# Record the README demo GIF, reproducibly, from a clean state.
#
# Boots two fresh Flex emulators (no persisted NVM), uploads the Random Access
# Memories sleeve to A, then walks the whole ceremony -- cut, pair+SAS, press,
# receive -- tapping each UI-gated beat, and captures the device screens at
# each beat into docs/screens/frames/raw/. Then compose_gif.py stitches the
# two screens + a plain-language caption into docs/demo.gif.
#
# The press carries the sleeve A->B, so B renders the real cover (no radar
# fallback). Run from WSL with nothing on ports 5001/5002:
#   bash scripts/record-demo.sh
set -e
source "$(dirname "$0")/env.sh"
cd /mnt/c/Users/sylve/projects/presse
OUT=docs/screens/frames/raw
mkdir -p "$OUT"
RAM=docs/art/ram-cover.bin

snap() { curl -s "http://127.0.0.1:$1/screenshot" -o "$OUT/$2.png"; }
tap()  { bash scripts/tap.sh "$1" "$2" >/dev/null; }
upload() {
  python3 - "$1" "$RAM" <<'EOF'
import sys, struct, requests
port, path = sys.argv[1], sys.argv[2]
data = open(path, "rb").read()
for off in range(0, len(data), 64):
    p = struct.pack("<H", off) + data[off:off + 64]
    apdu = bytes([0xB5, 0x62, 0, 0, len(p)]) + p
    requests.post(f"http://127.0.0.1:{port}/apdu", json={"data": apdu.hex()}, timeout=10)
EOF
}

# --- fresh emulators (delete any persisted NVM first) --------------------
pkill -f "speculos.*--api-port 500[12]" 2>/dev/null || true
sleep 1
rm -rf /tmp/presse-relay
nohup speculos --model flex --display headless --api-port 5001 --apdu-port 0 "$APP_ELF" >/tmp/rec-a.log 2>&1 &
nohup speculos --model flex --display headless --api-port 5002 --apdu-port 0 "$APP_ELF" >/tmp/rec-b.log 2>&1 &
for port in 5001 5002; do
  for _ in $(seq 1 40); do curl -s -o /dev/null "http://127.0.0.1:$port/events" && break; sleep 0.3; done
done
sleep 2

# --- empty libraries (both show the "Enclave Records" header) ------------
snap 5001 "a-empty"
snap 5002 "b-empty"

# --- sleeve upload + cut on A --------------------------------------------
# Sleeve must be uploaded before the cut: the cut hashes it into the album cert.
upload 5001
python3 relay/demo_steps.py cut "Random Access Memories" 5 > "$OUT/../out-cut.txt" 2>&1 &
PID=$!
sleep 2.5
snap 5001 "a-cut"
tap 5001 "Cut the master"
wait $PID
sleep 1
snap 5001 "a-home"        # A's library, now holding the RAM master

# --- pairing + SAS (the identical 4 words) -------------------------------
python3 relay/demo_steps.py pair > "$OUT/../out-pair.txt" 2>&1 &
PID=$!
sleep 3.5
snap 5001 "a-sas"
snap 5002 "b-sas"
tap 5001 "Words match"
tap 5002 "Words match"
wait $PID

# --- press + receive (carries the sleeve A->B) ---------------------------
python3 relay/demo_steps.py press > "$OUT/../out-press.txt" 2>&1 &
PID=$!
sleep 2.5
snap 5001 "a-press"
tap 5001 "Press this copy"
sleep 2.5
snap 5002 "b-receive"
tap 5002 "Receive it"
wait $PID
sleep 1.5

# --- A's master record card (4 left after the press) ---------------------
tap 5001 "Random Access"
sleep 1.5
snap 5001 "a-card"
tap 5001 "Back"
sleep 1

# --- B's pressing: page 1 (the cover that travelled) + page 2 (provenance)
tap 5002 "Random Access"
sleep 1.5
snap 5002 "b-card"
# forward chevron to the provenance page (Sleeve Verified / Edition Sealed)
curl -s -X POST http://127.0.0.1:5002/finger -H 'Content-Type: application/json' \
  -d '{"x":430,"y":550,"action":"press-and-release"}' >/dev/null
sleep 1.5
snap 5002 "b-prov"
tap 5002 "Back"

# --- offline verification (verdict for reference / the caption) ----------
python3 relay/demo_steps.py verify > "$OUT/../out-verify.txt" 2>&1 || true

echo "--- verify verdict ---"
cat "$OUT/../out-verify.txt"
echo "RAW FRAMES in $OUT"
pkill -f "speculos.*--api-port 500[12]" 2>/dev/null || true

# --- compose the GIF -----------------------------------------------------
python3 scripts/compose_gif.py
