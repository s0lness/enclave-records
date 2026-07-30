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
# Handing a held copy on, in two phases: the giver commits to one recipient,
# then the recipient's receipt releases it. See docs/protocol.md.
INS_GIVE_ALBUM = 0x70
INS_GIVE_PRESSING = 0x71
INS_GIVE_CHAIN = 0x72
INS_GIVE_OFFER = 0x73
INS_TAKE_ALBUM = 0x74
INS_TAKE_PRESSING = 0x75
INS_TAKE_CHAIN = 0x76
INS_TAKE_ACCEPT = 0x77
INS_GIVE_HANDOVER = 0x78
INS_GIVE_FINISH = 0x79
INS_TAKE_HANDOVER = 0x7A
INS_TAKE_CONFIRM = 0x7B
INS_TAKE_RECEIPT = 0x7C
# Taking back a promise whose key never left. Not part of the ceremony: it is
# local to the giver and needs no pairing.
INS_GIVE_CANCEL = 0x7D

SW_OK = "9000"
SW_SOLD_OUT = "b104"
SW_NO_PRESSING = "b108"
# The promise can no longer be taken back: the sealed key has already left.
SW_KEY_FLOWN = "b10a"

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
# The bearer key: a secp256k1 scalar, and the width of the HMAC pad that seals
# it on the wire.
BEARER_KEY_LEN = 32
SIG_MAX_LEN = 72
# The provenance chain on the wire: the 32-byte head, then the hop count it
# stands for. Constant whatever the copy's history, where the ring it replaced
# cost 129 bytes and forgot its root past 32 hops.
CHAIN_WIRE_LEN = 32 + 1
# The handover frame, identical in both directions so a relay forwards it
# whole: giver devpub || sig_len || sig.
HANDOVER_WIRE_LEN = PUBKEY_LEN + 1 + SIG_MAX_LEN
HANDOVER_TAG = b"presse-handover"
CHAIN_TAG = b"presse-chain"
LINK_TAG = b"presse-link"
# The provenance witness read back over GET_BUNDLE p1=2: chain || chain_prev ||
# hops || has_from || giverpub || sig_len || sig.
WITNESS_LEN = 32 + 32 + 1 + 1 + PUBKEY_LEN + 1 + SIG_MAX_LEN
# The receipt that releases a committed giver: album_id || number || devpub.
RECEIPT_WIRE_LEN = 32 + 2 + PUBKEY_LEN

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


# --- screen geometry ---------------------------------------------------------
#
# Flex is 480x600 and the pages this app draws are not scrollable: a page that
# asks for more room than it has is not an error and not a wrap, it is simply
# drawn under the footer, or past the bottom of the screen. Neither shows up in
# the OCR text, which is why these are asserted on coordinates.

# The header's separation line and the footer's, measured on the emulator. A
# page's content lives strictly between them; only the footer's own labels sit
# below the second.
HEADER_TEXT_Y = 30
FOOTER_RULE_Y = 504
FOOTER_LABEL_Y = 534
FOOTER_LABELS = ("Back", "Quit")


def current_screen(dev, since: int = 0) -> list:
    """The elements of the screen on display, out of the cumulative event log.

    Each draw re-emits every element of the page, header first, so the screen
    now up is whatever follows the last header line."""
    events = [e for e in dev.events()[since:] if "y" in e]
    heads = [i for i, e in enumerate(events) if e["y"] == HEADER_TEXT_Y]
    return events[heads[-1]:] if heads else events


def is_footer_label(e) -> bool:
    """The footer's own labels: an action ("Back", "Quit") or the card's pager,
    which legitimately sit below the footer's rule."""
    text = e.get("text", "")
    return e["y"] == FOOTER_LABEL_Y and (text in FOOTER_LABELS or " of " in text)


def assert_page_fits(dev, since: int = 0):
    """Nothing but the footer's own labels may reach past the footer's rule.

    This is what "the page overruns" looks like from outside: the row or line
    that did not fit is still drawn, at coordinates that put it under the
    footer, so it shows as glyph tops above the rule (or not at all). The
    device reports nothing, and the text is present in the OCR either way."""
    spilled = [
        e
        for e in current_screen(dev, since)
        if not is_footer_label(e) and e["y"] + e["h"] > FOOTER_RULE_Y
    ]
    assert not spilled, f"drawn under the footer (rule at y={FOOTER_RULE_Y}): {spilled}"


