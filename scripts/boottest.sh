#!/bin/bash
# Boot the app N times and report whether GET_INFO answers or the app exits.
source "$(dirname "$0")/env.sh"
for i in 1 2 3; do
  pkill -f "speculos.*5001" 2>/dev/null; sleep 1
  nohup speculos --model flex --display headless --api-port 5001 --apdu-port 0 \
    "$APP_ELF" >"/tmp/boot$i.log" 2>&1 &
  sleep 6
  R=$(curl -s --max-time 5 http://127.0.0.1:5001/apdu -d '{"data":"b501000000"}' 2>/dev/null | head -c 16)
  E=$(grep -c "exit called" "/tmp/boot$i.log")
  echo "run $i: reply=[$R] exit_called=$E"
  pkill -f "speculos.*5001" 2>/dev/null
done
