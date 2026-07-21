#!/bin/bash
# Run the full ceremony against the persistent emulators, screenshotting every
# beat into docs/screens/. Requires emu-up.sh already running.
set -e
source "$(dirname "$0")/env.sh"
cd /mnt/c/Users/sylve/projects/presse
OUT=docs/screens
mkdir -p "$OUT"
snap() { curl -s "http://127.0.0.1:$1/screenshot" -o "$OUT/$2.png"; }

snap 5001 "01-home-a"

python3 relay/demo_steps.py cut "Random Access Memories" 5 &
CUT_PID=$!
sleep 2
snap 5001 "02-cut-review-a"
bash scripts/tap.sh 5001 "Cut the master"
wait $CUT_PID

python3 relay/demo_steps.py pair &
PAIR_PID=$!
sleep 3
snap 5001 "03-sas-a"
snap 5002 "04-sas-b"
bash scripts/tap.sh 5001 "Words match"
bash scripts/tap.sh 5002 "Words match"
wait $PAIR_PID

python3 relay/demo_steps.py press &
PRESS_PID=$!
sleep 2
snap 5001 "05-press-offer-a"
bash scripts/tap.sh 5001 "Press this copy"
sleep 2
snap 5002 "06-press-accept-b"
bash scripts/tap.sh 5002 "Receive it"
wait $PRESS_PID

snap 5001 "07-home-a-after"
python3 relay/demo_steps.py verify
echo "storyboard in $OUT/"
