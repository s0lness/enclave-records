"""Handing a copy from one real Flex to another, this laptop as the relay.

Run inside WSL with both devices usbipd-attached, unlocked, presse app open:
    python3 relay/give.py            # paths[0] gives to paths[1]
    python3 relay/give.py --swap     # the other way round
    python3 relay/give.py --cancel   # take back paths[0]'s unsent promise

The pairing is the same four words as a press, and it is still the only thing
authenticating the channel: this script carries the bearer key across but never
sees it, because it is masked with a session key derived on the two devices.

**Interrupting this is safe.** The transfer is two-phase: the giver commits its
copy to this one recipient, and only the recipient's receipt erases it. Re-run
the command with the same two devices and it picks up wherever it stopped, with
no second confirmation on the giver. What it will not do is redirect a committed
copy to somebody else, which is what keeps the retry from being a double spend.

**And a promise is not yet a handover.** If the ceremony died between the
giver's tap and the key reaching the wire, the copy exists in exactly one place
and `--cancel` gives it back, on that device's own screen and with no second
device in the room. Once the key has flown the device refuses: from there the
copy may be in two places, and only the recipient's receipt ends the state.
"""

import argparse
import hashlib
import os
import sys
import threading

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tests"))

from hid_device import HidDevice, enumerate_ledgers  # noqa: E402
from presse_client import (  # noqa: E402
    INS_GIVE_CANCEL,
    INS_PAIR_COMMIT,
    INS_PAIR_FINISH,
    INS_PAIR_RESPOND,
    INS_PAIR_REVEAL,
    INS_PAIR_SAS,
    INS_TAKE_ACCEPT,
    INS_TAKE_CONFIRM,
    SW_KEY_FLOWN,
    SW_OK,
    Presse,
    apdu_hex,
    finish_give,
    give_offer,
    give_stage,
    split_sw,
    verify_chain,
    verify_possession,
)


class HardwarePresse(Presse):
    """UI gates block on the physical screen instead of emulator taps."""

    def cmd_gated(self, ins, data, button_text, wait_text):
        print(f"   >> look at {self.dev.name}: confirm on the device")
        return split_sw(self.dev.apdu(apdu_hex(ins, data)))


def gated_both(pa, pb, ins):
    """Fire a blocking UI APDU on both devices at once (the SAS step)."""
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


def fingerprint(devpub: bytes) -> str:
    """A device's public identity: the first 8 hex of SHA256(devpub), the same
    derivation the app shows on screen and stores as `committed_to`."""
    return hashlib.sha256(devpub).hexdigest()[:8].upper()


