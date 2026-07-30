#!/bin/bash
# Dry-run the piloted demo end to end against fresh emulators, using the same
# commands the user drives by hand: demo_steps.py per beat (auto-routes through
# the cockpit on :5050 when up) and tap.sh to play the finger. The flow is
# art (to A) -> cut -> pair -> press -> verify: the press now carries the
# sleeve A->B by itself, so there is NO manual re-upload to B and NO
# radar-fallback flash. Captures B's library the moment the pressing lands, as
# proof. Does NOT leave state behind: the caller resets with emu-up.sh.
set -e
source "$(dirname "$0")/../env.sh"
cd "$PRESSE_ROOT"
RAM=docs/art/ram-cover.bin
tap() { bash scripts/dev/tap.sh "$1" "$2" >/dev/null; }

echo "=== fresh emulators ==="
pkill -f "speculos.*--api-port 500[12]" 2>/dev/null || true
sleep 1
rm -rf /tmp/presse-relay
nohup speculos --model flex --display headless --api-port 5001 --apdu-port 0 "$APP_ELF" >/tmp/pc-a.log 2>&1 &
nohup speculos --model flex --display headless --api-port 5002 --apdu-port 0 "$APP_ELF" >/tmp/pc-b.log 2>&1 &
for port in 5001 5002; do
  for _ in $(seq 1 40); do curl -s -o /dev/null "http://127.0.0.1:$port/events" && break; sleep 0.3; done
done
sleep 2

echo "=== art (upload sleeve to A, blank + pre-cut) ==="
python3 relay/demo_steps.py art "$RAM"

echo "=== cut (tap Cut the master on A) ==="
python3 relay/demo_steps.py cut "Random Access Memories" 5 &
PID=$!; sleep 2.5; tap 5001 "Cut the master"; wait $PID

echo "=== pair (tap Words match on both) ==="
python3 relay/demo_steps.py pair &
PID=$!; sleep 3.5; tap 5001 "Words match"; tap 5002 "Words match"; wait $PID

echo "=== press (tap Press this copy on A, Receive it on B; sleeve rides along) ==="
python3 relay/demo_steps.py press &
PID=$!; sleep 2.5; tap 5001 "Press this copy"; sleep 2.5; tap 5002 "Receive it"; wait $PID

echo "=== B's library the instant the pressing landed (no manual step) ==="
# The cover must already be the RAM sleeve, not the generative radar.
sleep 1
curl -s "http://127.0.0.1:5002/screenshot" -o docs/screens/receiver-cover-on-press.png
echo "saved docs/screens/receiver-cover-on-press.png"

echo "=== verify ==="
python3 relay/demo_steps.py verify

echo "=== collection on A (tap Back) ==="
python3 relay/demo_steps.py collection a &
PID=$!; sleep 1.5; tap 5001 "Back"; wait $PID

echo "=== PILOT SEQUENCE OK ==="
pkill -f "speculos.*--api-port 500[12]" 2>/dev/null || true
