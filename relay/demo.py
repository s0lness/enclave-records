"""The live M5 demo: two real Flexes, this laptop as the untrusted relay.

Run inside WSL with both devices usbipd-attached, unlocked, presse app open:
    python3 relay/demo.py            # full ceremony
    python3 relay/demo.py --verify   # offline verification only

Every confirmation happens on the device screens; this script only carries
bytes and narrates. It never sees a key and could lie: that's the point of
the word-comparison step."""

import argparse
import os
import sys
import threading

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tests"))

from hid_device import HidDevice, enumerate_ledgers  # noqa: E402
from presse_client import (  # noqa: E402
    Presse,
    apdu_hex,
    split_sw,
    verify_chain,
    verify_possession,
    INS_PAIR_COMMIT,
    INS_PAIR_RESPOND,
    INS_PAIR_REVEAL,
    INS_PAIR_FINISH,
    INS_PAIR_SAS,
    INS_GET_ALBUM,
    INS_PRESS_REQUEST,
    INS_PRESS_OFFER,
    INS_PRESS_LOAD_ALBUM,
    INS_PRESS_ACCEPT,
    INS_CUT,
    SW_OK,
)
import struct  # noqa: E402


class HardwarePresse(Presse):
    """UI gates block on the physical screen instead of emulator taps."""

    def cmd_gated(self, ins, data, button_text, wait_text):
        print(f"   >> look at {self.dev.name}: confirm on the device")
        resp = self.dev.apdu(apdu_hex(ins, data))
        return split_sw(resp)


def gated_both(pa, pb, ins):
    """Fire a blocking UI APDU on both devices at once (SAS step)."""
    results = {}

    def run(p, key):
        results[key] = p.dev.apdu(apdu_hex(ins))

    ta = threading.Thread(target=run, args=(pa, "a"), daemon=True)
    tb = threading.Thread(target=run, args=(pb, "b"), daemon=True)
    ta.start()
    tb.start()
    ta.join()
    tb.join()
    return split_sw(results["a"]), split_sw(results["b"])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--verify", action="store_true", help="offline verification only")
    parser.add_argument("--collection", choices=["a", "b"],
                        help="browse the collection on device a or b (tap Back to leave)")
    parser.add_argument("--title", default="Random Access Memories")
    parser.add_argument("--edition", type=int, default=5)
    args = parser.parse_args()

    paths = enumerate_ledgers()
    if args.collection:
        idx = 0 if args.collection == "a" else 1
        if len(paths) <= idx:
            sys.exit(f"device {args.collection} not found")
        dev = HardwarePresse(HidDevice(f"Flex {args.collection.upper()}", paths[idx]))
        print(f">> browse the collection on Flex {args.collection.upper()}; tap Back to leave")
        _, sw = split_sw(dev.dev.apdu(apdu_hex(0x02)))
        print("closed." if sw == SW_OK else f"refused ({sw})")
        return

    if args.verify:
        if len(paths) < 1:
            sys.exit("no Ledger device found")
        holder = HardwarePresse(HidDevice("holder", paths[0]))
        print("== presse: offline verification (no network used) ==")
        pressing = holder.cmd(0x40, p1=0)
        album = holder.cmd(0x40, p1=1)
        info = holder.get_info()
        result = verify_chain(album, pressing, info["devpub"])
        verify_possession(holder, pressing)
        print(f"GENUINE: pressing {result['number']} of {result['edition']}"
              f" of \"{result['title']}\", bound to this device, key possession proven live.")
        return

    if len(paths) < 2:
        sys.exit(f"need 2 Ledger devices, found {len(paths)} (usbipd attached? unlocked? app open?)")

    a = HardwarePresse(HidDevice("Flex A (master)", paths[0]))
    b = HardwarePresse(HidDevice("Flex B (receiver)", paths[1]))

    print("== presse: cut ==")
    info_a = a.get_info()
    if info_a["has_master"]:
        print(f'   master already cut: "{info_a["title"]}", {info_a["counter"]} pressings left')
    else:
        print(f'   cutting master of "{args.title}", edition of {args.edition}')
        data = struct.pack("<H", args.edition) + args.title.encode()
        print("   >> confirm on Flex A")
        body, sw = split_sw(a.dev.apdu(apdu_hex(INS_CUT, data)))
        assert sw == SW_OK, f"cut refused: {sw}"
        print("   master cut. The edition size is now physics.")

    print("== presse: pairing (this relay is untrusted) ==")
    commitment = a.cmd(INS_PAIR_COMMIT)
    eb = b.cmd(INS_PAIR_RESPOND, commitment)
    ea = a.cmd(INS_PAIR_REVEAL, eb)
    b.cmd(INS_PAIR_FINISH, ea)
    print("   >> BOTH screens now show 4 words. Compare them out loud.")
    print("   >> Tap 'Words match' on both ONLY if they are identical.")
    (sas_a, sw_a), (sas_b, sw_b) = gated_both(a, b, INS_PAIR_SAS)
    if sw_a != SW_OK or sw_b != SW_OK:
        sys.exit("pairing aborted on-device. Good reflex if the words differed.")
    print("   channel authenticated by two humans.")

    print("== presse: press ==")
    album_msg = a.cmd(INS_GET_ALBUM)
    req = b.cmd(INS_PRESS_REQUEST)
    print("   >> confirm the press on Flex A")
    cert_mac, sw = split_sw(a.dev.apdu(apdu_hex(INS_PRESS_OFFER, req)))
    assert sw == SW_OK, f"press refused: {sw}"
    b.cmd(INS_PRESS_LOAD_ALBUM, album_msg)
    print("   >> confirm the receive on Flex B")
    _, sw = split_sw(b.dev.apdu(apdu_hex(INS_PRESS_ACCEPT, cert_mac)))
    assert sw == SW_OK, f"receive refused: {sw}"

    info_a = a.get_info()
    print(f"   pressed. {info_a['counter']} of {info_a['edition']} remain in the master.")

    print("== presse: offline verification of Flex B ==")
    pressing = b.cmd(0x40, p1=0)
    album = b.cmd(0x40, p1=1)
    info_b = b.get_info()
    result = verify_chain(album, pressing, info_b["devpub"])
    verify_possession(b, pressing)
    print(f"GENUINE: pressing {result['number']} of {result['edition']}"
          f" of \"{result['title']}\". No server, no chain, no trust in this laptop.")


if __name__ == "__main__":
    main()