def row_labels(dev, since: int = 0) -> list:
    """The label of each list row on the page: the text left of the value
    column, which `label_value_row` pads out with spaces."""
    return [
        e["text"].split("  ")[0].strip()
        for e in current_screen(dev, since)
        if e["y"] != HEADER_TEXT_Y and not is_footer_label(e)
    ]


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
        """Fire a UI-gated APDU, wait for its review screen, tap the button.

        Only screens drawn *after* the command is sent count. The event log is
        cumulative, so a device that has already been through one ceremony still
        carries the previous confirmation's words: matching those would tap at
        coordinates from a screen that is no longer up."""
        since = len(self.dev.events())
        thread, result = self.dev.apdu_async_start(apdu_hex(ins, data))
        assert self.wait_for_text_since(wait_text, since), (
            f"{self.dev.name}: never saw a fresh '{wait_text}': "
            f"{self.dev.screen_texts()[since:]}"
        )
        self.tap_text(button_text, since=since)
        thread.join(timeout=30)
        assert "data" in result, f"{self.dev.name}: gated INS {ins:#x} never returned"
        body, sw = split_sw(result["data"])
        return body, sw

    def wait_for_text_since(self, needle: str, since: int, timeout: float = 15.0) -> bool:
        """Like SpeculosDevice.wait_for_text, but blind to everything the device
        had already drawn before `since`."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            if any(needle in t for t in self.dev.screen_texts()[since:]):
                return True
            time.sleep(0.3)
        return False

    def tap_text(self, needle: str, timeout: float = 10.0, since: int = 0):
        deadline = time.time() + timeout
        while time.time() < deadline:
            for e in self.dev.events()[since:]:
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
            # The copy is promised to a recipient and awaiting its receipt.
            "committed": bool(flags & 4),
            # ...and the sealed key has left, so the promise is irrevocable.
            "key_flown": bool(flags & 8),
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
    # Wait for the words themselves, not for the button: on a device already
    # through one ceremony the button's text is still in the cumulative log,
    # and matching it would read the previous pairing's screen.
    words_m = wait_for_sas_words(master.dev, since_m)
    words_r = wait_for_sas_words(receiver.dev, since_r)
    assert words_m == words_r, f"SAS mismatch on screens: {words_m} vs {words_r}"
    assert len(words_m) == 4, f"no 4-word SAS on screen: {words_m}"

    master.tap_text("Words match", since=since_m)
    receiver.tap_text("Words match", since=since_r)
    tm.join(timeout=30)
    tr.join(timeout=30)
    sas_m, sw_m = split_sw(rm["data"])
    sas_r, sw_r = split_sw(rr["data"])
    assert sw_m == SW_OK and sw_r == SW_OK
    assert sas_m == sas_r, "devices derived different SAS bytes"
    return sas_m


def wait_for_sas_words(dev, since: int, timeout: float = 15.0) -> list:
    """Block until four SAS words have been drawn since `since`, or give up and
    return whatever was found so the caller can report it."""
    deadline = time.time() + timeout
    words = []
    while time.time() < deadline:
        words = sas_words_on_screen(dev, since)
        if len(words) == 4:
            return words
        time.sleep(0.3)
    return words


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
    # cert || sealed bearer key || MAC. The relay forwards it whole and can
    # read none of it: the key is masked with the session key it does not have.
    offer, sw = master.cmd_gated(INS_PRESS_OFFER, req, "Press this copy", "Press ")
    assert sw == SW_OK, f"press offer failed: {sw}"
    assert len(offer) == PRESSING_CERT_LEN + BEARER_KEY_LEN + MAC_LEN
    receiver.cmd(INS_PRESS_LOAD_ALBUM, album_msg)
    if carry_from is not None:
        carry_sleeve(carry_from, receiver)
    _, sw = receiver.cmd_gated(INS_PRESS_ACCEPT, offer, "Receive it", "Receive ")
    assert sw == SW_OK, f"press accept failed: {sw}"
    return offer[:PRESSING_CERT_LEN]


def give_stage(giver: Presse, taker: Presse, carry_art: bool = False) -> bytes:
    """Everything up to (not including) the taker's confirmation: the giver
    reads out what it holds and signs the handover, the taker stages it.

    Nothing here changes state on either device, so an interruption anywhere in
    this phase costs nothing at all. Ordering within each direction is what the
    sequence numbers pin, so a relay may interleave the two sides but never
    reorder one of them."""
    album_msg = giver.cmd(INS_GIVE_ALBUM)
    pressing_msg = giver.cmd(INS_GIVE_PRESSING)
    chain_msg = giver.cmd(INS_GIVE_CHAIN)
    assert len(chain_msg) == CHAIN_WIRE_LEN + MAC_LEN
    req = taker.cmd(INS_PRESS_REQUEST)
    handover = giver.cmd(INS_GIVE_HANDOVER, req)
    assert len(handover) == HANDOVER_WIRE_LEN + MAC_LEN

    taker.cmd(INS_TAKE_ALBUM, album_msg)
    taker.cmd(INS_TAKE_PRESSING, pressing_msg)
    taker.cmd(INS_TAKE_CHAIN, chain_msg)
    taker.cmd(INS_TAKE_HANDOVER, handover)
    if carry_art:
        carry_pressing_sleeve(giver, taker)
    return pressing_msg[:PRESSING_CERT_LEN]


def give_commit(giver: Presse):
    """GIVE_OFFER P1=0: the promise, and nothing else leaves the device.

    Gated the first time and silent on a retry: the commitment in flash already
    records the human's approval of exactly this handover, and re-asking would
    put a hand back on the path where a missed tap strands a copy."""
    if giver.get_info()["committed"]:
        giver.cmd(INS_GIVE_OFFER)
        return
    _, sw = giver.cmd_gated(INS_GIVE_OFFER, b"", "Give it away", "Give ")
    assert sw == SW_OK, f"give offer failed: {sw}"


def give_offer(giver: Presse) -> bytes:
    """The whole of the giver's phase 2: promise, then release the sealed key.

    Two commands because the gap between them is the only place a human can
    still change their mind: while the copy is promised but its key has not
    left, GIVE_CANCEL can take the promise back. Once P1=1 has run the key is
    flown and only the recipient's receipt ends the state."""
    give_commit(giver)
    sealed = giver.cmd(INS_GIVE_OFFER, p1=1)
    assert len(sealed) == BEARER_KEY_LEN + MAC_LEN
    return sealed


