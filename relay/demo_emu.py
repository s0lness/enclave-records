"""Visual emulated demo: two Speculos Flexes, watchable (and tappable) in a
browser. Run inside WSL:

    python3 relay/demo_emu.py           # you tap on the web UIs
    python3 relay/demo_emu.py --auto    # taps itself (rehearsal mode)

Then open, on the Windows side:
    http://localhost:5001   (Flex A, the master)
    http://localhost:5002   (Flex B, the receiver)

The script is the untrusted relay; the browser pages are the two "devices in
your hands"."""

import argparse
import os
import struct
import sys
import threading
import time

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tests"))

from conftest import SpeculosDevice  # noqa: E402
from presse_client import (  # noqa: E402
    Presse,
    apdu_hex,
    split_sw,
    sas_words_on_screen,
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

AUTO_TAP_DELAY = 2.0


class Narrated(Presse):
    def __init__(self, device, auto: bool):
        super().__init__(device)
        self.auto = auto

    def gated(self, ins, data, button_text, wait_text, prompt):
        thread, result = self.dev.apdu_async_start(apdu_hex(ins, data))
        assert self.dev.wait_for_text(wait_text), f"never saw '{wait_text}'"
        if self.auto:
            time.sleep(AUTO_TAP_DELAY)
            self.tap_text(button_text)
        else:
            print(f"   >> {prompt}")
        thread.join(timeout=600)
        assert "data" in result, "confirmation never arrived"
        return split_sw(result["data"])


def gated_both(pa, pb, ins, button_text, wait_text, prompt):
    results = {}

    def run(p, key):
        t, r = p.dev.apdu_async_start(apdu_hex(ins))
        t.join(timeout=600)
        results[key] = r

    ta = threading.Thread(target=run, args=(pa, "a"), daemon=True)
    tb = threading.Thread(target=run, args=(pb, "b"), daemon=True)
    ta.start()
    tb.start()
    assert pa.dev.wait_for_text(wait_text) and pb.dev.wait_for_text(wait_text)

    words_a = sas_words_on_screen(pa.dev)
    words_b = sas_words_on_screen(pb.dev)
    print(f"   Flex A shows: {' / '.join(words_a)}")
    print(f"   Flex B shows: {' / '.join(words_b)}")
    if pa.auto:
        time.sleep(AUTO_TAP_DELAY)
        pa.tap_text(button_text)
        pb.tap_text(button_text)
    else:
        print(f"   >> {prompt}")
    ta.join(timeout=600)
    tb.join(timeout=600)
    return split_sw(results["a"]["data"]), split_sw(results["b"]["data"])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--auto", action="store_true", help="tap the screens automatically")
    parser.add_argument("--title", default="Nuits Roses")
    parser.add_argument("--edition", type=int, default=5)
    args = parser.parse_args()

    print("booting two Flexes (Speculos)...")
    a_dev = SpeculosDevice("flex-a", 5001)
    b_dev = SpeculosDevice("flex-b", 5002)
    a = Narrated(a_dev, args.auto)
    b = Narrated(b_dev, args.auto)
    print("   Flex A (master):   http://localhost:5001")
    print("   Flex B (receiver): http://localhost:5002")
    if not args.auto:
        input("open both pages, then press Enter to start the ceremony... ")

    try:
        print("\n== cut the master ==")
        data = struct.pack("<H", args.edition) + args.title.encode()
        _, sw = a.gated(INS_CUT, data, "Cut the master", "Cut master",
                        "on Flex A's page: tap 'Cut the master'")
        assert sw == SW_OK, f"cut refused ({sw})"
        print(f'   master of "{args.title}" cut, edition of {args.edition}: now physics.')

        print("\n== pairing (this script is the untrusted relay) ==")
        commitment = a.cmd(INS_PAIR_COMMIT)
        eb = b.cmd(INS_PAIR_RESPOND, commitment)
        ea = a.cmd(INS_PAIR_REVEAL, eb)
        b.cmd(INS_PAIR_FINISH, ea)
        (_, sw_a), (_, sw_b) = gated_both(
            a, b, INS_PAIR_SAS, "Words match", "Words match",
            "compare the words on the two pages; tap 'Words match' on BOTH")
        assert sw_a == SW_OK and sw_b == SW_OK, "pairing aborted on-device"
        print("   channel authenticated by the word comparison.")

        print("\n== press 1 of {} ==".format(args.edition))
        album_msg = a.cmd(INS_GET_ALBUM)
        req = b.cmd(INS_PRESS_REQUEST)
        cert_mac, sw = a.gated(INS_PRESS_OFFER, req, "Press this copy", "Press ",
                               "on Flex A's page: tap 'Press this copy'")
        assert sw == SW_OK, f"press refused ({sw})"
        b.cmd(INS_PRESS_LOAD_ALBUM, album_msg)
        _, sw = b.gated(INS_PRESS_ACCEPT, cert_mac, "Receive it", "Receive ",
                        "on Flex B's page: tap 'Receive it'")
        assert sw == SW_OK, f"receive refused ({sw})"
        print(f"   pressed. {a.get_info()['counter']} remain in the master.")

        print("\n== offline verification of Flex B ==")
        pressing = b.cmd(0x40, p1=0)
        album = b.cmd(0x40, p1=1)
        info_b = b.get_info()
        result = verify_chain(album, pressing, info_b["devpub"])
        verify_possession(b, pressing)
        print(f'GENUINE: pressing {result["number"]} of {result["edition"]} of '
              f'"{result["title"]}", bound to Flex B, possession proven live.\n'
              "No server, no chain, no trust in this relay.")
        if not args.auto:
            input("press Enter to shut the devices down... ")
    finally:
        a_dev.stop()
        b_dev.stop()


if __name__ == "__main__":
    main()
