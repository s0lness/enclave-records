"""Development provisioning: the relay signs, the device still checks.

These pin the guarantees the provisioning path must not lose. It exists so a
scene can show a copy whose number no two-device ceremony can reach, but it must
stay as strict as a real receive about what it will store.
"""

import hashlib

import pytest

from presse_client import (
    ALBUM_CERT_LEN,
    BEARER_KEY_LEN,
    INS_PROVISION_ALBUM,
    INS_PROVISION_PRESSING,
    PRESSING_PAYLOAD_LEN,
    SLOT_PRESSING,
    Presse,
    build_album_cert,
    build_pressing_cert,
    demo_album_key,
    demo_bearer_key,
    provision_pressing,
    upload_art,
    verify_chain,
    verify_possession,
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


def certs_for(number: int = NUMBER, edition: int = EDITION,
              sleeve_hash: bytes | None = None):
    """The album cert, the pressing cert, and the bearer key the pressing is
    bound to. The recipient device plays no part in the certificates: a copy is
    bound to a key, and provisioning hands over both halves of it."""
    priv, albpub = demo_album_key(TITLE, ARTIST, edition)
    album = build_album_cert(
        priv, albpub, TITLE, edition, sleeve_hash or bytes(32), ARTIST
    )
    bearer_priv, bearer_pub = demo_bearer_key(TITLE, ARTIST, edition, number)
    pressing = build_pressing_cert(
        priv, hashlib.sha256(albpub).digest(), number, edition, bearer_pub
    )
    return album, pressing, bearer_priv


def test_provisioned_pressing_is_held_and_verifies(device):
    """A provisioned copy is indistinguishable from a pressed one afterwards:
    the device reports it, lists it, and the chain verifies offline."""
    p = Presse(device)
    album, pressing, bearer = certs_for()

    provision_pressing(p, album, pressing, bearer)

    info = p.get_info()
    assert info["has_pressing"] is True
    assert info["has_master"] is False, "provisioning must not fabricate a master"
    result = verify_chain(album, pressing)
    assert (result["number"], result["edition"]) == (NUMBER, EDITION)
    # The bearer key landed with the certificate, so the device can answer for
    # the copy: a provisioned copy is as real as a pressed one, or it is useless.
    verify_possession(p, pressing)
    assert device.wait_for_text(TITLE), device.screen_texts()
    assert device.wait_for_text(f"#{NUMBER} of {EDITION}"), device.screen_texts()


def test_the_sleeve_lands_in_the_pressing_slot(device):
    """The cover is carried into the pressing's own slot and, its hash matching
    the signed one, the device shows it rather than the generative fallback."""
    p = Presse(device)
    art = a_sleeve()
    album, pressing, bearer = certs_for(sleeve_hash=hashlib.sha256(art).digest())

    upload_art(p, art, SLOT_PRESSING)
    provision_pressing(p, album, pressing, bearer)

    assert p.get_info()["has_pressing"] is True
    assert device.wait_for_text(TITLE), device.screen_texts()


def test_provisioning_never_overwrites_a_holding(device):
    """Provisioning only ever adds a record: a device already holding one
    refuses the second, so no copy is ever displaced by this path."""
    p = Presse(device)
    album, pressing, bearer = certs_for()
    provision_pressing(p, album, pressing, bearer)

    other_album, other_pressing, other_bearer = certs_for(number=2, edition=EDITION)
    p.cmd(INS_PROVISION_ALBUM, other_album)
    assert p.cmd_sw(INS_PROVISION_PRESSING, other_pressing + other_bearer) != "9000"
    assert p.get_info()["has_pressing"] is True


def test_a_bearer_key_that_does_not_match_the_certificate_is_refused(device):
    """The binding is checked, not trusted. A certificate is worthless without
    the scalar whose point the album key signed, so handing over a key of one's
    own choosing plants nothing."""
    p = Presse(device)
    album, pressing, _ = certs_for()
    wrong_bearer = bytes(range(1, BEARER_KEY_LEN + 1))
    p.cmd(INS_PROVISION_ALBUM, album)
    assert p.cmd_sw(INS_PROVISION_PRESSING, pressing + wrong_bearer) != "9000"
    assert p.get_info()["has_pressing"] is False


def test_an_unsigned_pressing_is_refused(device):
    """The relay is the authority for *who pressed*, never for the signature:
    a corrupted certificate is rejected exactly as on a real receive."""
    p = Presse(device)
    album, pressing, bearer = certs_for()
    tampered = bytearray(pressing)
    # Flip the first byte OF THE SIGNATURE. The tail of the certificate is
    # zero-padding past sig_len, neither signed nor read, so corrupting it
    # would prove nothing.
    tampered[PRESSING_PAYLOAD_LEN + 1] ^= 0xFF
    p.cmd(INS_PROVISION_ALBUM, album)
    assert p.cmd_sw(INS_PROVISION_PRESSING, bytes(tampered) + bearer) != "9000"
    assert p.get_info()["has_pressing"] is False


def test_the_pressing_step_requires_a_staged_album(device):
    """Without its album the pressing means nothing: the device refuses to store
    a certificate it cannot chain."""
    p = Presse(device)
    _, pressing, bearer = certs_for()
    assert p.cmd_sw(INS_PROVISION_PRESSING, pressing + bearer) != "9000"
    assert p.get_info()["has_pressing"] is False


@pytest.mark.parametrize("bad_len", [ALBUM_CERT_LEN - 1, ALBUM_CERT_LEN + 1])
def test_a_wrong_length_album_is_refused(device, bad_len):
    """Length is validated before anything is parsed."""
    p = Presse(device)
    assert p.cmd_sw(INS_PROVISION_ALBUM, bytes(bad_len)) != "9000"
