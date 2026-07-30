#!/bin/bash
# Boot the app N times and report whether GET_INFO answers or the app exits.
#
# Which ELF gets checked is the whole point of this script, so it is not left to
# whichever env file happens to be sourced: `env.sh` hard-codes APP_DIR to the
# ../presse checkout, and a boot check that silently validates the *other*
# worktree's binary is worse than no boot check at all. Resolution order:
#
#   1. APP_ELF (or APP_DIR) exported by the caller  -- always wins;
#   2. env-video.sh beside this script, when present -- this worktree;
#   3. env.sh                                        -- the original checkout.
#
# PORT overrides the Speculos API port (default 5001) so a boot check can run
# beside a live emulator pair.
CALLER_APP_ELF="$APP_ELF"
CALLER_APP_DIR="$APP_DIR"
HERE="$(dirname "$0")"
if [ -f "$HERE/env-video.sh" ]; then
  source "$HERE/env-video.sh"
else
  source "$HERE/env.sh"
fi
if [ -n "$CALLER_APP_DIR" ]; then
  export APP_DIR="$CALLER_APP_DIR"
  export APP_ELF="$APP_DIR/target/flex/release/presse"
fi
[ -n "$CALLER_APP_ELF" ] && export APP_ELF="$CALLER_APP_ELF"

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
