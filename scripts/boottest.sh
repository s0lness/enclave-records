#!/bin/bash
# Boot the app N times and report whether GET_INFO answers or the app exits.
#
# Which ELF gets checked is the whole point of this script. env.sh derives it from
# its own location, so the default is the checkout this script lives in, and a
# caller that exports APP_ELF (or APP_DIR) still wins. A boot check that silently
# validates another worktree's binary is worse than no boot check at all.
#
# Three outcomes per run, and only two of them are the app's verdict:
#
#   ok     GET_INFO answered.
#   panic  the Speculos log carries "exit called": the app installed, panicked
#          before its first APDU (`exiting_panic` -> `exit_app(0)`) and answered
#          nothing. Read off the log rather than inferred from silence, so a
#          panic and a slow start stop looking alike.
#   slow   neither, inside the deadline. Speculos start-up under load says
#          nothing about the app, so a slow run is retried rather than counted.
#          A fixed sleep followed by a single request counted those as failures,
#          which is how an emulator hiccup gets read as a property of the build.
#
# PORT overrides the Speculos API port (default 5001) so a boot check can run
# beside a live emulator pair. RUNS, DEADLINE and RETRIES are the other knobs.
source "$(dirname "$0")/env.sh"

PORT="${PORT:-5001}"
RUNS="${RUNS:-3}"
DEADLINE="${DEADLINE:-25}"
RETRIES="${RETRIES:-3}"

if [ ! -f "$APP_ELF" ]; then
  echo "no ELF at $APP_ELF" >&2
  exit 1
fi

# A load size that is a multiple of 4096 fails here for a reason that has
# nothing to do with the app: stock Speculos maps one page of .nvm_data and the
# storage flags fall outside it. Say so up front instead of reporting 0/3.
presse_check_load_size || exit 1

boot_once() {
  pkill -f "speculos.*$PORT" 2>/dev/null; sleep 1
  nohup speculos --model flex --display headless --api-port "$PORT" --apdu-port 0 \
    "$APP_ELF" >"/tmp/boot-$PORT.log" 2>&1 &
  local verdict=slow
  for _ in $(seq 1 "$DEADLINE"); do
    sleep 1
    if grep -q "exit called" "/tmp/boot-$PORT.log"; then
      verdict=panic
      break
    fi
    if [ -n "$(curl -s --max-time 2 "http://127.0.0.1:$PORT/apdu" \
      -d '{"data":"b501000000"}' 2>/dev/null)" ]; then
      verdict=ok
      break
    fi
  done
  pkill -f "speculos.*$PORT" 2>/dev/null
  echo "$verdict"
}

echo "checking $APP_ELF on port $PORT"
OK=0
for i in $(seq 1 "$RUNS"); do
  for _ in $(seq 1 "$RETRIES"); do
    V=$(boot_once)
    [ "$V" != "slow" ] && break
  done
  echo "run $i: $V"
  [ "$V" = "ok" ] && OK=$((OK + 1))
done
echo "boot $OK/$RUNS ok"