def give_cancel(giver: Presse):
    """GIVE_CANCEL: take back a promise whose key never left. UI-gated, and
    refused outright once the key has flown, so there is no relay-side flag
    that could turn it into a way of holding a copy twice."""
    _, sw = giver.cmd_gated(INS_GIVE_CANCEL, b"", "Take it back", "Take back")
    assert sw == SW_OK, f"give cancel failed: {sw}"


def finish_give(giver: Presse, taker: Presse):
    """The taker acknowledges, the giver erases. Neither half is gated, so this
    always completes on a live link and never waits on a human."""
    receipt = taker.cmd(INS_TAKE_RECEIPT)
    assert len(receipt) == RECEIPT_WIRE_LEN + MAC_LEN
    giver.cmd(INS_GIVE_FINISH, receipt)


def run_give(giver: Presse, taker: Presse, carry_art: bool = False) -> bytes:
    """One whole transfer over an already-paired channel, resumable.

    Safe to re-run against the same two devices after an interruption at any
    point: a giver that already committed re-sends the same key silently, and a
    taker that already stored the copy needs only to acknowledge it."""
    if taker.get_info()["has_pressing"]:
        # The copy landed; only its acknowledgement went missing.
        cert = taker.cmd(INS_GET_BUNDLE, p1=0)
        finish_give(giver, taker)
        return cert

    cert = give_stage(giver, taker, carry_art=carry_art)
    _, sw = taker.cmd_gated(INS_TAKE_CONFIRM, b"", "Receive it", "Receive ")
    assert sw == SW_OK, f"take confirm failed: {sw}"
    taker.cmd(INS_TAKE_ACCEPT, give_offer(giver))
    finish_give(giver, taker)
    return cert


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
    holderpub = cert[40 : 40 + PUBKEY_LEN]
    sig_len = cert[PRESSING_PAYLOAD_LEN]
    sig = cert[PRESSING_PAYLOAD_LEN + 1 : PRESSING_PAYLOAD_LEN + 1 + sig_len]
    return album_id, number, edition, holderpub, sig, cert[:PRESSING_PAYLOAD_LEN]


