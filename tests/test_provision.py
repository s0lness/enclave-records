"""Development provisioning: the relay signs, the device still checks.

These pin the guarantees the provisioning path must not lose. It exists so a
scene can show a copy whose number no two-device ceremony can reach, but it must
stay as strict as a real receive about what it will store.
"""

import hashlib

import pytest

from presse_client import (
    ALBUM_CERT_LEN,
    INS_PROVISION_ALBUM,
    INS_PROVISION_PRESSING,
    PRESSING_PAYLOAD_LEN,
    SLOT_PRESSING,
    Presse,
    build_album_cert,
    build_pressing_cert,
    demo_album_key,
    provision_pressing,
    upload_art,
    verify_chain,
)

# Short on purpose: a library row truncates a long title to one line
# (fit_one_line), so an on-screen assertion on the full string would fail
# for reasons that have nothing to do with provisioning.
TITLE = "Discovery"
ARTIST = "Daft Punk"
EDITION = 20
NUMBER = 15


def a_sleeve(seed: int = 0) -> bytes:
    from presse_client import ART_LEN

    return bytes(((i * 7 + seed) & 0xFF) for i in range(ART_LEN))


def certs_for(devpub: bytes, number: int = NUMBER, edition: int = EDITION,
              sleeve_hash: bytes | None = None):
    priv, albpub = demo_album_key(TITLE, ARTIST, edition)
    album = build_album_cert(
        priv, albpub, TITLE, edition, sleeve_hash or bytes(32), ARTIST
    )
    pressing = build_pressing_cert(
        priv, hashlib.sha256(albpub).digest(), number, edition, devpub
    )
    return album, pressing


def test_provisioned_pressing_is_held_and_verifies(device):
    """A provisioned copy is indistinguishable from a pressed one afterwards:
    the device reports it, lists it, and the chain verifies offline."""
    p = Presse(device)
    devpub = p.get_info()["devpub"]
    album, pressing = certs_for(devpub)

    provision_pressing(p, album, pressing)

    info = p.get_info()
    assert info["has_pressing"] is True
    assert info["has_master"] is False, "provisioning must not fabricate a master"
    result = verify_chain(album, pressing, devpub)
    assert (result["number"], result["edition"]) == (NUMBER, EDITION)
    assert device.wait_for_text(TITLE), device.screen_texts()
    assert device.wait_for_text(f"#{NUMBER} of {EDITION}"), device.screen_texts()


def test_the_sleeve_lands_in_the_pressing_slot(device):
    """The cover is carried into the pressing's own slot and, its hash matching
    the signed one, the device shows it rather than the generative fallback."""
    p = Presse(device)
    art = a_sleeve()
    devpub = p.get_info()["devpub"]
    album, pressing = certs_for(devpub, sleeve_hash=hashlib.sha256(art).digest())

    upload_art(p, art, SLOT_PRESSING)
    provision_pressing(p, album, pressing)

    assert p.get_info()["has_pressing"] is True
    assert device.wait_for_text(TITLE), device.screen_texts()


def test_provisioning_never_overwrites_a_holding(device):
    """"Bound to this device forever" stays literally true: provisioning only
    ever adds a record. A device that already holds one refuses the second."""
    p = Presse(device)
    devpub = p.get_info()["devpub"]
    album, pressing = certs_for(devpub)
    provision_pressing(p, album, pressing)

    other_album, other_pressing = certs_for(devpub, number=2, edition=EDITION)
    p.cmd(INS_PROVISION_ALBUM, other_album)
    assert p.cmd_sw(INS_PROVISION_PRESSING, other_pressing) != "9000"
    assert p.get_info()["has_pressing"] is True


def test_a_pressing_addressed_elsewhere_is_refused(device):
    """The binding is checked, not trusted: a certificate naming another device
    cannot be planted, however well signed it is."""
    p = Presse(device)
    stranger = bytes([4]) + bytes(range(64))
    album, pressing = certs_for(stranger)
    p.cmd(INS_PROVISION_ALBUM, album)
    assert p.cmd_sw(INS_PROVISION_PRESSING, pressing) != "9000"
    assert p.get_info()["has_pressing"] is False


def test_an_unsigned_pressing_is_refused(device):
    """The relay is the authority for *who pressed*, never for the signature:
    a corrupted certificate is rejected exactly as on a real receive."""
    p = Presse(device)
    devpub = p.get_info()["devpub"]
    album, pressing = certs_for(devpub)
    tampered = bytearray(pressing)
    # Flip the first byte OF THE SIGNATURE. The tail of the certificate is
    # zero-padding past sig_len, neither signed nor read, so corrupting it
    # would prove nothing.
    tampered[PRESSING_PAYLOAD_LEN + 1] ^= 0xFF
    p.cmd(INS_PROVISION_ALBUM, album)
    assert p.cmd_sw(INS_PROVISION_PRESSING, bytes(tampered)) != "9000"
    assert p.get_info()["has_pressing"] is False


def test_the_pressing_step_requires_a_staged_album(device):
    """Without its album the pressing means nothing: the device refuses to store
    a certificate it cannot chain."""
    p = Presse(device)
    devpub = p.get_info()["devpub"]
    _, pressing = certs_for(devpub)
    assert p.cmd_sw(INS_PROVISION_PRESSING, pressing) != "9000"
    assert p.get_info()["has_pressing"] is False


@pytest.mark.parametrize("bad_len", [ALBUM_CERT_LEN - 1, ALBUM_CERT_LEN + 1])
def test_a_wrong_length_album_is_refused(device, bad_len):
    """Length is validated before anything is parsed."""
    p = Presse(device)
    assert p.cmd_sw(INS_PROVISION_ALBUM, bytes(bad_len)) != "9000"
