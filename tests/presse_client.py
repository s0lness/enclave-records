"""APDU client + ceremony driver for the presse app.

Independent of the device code on purpose: certificate parsing and signature
verification here re-implement docs/protocol.md (python-ecdsa + hashlib), so a
device bug can't hide behind shared code.
"""

import hashlib
import struct
import time

from ecdsa import VerifyingKey, SigningKey, SECP256k1, BadSignatureError
from ecdsa.util import sigdecode_der, sigencode_der

CLA = 0xB5

INS_GET_INFO = 0x01
INS_COLLECTION = 0x02
INS_CUT = 0x10
INS_PAIR_COMMIT = 0x21
INS_PAIR_RESPOND = 0x22
INS_PAIR_REVEAL = 0x23
INS_PAIR_FINISH = 0x24
INS_PAIR_SAS = 0x25
INS_GET_ALBUM = 0x30
INS_PRESS_REQUEST = 0x31
INS_PRESS_OFFER = 0x32
INS_PRESS_LOAD_ALBUM = 0x33
INS_PRESS_ACCEPT = 0x34
INS_PROVISION_ALBUM = 0x66
INS_PROVISION_PRESSING = 0x67
INS_GET_BUNDLE = 0x40
INS_CHALLENGE = 0x41

SW_OK = "9000"
SW_SOLD_OUT = "b104"

PUBKEY_LEN = 65
MAC_LEN = 32
SLEEVE_HASH_LEN = 32
TITLE_MAX = 32
# Capped so ALBUM_CERT_LEN (223) + MAC (32) == 255, the single-frame APDU limit.
ARTIST_MAX = 13
# magic(4) albpub(65) title_len(1) title(32) edition(2) sleeve_hash(32)
#   artist_len(1) artist(13)
ARTIST_LEN_OFF = 4 + PUBKEY_LEN + 1 + TITLE_MAX + 2 + SLEEVE_HASH_LEN
ARTIST_OFF = ARTIST_LEN_OFF + 1
ALBUM_PAYLOAD_LEN = ARTIST_OFF + ARTIST_MAX
ALBUM_CERT_LEN = ALBUM_PAYLOAD_LEN + 1 + 72
PRESSING_PAYLOAD_LEN = 4 + 32 + 2 + 2 + PUBKEY_LEN
PRESSING_CERT_LEN = PRESSING_PAYLOAD_LEN + 1 + 72

INS_SET_ART = 0x62
INS_GET_ART = 0x64
ART_CHUNK = 64
ART_W = 128  # must track state.rs: the device rejects any offset past ART_LEN
ART_LEN = ART_W * ART_W // 8  # 1bpp square sleeve, 2048 bytes
# One art slot per record a device can hold: its master and its pressing.
SLOT_MASTER = 0
SLOT_PRESSING = 1


def apdu_hex(ins: int, data: bytes = b"", p1: int = 0, p2: int = 0) -> str:
    return bytes([CLA, ins, p1, p2, len(data)]).hex() + data.hex()


def split_sw(resp_hex: str):
    return bytes.fromhex(resp_hex[:-4]), resp_hex[-4:]


