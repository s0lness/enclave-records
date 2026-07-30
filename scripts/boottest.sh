#!/bin/bash
# Boot the app N times and report whether GET_INFO answers or the app exits.
#
# Which ELF gets checked is the whole point of this script. env.sh derives it from
# its own location, so the default is the checkout this script lives in, and a
# caller that exports APP_ELF (or APP_DIR) still wins. A boot check that silently
# validates another worktree's binary is worse than no boot check at all.
#
# PORT overrides the Speculos API port (default 5001) so a boot check can run
# beside a live emulator pair.
source "$(dirname "$0")/env.sh"

PORT="${PORT:-5001}"
if [ ! -f "$APP_ELF" ]; then
  echo "no ELF at $APP_ELF" >&2
  exit 1
fi
echo "checking $APP_ELF on port $PORT"
for i in 1 2 3; do
  pkill -f "speculos.*$PORT" 2>/dev/null; sleep 1
  nohup speculos --model flex --display headless --api-port "$PORT" --apdu-port 0 \
    "$APP_ELF" >"/tmp/boot$i.log" 2>&1 &
  sleep 6
  R=$(curl -s --max-time 5 "http://127.0.0.1:$PORT/apdu" -d '{"data":"b501000000"}' 2>/dev/null | head -c 16)
  E=$(grep -c "exit called" "/tmp/boot$i.log")
  echo "run $i: reply=[$R] exit_called=$E"
  pkill -f "speculos.*$PORT" 2>/dev/null
done