def ecdsa_verify(pubkey_uncompressed: bytes, payload: bytes, sig_der: bytes) -> bool:
    vk = VerifyingKey.from_string(pubkey_uncompressed, curve=SECP256k1)
    digest = hashlib.sha256(payload).digest()
    try:
        return vk.verify_digest(sig_der, digest, sigdecode=sigdecode_der)
    except BadSignatureError:
        return False


def handover_message(
    album_id: bytes, number: int, giverpub: bytes, takerpub: bytes, chain: bytes
) -> bytes:
    """The record a giver signs with its device key, mirroring give.rs.

    Four bindings, one substitution stopped each: the copy (album id, number),
    the two devices, and `chain`, the head of the provenance chain as the giver
    received it. The last is what pins the signature to one *moment* of one
    copy's history, so a link cannot be replayed from earlier in that history,
    reordered, or grafted onto a truncated one."""
    assert len(album_id) == 32 and len(chain) == 32
    assert len(giverpub) == PUBKEY_LEN and len(takerpub) == PUBKEY_LEN
    return (
        HANDOVER_TAG
        + album_id
        + struct.pack("<H", number)
        + giverpub
        + takerpub
        + chain
    )


def chain_genesis(album_id: bytes, number: int) -> bytes:
    """The root of a copy's chain, derived from the copy's own signed identity.
    Nothing transmits it: every device computes the same value, and two copies
    never share one."""
    return hashlib.sha256(CHAIN_TAG + album_id + struct.pack("<H", number)).digest()


def chain_link(prev: bytes, giverpub: bytes, sig: bytes, takerpub: bytes) -> bytes:
    """Fold one hop into the chain: the head it extends, the handover frame that
    proves it, and the device it landed on. The signature is inside the digest,
    so a head commits to the proof of its own last hop."""
    assert len(prev) == 32
    return hashlib.sha256(
        LINK_TAG
        + prev
        + giverpub
        + bytes([len(sig)])
        + sig.ljust(SIG_MAX_LEN, b"\x00")
        + takerpub
    ).digest()


def read_witness(presse: "Presse") -> dict:
    """GET_BUNDLE p1=2: everything a third party needs to check the last link
    and to compare two copies claiming one number."""
    body = presse.cmd(INS_GET_BUNDLE, p1=2)
    assert len(body) == WITNESS_LEN, len(body)
    sig_len = body[66 + PUBKEY_LEN]
    return {
        "chain": body[:32],
        "chain_prev": body[32:64],
        "hops": body[64],
        "has_from": bool(body[65]),
        "giverpub": body[66 : 66 + PUBKEY_LEN],
        "sig": body[132 : 132 + sig_len],
    }


def verify_witness(witness: dict, album_id: bytes, number: int, takerpub: bytes) -> bool:
    """Check a device's provenance witness end to end, with no device code: the
    stored signature verifies over the head that precedes it, and hashing that
    link reproduces the head the device reports holding.

    A copy that never moved has no link, and its head must be the genesis its
    own certificate implies -- so "straight from the press" is a claim with a
    shape, not the absence of one."""
    if not witness["has_from"]:
        return witness["chain"] == chain_genesis(album_id, number) and witness["hops"] == 0
    msg = handover_message(
        album_id, number, witness["giverpub"], takerpub, witness["chain_prev"]
    )
    if not ecdsa_verify(witness["giverpub"], msg, witness["sig"]):
        return False
    expected = chain_link(
        witness["chain_prev"], witness["giverpub"], witness["sig"], takerpub
    )
    return expected == witness["chain"]


