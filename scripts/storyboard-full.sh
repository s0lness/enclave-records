#!/bin/bash
# Full demo capture: every beat of the ceremony, both screens, wire log.
# Requires emu-up.sh + cockpit.sh already running.
set -e
source "$(dirname "$0")/env.sh"
cd /mnt/c/Users/sylve/projects/presse
OUT=docs/screens/full-demo
mkdir -p "$OUT"
snap() { curl -s "http://127.0.0.1:$1/screenshot" -o "$OUT/$2.png"; }

snap 5001 "01-a-home"
snap 5002 "02-b-home"

python3 relay/demo_steps.py cut "Random Access Memories" 5 > "$OUT/out-cut.txt" 2>&1 &
PID=$!
sleep 2.5
snap 5001 "03-a-cut-review"
bash scripts/tap.sh 5001 "Cut the master" >/dev/null
wait $PID
sleep 1
snap 5001 "04-a-home-after-cut"

python3 relay/demo_steps.py pair > "$OUT/out-pair.txt" 2>&1 &
PID=$!
sleep 3.5
snap 5001 "05-a-sas"
snap 5002 "06-b-sas"
bash scripts/tap.sh 5001 "Words match" >/dev/null
bash scripts/tap.sh 5002 "Words match" >/dev/null
wait $PID

python3 relay/demo_steps.py press > "$OUT/out-press.txt" 2>&1 &
PID=$!
sleep 2.5
snap 5001 "07-a-press-offer"
bash scripts/tap.sh 5001 "Press this copy" >/dev/null
sleep 2.5
snap 5002 "08-b-receive"
bash scripts/tap.sh 5002 "Receive it" >/dev/null
wait $PID

python3 relay/demo_steps.py verify > "$OUT/out-verify.txt" 2>&1
curl -s "http://127.0.0.1:5050/log?after=0" -o "$OUT/wire.json"
cat "$OUT"/out-*.txt
echo "CAPTURED in $OUT"
