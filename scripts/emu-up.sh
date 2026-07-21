#!/bin/bash
# Start two persistent Speculos Flexes (A on :5001, B on :5002) for the visual
# demo. View at http://localhost:5001 and :5002. Stop with emu-down.sh.
source "$(dirname "$0")/env.sh"
pkill -f "speculos.*--api-port 500[12]" 2>/dev/null
sleep 0.5
nohup speculos --model flex --display headless --api-port 5001 --apdu-port 0 "$APP_ELF" >/tmp/speculos-a.log 2>&1 &
echo $! > /tmp/speculos-a.pid
nohup speculos --model flex --display headless --api-port 5002 --apdu-port 0 "$APP_ELF" >/tmp/speculos-b.log 2>&1 &
echo $! > /tmp/speculos-b.pid
for port in 5001 5002; do
  for i in $(seq 1 40); do
    curl -s -o /dev/null "http://127.0.0.1:$port/events" && break
    sleep 0.3
  done
done
echo "Flex A (master):   http://localhost:5001"
echo "Flex B (receiver): http://localhost:5002"
