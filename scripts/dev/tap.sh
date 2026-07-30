#!/bin/bash
# tap.sh <port> <button text>  - tap the on-screen element whose OCR text
# contains the given string. Retries for up to 8s while the screen draws.
PORT="$1"; shift
NEEDLE="$*"
python3 - "$PORT" "$NEEDLE" <<'EOF'
import sys, time, requests
port, needle = sys.argv[1], sys.argv[2]
url = f"http://127.0.0.1:{port}"
deadline = time.time() + 8
while time.time() < deadline:
    for e in requests.get(f"{url}/events", timeout=5).json().get("events", []):
        if needle in e.get("text", "") and "x" in e:
            requests.post(f"{url}/finger", json={"x": e["x"], "y": e["y"], "action": "press-and-release"}, timeout=5)
            print(f"tapped '{e['text']}' at ({e['x']},{e['y']})")
            sys.exit(0)
    time.sleep(0.4)
sys.exit(f"no '{needle}' on screen after 8s")
EOF