class Presse:
    """Wraps a SpeculosDevice with presse commands. UI-gated commands take a
    `tap` callable run once the review screen is up."""

    def __init__(self, device):
        self.dev = device

    def cmd(self, ins: int, data: bytes = b"", p1: int = 0, p2: int = 0) -> bytes:
        resp = self.dev.apdu(apdu_hex(ins, data, p1, p2))
        body, sw = split_sw(resp)
        assert sw == SW_OK, f"{self.dev.name}: INS {ins:#x} returned SW {sw}"
        return body

    def cmd_sw(self, ins: int, data: bytes = b"", p1: int = 0, p2: int = 0) -> str:
        """Variant returning the status word for error-path tests."""
        _, sw = split_sw(self.dev.apdu(apdu_hex(ins, data, p1, p2)))
        return sw

    def cmd_gated(self, ins: int, data: bytes, button_text: str, wait_text: str):
        """Fire a UI-gated APDU, wait for its review screen, tap the button."""
        thread, result = self.dev.apdu_async_start(apdu_hex(ins, data))
        assert self.dev.wait_for_text(wait_text), (
            f"{self.dev.name}: never saw '{wait_text}': {self.dev.screen_texts()}"
        )
        self.tap_text(button_text)
        thread.join(timeout=30)
        assert "data" in result, f"{self.dev.name}: gated INS {ins:#x} never returned"
        body, sw = split_sw(result["data"])
        return body, sw

    def tap_text(self, needle: str, timeout: float = 10.0):
        deadline = time.time() + timeout
        while time.time() < deadline:
            for e in self.dev.events():
                if needle in e.get("text", "") and "x" in e and "y" in e:
                    self.dev.finger(e["x"], e["y"])
                    return
            time.sleep(0.3)
        raise AssertionError(f"{self.dev.name}: no tappable '{needle}'")

    # --- high-level ceremony steps ---

    def get_info(self):
        body = self.cmd(INS_GET_INFO)
        flags = body[0]
        devpub = body[1 : 1 + PUBKEY_LEN]
        edition, counter = struct.unpack_from("<HH", body, 1 + PUBKEY_LEN)
        title_len = body[1 + PUBKEY_LEN + 4]
        title = body[1 + PUBKEY_LEN + 5 : 1 + PUBKEY_LEN + 5 + title_len].decode()
        return {
            "has_master": bool(flags & 1),
            "has_pressing": bool(flags & 2),
            "devpub": devpub,
            "edition": edition,
            "counter": counter,
            "title": title,
        }

    def cut(self, title: str, edition: int, artist: str = "") -> bytes:
        """CUT: edition(2 LE) || title_len(1) || title || artist. Artist is
        optional and capped at ARTIST_MAX bytes so cert+MAC stays one frame."""
        title_b = title.encode()
        artist_b = artist.encode()
        assert len(artist_b) <= ARTIST_MAX, f"artist over {ARTIST_MAX} bytes"
        data = struct.pack("<HB", edition, len(title_b)) + title_b + artist_b
        body, sw = self.cmd_gated(INS_CUT, data, "Cut the master", "Cut master")
        assert sw == SW_OK, f"cut failed: {sw}"
        assert len(body) == ALBUM_CERT_LEN
        return body


def run_pairing(master: Presse, receiver: Presse):
    """Happy-path pairing up to (but not including) the SAS taps."""
    commitment = master.cmd(INS_PAIR_COMMIT)
    assert len(commitment) == 32
    eb = receiver.cmd(INS_PAIR_RESPOND, commitment)
    assert len(eb) == PUBKEY_LEN
    ea = master.cmd(INS_PAIR_REVEAL, eb)
    assert len(ea) == PUBKEY_LEN
    receiver.cmd(INS_PAIR_FINISH, ea)
    return ea, eb


def confirm_sas_both(master: Presse, receiver: Presse):
    """Fire PAIR_SAS on both devices, assert the words match on both screens,
    tap both. Returns the SAS bytes of each device."""
    since_m = len(master.dev.events())
    since_r = len(receiver.dev.events())
    tm, rm = master.dev.apdu_async_start(apdu_hex(INS_PAIR_SAS))
    tr, rr = receiver.dev.apdu_async_start(apdu_hex(INS_PAIR_SAS))
    assert master.dev.wait_for_text("Words match")
    assert receiver.dev.wait_for_text("Words match")

    words_m = sas_words_on_screen(master.dev, since_m)
    words_r = sas_words_on_screen(receiver.dev, since_r)
    assert words_m == words_r, f"SAS mismatch on screens: {words_m} vs {words_r}"
    assert len(words_m) == 4

    master.tap_text("Words match")
    receiver.tap_text("Words match")
    tm.join(timeout=30)
    tr.join(timeout=30)
    sas_m, sw_m = split_sw(rm["data"])
    sas_r, sw_r = split_sw(rr["data"])
    assert sw_m == SW_OK and sw_r == SW_OK
    assert sas_m == sas_r, "devices derived different SAS bytes"
    return sas_m


