"""Development provisioning: hand a device a pressing without a ceremony.

A pressing binds to its recipient and a device holds one at a time, so the
honest route to copy #15 is fourteen earlier presses to fourteen distinct
devices. With two Flex on a desk that is unreachable, and staging a scene should
not depend on it. So the relay plays the master here: it derives the album key,
signs both certificates, carries the sleeve, and pushes the result.

The device still verifies everything it would verify on a real receive (both
signatures, the album_id linkage, the edition match, the binding to its own
public key) and still refuses to overwrite a pressing it already holds. Only the
authorship of the press is fictional.

    ./scripts/provision.sh --number 15 --edition 20
    ./scripts/provision.sh --device b --title "Discovery" --number 3 --edition 12

The album key is derived from title/artist/edition, so the Edition ID on screen
is stable across takes and across an NVM wipe: re-provisioning after a reflash
reproduces the same edition rather than a look-alike.
"""

import argparse
import hashlib
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tests"))

from hid_device import HidDevice, enumerate_ledgers  # noqa: E402
from presse_client import (  # noqa: E402
    ART_LEN,
    SLOT_PRESSING,
    Presse,
    build_album_cert,
    build_pressing_cert,
    demo_album_key,
    demo_bearer_key,
    provision_pressing,
    upload_art,
    verify_chain,
)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--device", choices=["a", "b"], default="a",
                        help="which HID path to provision (a = paths[0])")
    parser.add_argument("--title", default="Random Access Memories")
    parser.add_argument("--artist", default="Daft Punk")
    parser.add_argument("--edition", type=int, default=20)
    parser.add_argument("--number", type=int, default=15)
    parser.add_argument("--cover", default="ram-cover.bin",
                        help="sleeve in docs/art/, sealed into the album cert")
    args = parser.parse_args()

    if not 0 < args.number <= args.edition:
        sys.exit(f"number {args.number} outside an edition of {args.edition}")

    paths = enumerate_ledgers()
    idx = 0 if args.device == "a" else 1
    if len(paths) <= idx:
        sys.exit(f"device {args.device} not found ({len(paths)} Flex attached)")
    dev = Presse(HidDevice(f"Flex {args.device.upper()}", paths[idx]))

    info = dev.get_info()
    if info["has_pressing"]:
        sys.exit("this device already holds a pressing; provisioning never overwrites one")

    cover_path = os.path.join(os.path.dirname(__file__), "..", "docs", "art", args.cover)
    with open(cover_path, "rb") as fh:
        art = fh.read()
    if len(art) != ART_LEN:
        sys.exit(f"{args.cover} is {len(art)} bytes, the device expects {ART_LEN}")
    sleeve_hash = hashlib.sha256(art).digest()

    priv, albpub = demo_album_key(args.title, args.artist, args.edition)
    album_cert = build_album_cert(
        priv, albpub, args.title, args.edition, sleeve_hash, args.artist
    )
    album_id = hashlib.sha256(albpub).digest()
    # The copy is bound to a bearer key, not to this device: provisioned or
    # pressed, a copy is transferable afterwards and nothing here should make
    # it less so. The relay mints that key and hands over both halves.
    bearer_priv, bearer_pub = demo_bearer_key(
        args.title, args.artist, args.edition, args.number
    )
    pressing_cert = build_pressing_cert(
        priv, album_id, args.number, args.edition, bearer_pub
    )

    # Verified here too, against the same rules the device applies: a relay bug
    # should fail on the laptop, not as an opaque status word on the device.
    result = verify_chain(album_cert, pressing_cert)
    assert result["holderpub"] == bearer_pub, "bearer key does not match the certificate"

    print(f'== provisioning Flex {args.device.upper()} ==')
    print(f'   "{args.title}" by {args.artist}, #{args.number} of {args.edition}')
    upload_art(dev, art, SLOT_PRESSING)
    print(f"   sleeve carried into the pressing slot ({args.cover}, {len(art)} bytes)")
    provision_pressing(dev, album_cert, pressing_cert, bearer_priv)
    print(f"   edition ID {hashlib.sha256(albpub).hexdigest()[:8].upper()}")
    print("   provisioned. The device verified both signatures and the bearer key.")


if __name__ == "__main__":
    main()
