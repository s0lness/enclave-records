"""Sweep os_factory_setting_get ids through FACTORY_PROBE (0x69) and report,
per id, what the OS answered.

Needs a build carrying the `factprobe` feature (scripts/build.sh -- --features
factprobe); a default build answers 0x6D00 to every id, which is the check that
the probe is absent from a shipped binary.

Two things the raw bytes cannot tell you unless the probe is read carefully:

- Speculos does not implement this syscall at all. It answers 0 and leaves the
  buffer alone, so every id "succeeds" with an empty answer. The probe fills its
  buffer with 0xA5 first, so an untouched buffer is visible as such; a sweep that
  reports nothing but poison has learned nothing about the OS, only about the
  emulator. Run it there to rehearse the plumbing, never to conclude.
- an id the OS refuses may throw. The probe catches the throw and reports the
  exception code instead of dying, so a sweep does not cost one physical app
  relaunch per bad id. If the app dies anyway (no answer, HID error), the sweep
  stops and says where to resume.

Results go OUTSIDE the repo by default: whatever a factory setting turns out to
be, it is device-identifying material and does not belong in git.

  python3 scripts/dev/factory-sweep.py --speculos          # emulator rehearsal
  python3 scripts/dev/factory-sweep.py --device 0          # first HID Flex
  python3 scripts/dev/factory-sweep.py --device 0 --resume # after a relaunch
"""

import argparse
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "relay"))

CLA = 0xB5
INS_FACTORY_PROBE = 0x69
POISON = 0xA5

# The id space to try, and why it stops where it does. The syscall takes a
# 32-bit id, so exhaustion is not on the table; what is on the table is the
# shape BOLOS actually uses. Its sibling `os_setting_get` indexes a dense enum
# of about a dozen values, and the factory notion the OS export exposes is
# numbered slots (FACTORY_SETTINGS_SLOT_1/2), so a low dense range is where a
# real id would live. 0..255 covers that with room to spare; the extras probe
# the alternative shape, a tagged or bit-flagged id, at the boundaries where one
# would show up. An id outside both shapes is not findable by sweeping at all.
DENSE_IDS = list(range(0, 256))
STRUCTURED_IDS = [
    0x0100, 0x0101, 0x0102, 0x0200, 0x0201, 0x1000, 0x1001,
    0x8000, 0x8001, 0xFFFF, 0x10000, 0x1000000, 0xFFFFFFFF,
]
# Reserved by the probe: throws inside its own try context to prove the catch
# works on this device before the sweep leans on it.
SELFTEST_ID = 0xDEADBEEF
SELFTEST_EX = 0x1234

REQUEST_LEN = 64


def probe(dev, setting_id: int, maxlen: int = REQUEST_LEN) -> dict:
    """One FACTORY_PROBE call. Returns the decoded reply."""
    body = setting_id.to_bytes(4, "little") + bytes([maxlen])
    apdu = bytes([CLA, INS_FACTORY_PROBE, 0, 0, len(body)]) + body
    reply = bytes.fromhex(dev.apdu(apdu.hex()))
    sw = int.from_bytes(reply[-2:], "big")
    if sw != 0x9000:
        return {"id": setting_id, "sw": sw}
    payload = reply[:-2]
    caught = payload[0]
    ex = int.from_bytes(payload[1:3], "little")
    ret = int.from_bytes(payload[3:7], "little")
    length = payload[7]
    buf = payload[8 : 8 + length]
    written = len(buf) - len(buf.rstrip(bytes([POISON])))
    return {
        "id": setting_id,
        "sw": sw,
        "caught": caught,
        "ex": ex,
        "ret": ret,
        "requested": length,
        # How many leading bytes are not poison: the OS's actual footprint,
        # independent of whatever length it claims to have returned.
        "touched": length - written,
        "bytes": buf.hex(),
    }


def interesting(r: dict) -> bool:
    """An answer worth a human's attention: the OS wrote something."""
    return r.get("sw") == 0x9000 and not r.get("caught") and r.get("touched", 0) > 0


def describe(r: dict) -> str:
    if r.get("sw") != 0x9000:
        return f"SW {r['sw']:04X}"
    if r.get("caught"):
        return f"threw {r['ex']:#06x}"
    if r["touched"] == 0:
        return f"ret={r['ret']} buffer untouched"
    head = r["bytes"][: 2 * min(8, r["touched"])]
    shape = ""
    first = int(r["bytes"][0:2], 16) if r["touched"] else 0
    if r["touched"] >= 65 and first == 0x04:
        shape = "  <- uncompressed EC point shape"
    elif r["touched"] >= 2 and first == 0x30:
        shape = "  <- DER shape"
    return f"ret={r['ret']} touched={r['touched']} head={head}...{shape}"


def open_device(args):
    if args.speculos:
        sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tests"))

        import requests

        class SpeculosDevice:
            def __init__(self, port):
                self.url = f"http://127.0.0.1:{port}/apdu"

            def apdu(self, hexstr):
                r = requests.post(self.url, json={"data": hexstr}, timeout=30)
                return r.json()["data"]

        return SpeculosDevice(args.port)

    from hid_device import HidDevice, enumerate_ledgers

    paths = enumerate_ledgers()
    if not paths:
        raise SystemExit("no Ledger seen in HID")
    if args.device >= len(paths):
        raise SystemExit(f"device {args.device} out of range ({len(paths)} seen)")
    return HidDevice(f"Flex {args.device}", paths[args.device])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--speculos", action="store_true", help="drive the emulator instead of HID")
    ap.add_argument("--port", type=int, default=5001, help="Speculos API port")
    ap.add_argument("--device", type=int, default=0, help="index into the HID Ledgers")
    ap.add_argument(
        "--out",
        default=os.path.join(os.path.expanduser("~"), "factory-sweep.json"),
        help="results file, deliberately outside the repo",
    )
    ap.add_argument("--resume", action="store_true", help="skip ids already in --out")
    args = ap.parse_args()

    out = Path(args.out)
    results = {}
    if args.resume and out.exists():
        results = {int(k): v for k, v in json.loads(out.read_text()).items()}
        print(f"resuming, {len(results)} ids already done")

    dev = open_device(args)

    if SELFTEST_ID not in results:
        r = probe(dev, SELFTEST_ID)
        ok = r.get("caught") == 1 and r.get("ex") == SELFTEST_EX
        print(f"catch self-test: {'OK' if ok else 'FAILED'} ({describe(r)})")
        if not ok:
            raise SystemExit(
                "the probe's try context is not catching; a bad id would kill the app"
            )
        results[SELFTEST_ID] = r

    todo = [i for i in DENSE_IDS + STRUCTURED_IDS if i not in results]
    hits, threw = [], 0
    try:
        for setting_id in todo:
            r = probe(dev, setting_id)
            results[setting_id] = r
            if r.get("caught"):
                threw += 1
            if interesting(r):
                hits.append(setting_id)
                print(f"id {setting_id:#010x}: {describe(r)}")
    except Exception as exc:  # a dead app looks like an IO error on this pipe
        print(f"\nSTOPPED at id {setting_id:#010x}: {exc}")
        print("if the device left the app, relaunch it by hand and re-run with --resume")
    finally:
        out.write_text(json.dumps({str(k): v for k, v in results.items()}, indent=1))

    done = len(results) - 1
    print(f"\n{done} ids probed, {threw} threw (caught), {len(hits)} wrote bytes")
    print(f"hits: {[hex(i) for i in hits] if hits else 'none'}")
    print(f"full answers in {out} (outside the repo, keep it there)")


if __name__ == "__main__":
    main()