def sas_words_on_screen(dev, since: int = 0) -> list:
    """The SAS message is a 4-line text block; OCR may deliver it as one
    event or per-line. Only look at events emitted after `since` (the event
    count captured before firing PAIR_SAS), so stale screens can't leak in."""
    texts = [e.get("text", "") for e in dev.events()[since:]]
    for t in texts:
        parts = [w for w in t.replace("\n", " ").split(" ") if w]
        if len(parts) == 4 and all(w.isalpha() and w.islower() for w in parts):
            return parts
    # Per-line fallback: four consecutive lowercase single-word events.
    words = []
    for t in texts:
        w = t.strip()
        if w.isalpha() and w.islower():
            words.append(w)
        elif words:
            if len(words) >= 4:
                break
            words = []
    return words[-4:] if len(words) >= 4 else words


def run_press(master: Presse, receiver: Presse, carry_from: "Presse | None" = None) -> bytes:
    """One full press onto the receiver. Returns the PressingCert.

    When `carry_from` is given, its sleeve is streamed to the receiver AFTER the
    album cert is loaded but BEFORE the pressing is accepted. The receiver only
    repaints its library when the pressing lands (PRESS_ACCEPT), so carrying the
    art first is what makes that single repaint show the real cover instead of
    the generative placeholder. SET_ART itself never repaints."""
    album_msg = master.cmd(INS_GET_ALBUM)
    req = receiver.cmd(INS_PRESS_REQUEST)
    cert_mac, sw = master.cmd_gated(INS_PRESS_OFFER, req, "Press this copy", "Press ")
    assert sw == SW_OK, f"press offer failed: {sw}"
    receiver.cmd(INS_PRESS_LOAD_ALBUM, album_msg)
    if carry_from is not None:
        carry_sleeve(carry_from, receiver)
    _, sw = receiver.cmd_gated(INS_PRESS_ACCEPT, cert_mac, "Receive it", "Receive ")
    assert sw == SW_OK, f"press accept failed: {sw}"
    return cert_mac[:PRESSING_CERT_LEN]


# --- independent verification (no device code, no session secrets) ---


def parse_album_cert(cert: bytes):
    assert len(cert) == ALBUM_CERT_LEN and cert[:4] == b"PRA1"
    albpub = cert[4 : 4 + PUBKEY_LEN]
    title_len = cert[69]
    title = cert[70 : 70 + title_len].decode()
    edition = struct.unpack_from("<H", cert, 102)[0]
    sleeve_hash = cert[104 : 104 + SLEEVE_HASH_LEN]
    artist_len = cert[ARTIST_LEN_OFF]
    artist = cert[ARTIST_OFF : ARTIST_OFF + artist_len].decode()
    sig_len = cert[ALBUM_PAYLOAD_LEN]
    sig = cert[ALBUM_PAYLOAD_LEN + 1 : ALBUM_PAYLOAD_LEN + 1 + sig_len]
    return albpub, title, artist, edition, sleeve_hash, sig, cert[:ALBUM_PAYLOAD_LEN]


def parse_pressing_cert(cert: bytes):
    assert len(cert) == PRESSING_CERT_LEN and cert[:4] == b"PRP1"
    album_id = cert[4:36]
    number, edition = struct.unpack_from("<HH", cert, 36)
    recvpub = cert[40 : 40 + PUBKEY_LEN]
    sig_len = cert[PRESSING_PAYLOAD_LEN]
    sig = cert[PRESSING_PAYLOAD_LEN + 1 : PRESSING_PAYLOAD_LEN + 1 + sig_len]
    return album_id, number, edition, recvpub, sig, cert[:PRESSING_PAYLOAD_LEN]


def ecdsa_verify(pubkey_uncompressed: bytes, payload: bytes, sig_der: bytes) -> bool:
    vk = VerifyingKey.from_string(pubkey_uncompressed, curve=SECP256k1)
    digest = hashlib.sha256(payload).digest()
    try:
        return vk.verify_digest(sig_der, digest, sigdecode=sigdecode_der)
    except BadSignatureError:
        return False


