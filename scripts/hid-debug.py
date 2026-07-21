"""List every HID device visible to hidapi, plus hidraw device nodes."""
import glob
import os

import hid

devs = hid.enumerate()
print(f"hidapi sees {len(devs)} devices")
for d in devs:
    print({k: (v.decode() if isinstance(v, bytes) else v) for k, v in d.items()})
for node in sorted(glob.glob("/dev/hidraw*")):
    st = os.stat(node)
    print(node, oct(st.st_mode))
