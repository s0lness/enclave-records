#!/bin/bash
# Pre-flight cerémonie, LECTURE SEULE: liste les Flex vus en HID et interroge
# GET_INFO (0x01) sur chacun. Aucune commande UI-gated, aucun master consommé.
# Sortie: l'ordre paths[0]=A / paths[1]=B tel que relay/demo.py le verra.
set -e
export PATH=~/venv-ledger/bin:$PATH
cd /mnt/c/Users/sylve/projects/presse-video
python3 - <<'EOF'
import hashlib
import sys
sys.path.insert(0, "relay")
sys.path.insert(0, "tests")
from hid_device import HidDevice, enumerate_ledgers
from presse_client import Presse

paths = enumerate_ledgers()
print(f"{len(paths)} Flex vu(s) en HID")
if len(paths) < 2:
    print("!! la ceremonie exige 2 chemins HID (relancer attach-usb.ps1)")
for i, p in enumerate(paths):
    role = "A (master)" if i == 0 else ("B (receiver)" if i == 1 else f"#{i}")
    info = Presse(HidDevice(f"Flex {role}", p)).get_info()
    print(f"  paths[{i}] = {role}")
    print(f"    has_master   : {info['has_master']}")
    print(f"    has_pressing : {info['has_pressing']}")
    if info["has_master"]:
        print(f"    master       : \"{info['title']}\", {info['counter']}/{info['edition']} restants")
    # Même empreinte que l'écran du Flex ("Device ID", "For device XXXXXXXX"):
    # SHA256(devpub)[:4].
    fp = hashlib.sha256(info["devpub"]).digest()[:4].hex().upper()
    print(f"    Device ID    : {fp}")
EOF