def verify_chain(album_cert: bytes, pressing_cert: bytes, holder_devpub: bytes) -> dict:
    """Full offline verification: album self-signature, pressing signature,
    album_id linkage, device binding, number sanity. The album signature now
    also covers the sleeve hash, so a returned sleeve_hash is authenticated."""
    albpub, title, artist, edition, sleeve_hash, alb_sig, alb_payload = parse_album_cert(
        album_cert
    )
    assert ecdsa_verify(albpub, alb_payload, alb_sig), "album cert signature invalid"

    album_id, number, p_edition, recvpub, p_sig, p_payload = parse_pressing_cert(pressing_cert)
    assert ecdsa_verify(albpub, p_payload, p_sig), "pressing cert signature invalid"
    assert album_id == hashlib.sha256(albpub).digest(), "album_id mismatch"
    assert p_edition == edition, "edition mismatch between certs"
    assert 1 <= number <= edition, "pressing number out of range"
    assert recvpub == holder_devpub, "pressing not bound to this device"
    return {
        "title": title,
        "artist": artist,
        "number": number,
        "edition": edition,
        "sleeve_hash": sleeve_hash,
    }


def verify_sleeve(album_cert: bytes, art_bytes: bytes) -> bool:
    """The sleeve is genuine iff its bytes hash to the sleeve_hash the album
    signature commits to. An all-zero sleeve_hash means the edition bound no
    sleeve. Independent of the device: this is the check a third party runs."""
    _, _, _, _, sleeve_hash, _, _ = parse_album_cert(album_cert)
    if sleeve_hash == b"\x00" * SLEEVE_HASH_LEN:
        return False
    return hashlib.sha256(art_bytes).digest() == sleeve_hash


def upload_art(presse: "Presse", art_bytes: bytes, slot: int = SLOT_MASTER):
    """Push a packed sleeve into one of the device's art slots, chunk by chunk."""
    for off in range(0, len(art_bytes), ART_CHUNK):
        payload = struct.pack("<H", off) + art_bytes[off : off + ART_CHUNK]
        presse.cmd(INS_SET_ART, payload, p1=slot)


