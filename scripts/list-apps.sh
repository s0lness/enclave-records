#!/bin/bash
# List the apps installed on the single attached Ledger device.
source "$(dirname "$0")/env.sh"
python3 - <<'EOF'
from ledgerwallet.client import LedgerClient
from ledgerwallet.transport import enumerate_devices
devs = list(enumerate_devices())
print(f"{len(devs)} device(s) attached")
if not devs:
    raise SystemExit(1)
c = LedgerClient(devs[0])
apps = c.apps() if callable(getattr(c, "apps", None)) else c.apps
for app in apps:
    name = getattr(app, "name", None) or (app.get("name") if isinstance(app, dict) else str(app))
    print(" APP:", name)
EOF