def verify_chain(album_cert: bytes, pressing_cert: bytes) -> dict:
    """Offline verification of the certificates alone: album self-signature,
    pressing signature, album_id linkage, edition match, number sanity. The
    album signature also covers the sleeve hash, so a returned sleeve_hash is
    authenticated.

    This says the copy is real; it does NOT say who holds it. The certificate
    names a bearer key, not a device, so the holder question is answered only by
    `verify_possession`, live, against that key. The two together are the
    verification: neither alone is."""
    albpub, title, artist, edition, sleeve_hash, alb_sig, alb_payload = parse_album_cert(
        album_cert
    )
    assert ecdsa_verify(albpub, alb_payload, alb_sig), "album cert signature invalid"

    album_id, number, p_edition, holderpub, p_sig, p_payload = parse_pressing_cert(pressing_cert)
    assert ecdsa_verify(albpub, p_payload, p_sig), "pressing cert signature invalid"
    assert album_id == hashlib.sha256(albpub).digest(), "album_id mismatch"
    assert p_edition == edition, "edition mismatch between certs"
    assert 1 <= number <= edition, "pressing number out of range"
    return {
        "title": title,
        "artist": artist,
        "number": number,
        "edition": edition,
        "sleeve_hash": sleeve_hash,
        "holderpub": holderpub,
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


def carry_pressing_sleeve(src: "Presse", dst: "Presse"):
    """Copy a held copy's cover from one device's pressing slot to the other's,
    for a transfer. The record moves whole: without this the taker would hold a
    genuine copy and render the generative fallback for its cover."""
    art = read_art(src, SLOT_PRESSING)
    if not any(art):
        return None
    upload_art(dst, art, SLOT_PRESSING)
    return hashlib.sha256(art).hexdigest()


def verify_possession(presse: Presse, pressing_cert: bytes):
    """Challenge-response: the device proves, live, that it holds the bearer
    key the certificate names. This is the whole of "who owns this copy": a
    device that gave it away can no longer answer, and one that never had it
    never could."""
    import os

    _, _, _, holderpub, _, _ = parse_pressing_cert(pressing_cert)
    nonce = os.urandom(32)
    body = presse.cmd(INS_CHALLENGE, nonce)
    sig_len = body[0]
    sig = body[1 : 1 + sig_len]
    assert ecdsa_verify(holderpub, b"presse-verify" + nonce, sig), "challenge signature invalid"


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


def demo_bearer_key(title: str, artist: str, edition: int, number: int) -> tuple[bytes, bytes]:
    """A reproducible bearer keypair for provisioning, derived from the copy's
    identity for the same reason `demo_album_key` is: re-provisioning after an
    NVM wipe must reproduce the copy, not a look-alike. Development only; a real
    press mints this from the master's TRNG and it exists nowhere else."""
    seed = hashlib.sha256(
        b"presse-demo-bearer|" + f"{title}|{artist}|{edition}|{number}".encode()
    ).digest()
    sk = SigningKey.from_string(seed, curve=SECP256k1)
    return seed, sk.get_verifying_key().to_string("uncompressed")


def build_pressing_cert(
    priv: bytes, album_id: bytes, number: int, edition: int, holderpub: bytes
) -> bytes:
    """Mirror of certs.rs build_pressing_cert."""
    assert len(album_id) == 32 and len(holderpub) == PUBKEY_LEN
    assert 0 < number <= edition, "number outside the edition"
    cert = bytearray(PRESSING_CERT_LEN)
    cert[0:4] = PRESSING_MAGIC
    cert[4:36] = album_id
    struct.pack_into("<HH", cert, 36, number, edition)
    cert[40 : 40 + PUBKEY_LEN] = holderpub
    sig = ecdsa_sign(priv, bytes(cert[:PRESSING_PAYLOAD_LEN]))
    cert[PRESSING_PAYLOAD_LEN] = len(sig)
    cert[PRESSING_PAYLOAD_LEN + 1 : PRESSING_PAYLOAD_LEN + 1 + len(sig)] = sig
    return bytes(cert)


def provision_pressing(
    presse: "Presse", album_cert: bytes, pressing_cert: bytes, bearer_priv: bytes
):
    """Push a relay-signed pressing, and the bearer key that owns it, into a
    device. Refused if it already holds one: provisioning only ever adds.

    The key crosses the USB cable in the clear. There is no pairing on this
    path and so no session key to seal it with, which is exactly why this is a
    development path and not a ceremony."""
    assert len(bearer_priv) == BEARER_KEY_LEN
    presse.cmd(INS_PROVISION_ALBUM, album_cert)
    presse.cmd(INS_PROVISION_PRESSING, pressing_cert + bearer_priv)