def read_art(presse: "Presse", slot: int = SLOT_MASTER) -> bytes:
    """Read one of a device's stored sleeves back over GET_ART, chunk by chunk."""
    art = bytearray()
    for chunk in range((ART_LEN + ART_CHUNK - 1) // ART_CHUNK):
        art += presse.cmd(INS_GET_ART, p1=chunk, p2=slot)
    return bytes(art)


def carry_sleeve(src: "Presse", dst: "Presse"):
    """Copy the master's sleeve on src into dst's pressing slot over the relay.

    A press moves a copy of the master's record, so the sleeve leaves src's
    master slot and lands in dst's pressing slot: each record keeps its own
    cover, and a device holding both shows both.

    The bytes are public and dst validates them against the sleeve hash the
    album certificate already commits to, so an untrusted relay carrying them
    is fine. Returns the sha256 hex, or None when src has no sleeve (blank
    slot): nothing to carry, so the caller stays silent."""
    art = read_art(src, SLOT_MASTER)
    if not any(art):
        return None
    upload_art(dst, art, SLOT_PRESSING)
    return hashlib.sha256(art).hexdigest()


def verify_possession(presse: Presse, pressing_cert: bytes):
    """Challenge-response: the device proves it holds the bound key, live."""
    import os

    _, _, _, recvpub, _, _ = parse_pressing_cert(pressing_cert)
    nonce = os.urandom(32)
    body = presse.cmd(INS_CHALLENGE, nonce)
    sig_len = body[0]
    sig = body[1 : 1 + sig_len]
    assert ecdsa_verify(recvpub, b"presse-verify" + nonce, sig), "challenge signature invalid"


# --- relay-side certificate authority (development provisioning) ------------
#
# The device is the authority in a real ceremony: it generates the album key at
# cut time and never lets it out. These builders exist so the RELAY can play the
# master when a scene needs a copy whose number no two-device ceremony can
# reach. See relay/provision.py and handlers/provision.rs.

ALBUM_MAGIC = b"PRA1"
PRESSING_MAGIC = b"PRP1"


def ecdsa_sign(priv: bytes, payload: bytes) -> bytes:
    """Sign SHA-256(payload), DER-encoded: what the device's verify expects."""
    sk = SigningKey.from_string(priv, curve=SECP256k1)
    return sk.sign_digest_deterministic(
        hashlib.sha256(payload).digest(), sigencode=sigencode_der
    )


def demo_album_key(title: str, artist: str, edition: int) -> tuple[bytes, bytes]:
    """A reproducible album keypair for provisioning, derived from the album's
    own identity. Deterministic on purpose: the Edition ID shown on screen must
    be the same across takes, and across a re-provision after an NVM wipe.
    Development only, an album key is generated inside the device by a real cut."""
    seed = hashlib.sha256(
        b"presse-demo-album|" + f"{title}|{artist}|{edition}".encode()
    ).digest()
    sk = SigningKey.from_string(seed, curve=SECP256k1)
    return seed, b"" + sk.get_verifying_key().to_string()


def build_album_cert(
    priv: bytes, albpub: bytes, title: str, edition: int, sleeve_hash: bytes, artist: str
) -> bytes:
    """Mirror of certs.rs build_album_cert. Layout is duplicated, not shared:
    docs/protocol.md is the contract, and an independent re-implementation is
    what catches a layout drift."""
    title_b, artist_b = title.encode(), artist.encode()
    assert 0 < len(title_b) <= TITLE_MAX, "title out of range"
    assert len(artist_b) <= ARTIST_MAX, "artist too long"
    assert len(sleeve_hash) == SLEEVE_HASH_LEN
    cert = bytearray(ALBUM_CERT_LEN)
    cert[0:4] = ALBUM_MAGIC
    cert[4 : 4 + PUBKEY_LEN] = albpub
    cert[69] = len(title_b)
    cert[70 : 70 + len(title_b)] = title_b
    struct.pack_into("<H", cert, 102, edition)
    cert[104 : 104 + SLEEVE_HASH_LEN] = sleeve_hash
    cert[ARTIST_LEN_OFF] = len(artist_b)
    cert[ARTIST_OFF : ARTIST_OFF + len(artist_b)] = artist_b
    sig = ecdsa_sign(priv, bytes(cert[:ALBUM_PAYLOAD_LEN]))
    cert[ALBUM_PAYLOAD_LEN] = len(sig)
    cert[ALBUM_PAYLOAD_LEN + 1 : ALBUM_PAYLOAD_LEN + 1 + len(sig)] = sig
    return bytes(cert)


def build_pressing_cert(
    priv: bytes, album_id: bytes, number: int, edition: int, recvpub: bytes
) -> bytes:
    """Mirror of certs.rs build_pressing_cert."""
    assert len(album_id) == 32 and len(recvpub) == PUBKEY_LEN
    assert 0 < number <= edition, "number outside the edition"
    cert = bytearray(PRESSING_CERT_LEN)
    cert[0:4] = PRESSING_MAGIC
    cert[4:36] = album_id
    struct.pack_into("<HH", cert, 36, number, edition)
    cert[40 : 40 + PUBKEY_LEN] = recvpub
    sig = ecdsa_sign(priv, bytes(cert[:PRESSING_PAYLOAD_LEN]))
    cert[PRESSING_PAYLOAD_LEN] = len(sig)
    cert[PRESSING_PAYLOAD_LEN + 1 : PRESSING_PAYLOAD_LEN + 1 + len(sig)] = sig
    return bytes(cert)


def provision_pressing(presse: "Presse", album_cert: bytes, pressing_cert: bytes):
    """Push a relay-signed pressing into a device. Refused if it already holds
    one: provisioning only ever adds a record, so "bound forever" stays true."""
    presse.cmd(INS_PROVISION_ALBUM, album_cert)
    presse.cmd(INS_PROVISION_PRESSING, pressing_cert)