def cancel(giver: HardwarePresse):
    """Take back a promise the giver made but never delivered on.

    One device, no pairing: nothing crosses to anybody, so there is no channel
    to authenticate and no second human to involve. The device itself decides
    whether the promise is still takeable back, and this script only reports
    what it answered."""
    info = giver.get_info()
    if not info["has_pressing"]:
        sys.exit("this device holds no copy, so it has promised nothing.")
    if not info["committed"]:
        sys.exit("this device has promised nothing; its copy is already free to give.")
    print("== presse: taking back an undelivered promise ==")
    print("   >> confirm on the device.")
    _, sw = split_sw(giver.dev.apdu(apdu_hex(INS_GIVE_CANCEL)))
    if sw == SW_KEY_FLOWN:
        sys.exit(
            "refused: the key has already left this device.\n"
            "The copy may exist on the recipient too, so nothing here can un-promise it.\n"
            "Finish the handover with the device the row names (re-run give.py), or accept\n"
            "that the copy is stuck if that device is gone."
        )
    if sw != SW_OK:
        sys.exit(f"cancel refused ({sw}).")
    print("   the promise is off. The copy is usable and givable again.")
    verify_possession(giver, giver.cmd(0x40, p1=0))
    print("GENUINE: possession proven live, on the device that nearly gave it away.")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--swap", action="store_true",
                        help="reverse the roles: paths[1] gives to paths[0]")
    parser.add_argument("--no-cover", action="store_true",
                        help="skip carrying the sleeve (the taker falls back to generative art)")
    parser.add_argument("--cancel", action="store_true",
                        help="take back the giver's promise, if its key never left")
    args = parser.parse_args()

    paths = enumerate_ledgers()
    if args.cancel:
        if not paths:
            sys.exit("no Ledger device found (usbipd attached? unlocked? app open?)")
        if args.swap:
            paths = list(reversed(paths))
        cancel(HardwarePresse(HidDevice("Flex A (giver)", paths[0])))
        return
    if len(paths) < 2:
        sys.exit(f"need 2 Ledger devices, found {len(paths)} (usbipd attached? unlocked? app open?)")
    if args.swap:
        paths = list(reversed(paths))

    giver = HardwarePresse(HidDevice("Flex A (giver)", paths[0]))
    taker = HardwarePresse(HidDevice("Flex B (taker)", paths[1]))

    info_giver = giver.get_info()
    info_taker = taker.get_info()
    # The same 8-hex identity the devices show ("To device ...", "Device ID",
    # and the recipient named on a committed row). Printed on every run
    # because finishing an interrupted give means finding the one device the
    # giver's row names, and a device holding nothing has no record card to
    # show its own.
    print(f"   giver {fingerprint(info_giver['devpub'])}"
          f"   taker {fingerprint(info_taker['devpub'])}")
    if not info_giver["has_pressing"]:
        sys.exit("the giver holds no copy; press one onto it first (relay/demo.py)")
    # A taker already holding the copy means an earlier run got that far and only
    # the acknowledgement was lost; anything else is a device with no room.
    resuming = info_giver["committed"]
    taker_has_it = info_taker["has_pressing"]
    if taker_has_it and not resuming:
        sys.exit("the taker already holds a copy; a device holds one at a time")
    if resuming:
        print("== presse: resuming an interrupted give (the giver is committed) ==")
        print(f"   the giver's row names its recipient; it must read"
              f" {fingerprint(info_taker['devpub'])} or this is the wrong device.")

    print("== presse: pairing (this relay is untrusted) ==")
    commitment = giver.cmd(INS_PAIR_COMMIT)
    eb = taker.cmd(INS_PAIR_RESPOND, commitment)
    ea = giver.cmd(INS_PAIR_REVEAL, eb)
    taker.cmd(INS_PAIR_FINISH, ea)
    print("   >> BOTH screens now show 4 words. Compare them out loud.")
    print("   >> Tap 'Words match' on both ONLY if they are identical.")
    (_, sw_a), (_, sw_b) = gated_both(giver, taker, INS_PAIR_SAS)
    if sw_a != SW_OK or sw_b != SW_OK:
        sys.exit("pairing aborted on-device. Good reflex if the words differed.")
    print("   channel authenticated by two humans.")

    if not taker_has_it:
        print("== presse: give ==")
        give_stage(giver, taker, carry_art=not args.no_cover)

        # The taker is asked first, on a copy it has fully verified. Refusing
        # here costs nothing: the giver has not been asked and still holds it.
        print("   >> confirm the receive on the taker")
        _, sw = split_sw(taker.dev.apdu(apdu_hex(INS_TAKE_CONFIRM)))
        if sw != SW_OK:
            sys.exit(f"receive declined ({sw}); the giver still holds the copy.")

        if not resuming:
            print("   >> confirm the give on the giver. After this it is committed.")
        sealed = give_offer(giver)
        print("   committed. The copy is now deliverable to this taker and nobody else.")

        _, sw = split_sw(taker.dev.apdu(apdu_hex(INS_TAKE_ACCEPT, sealed)))
        if sw != SW_OK:
            sys.exit(f"accept refused ({sw}). Nothing is lost: re-run to retry.")

    finish_give(giver, taker)

    print("== presse: offline verification of the taker ==")
    pressing = taker.cmd(0x40, p1=0)
    album = taker.cmd(0x40, p1=1)
    result = verify_chain(album, pressing)
    verify_possession(taker, pressing)
    print(f"GENUINE: pressing {result['number']} of {result['edition']}"
          f" of \"{result['title']}\", now held by the taker, possession proven live.")


if __name__ == "__main__":
    main()
