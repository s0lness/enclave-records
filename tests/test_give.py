"""Handing a copy on, between two emulated Flexes with this file as the relay.

A copy is a bearer key, and giving it away is sending that key over the paired
channel. Because a device cannot delete and deliver in the same instant, this
happens in two phases: the giver commits its copy to one named recipient, and
the recipient's receipt is what finally erases it.

So the assertions come in two families. What the giver can no longer do the
moment it commits (prove possession, offer the copy to anyone else), and what a
dropped link must never cost: every interruption here is resumed, not mourned.
"""

import hashlib
import struct

import pytest
from ecdsa import SigningKey, SECP256k1

from mitm import MitmEndpoint
from presse_client import (
    ART_LEN,
    BEARER_KEY_LEN,
    CHAIN_WIRE_LEN,
    INS_CHALLENGE,
    INS_GIVE_ALBUM,
    INS_GIVE_CANCEL,
    INS_GIVE_CHAIN,
    INS_GIVE_FINISH,
    INS_GIVE_HANDOVER,
    INS_GIVE_OFFER,
    INS_GIVE_PRESSING,
    INS_PAIR_COMMIT,
    INS_PAIR_FINISH,
    INS_PAIR_RESPOND,
    INS_PAIR_REVEAL,
    INS_PAIR_SAS,
    INS_PRESS_REQUEST,
    INS_TAKE_ACCEPT,
    INS_TAKE_ALBUM,
    INS_TAKE_CHAIN,
    INS_TAKE_CONFIRM,
    INS_TAKE_HANDOVER,
    INS_TAKE_PRESSING,
    INS_TAKE_RECEIPT,
    PUBKEY_LEN,
    Presse,
    SIG_MAX_LEN,
    SLOT_MASTER,
    SLOT_PRESSING,
    SW_KEY_FLOWN,
    SW_NO_PRESSING,
    SW_OK,
    apdu_hex,
    assert_page_fits,
    build_album_cert,
    build_pressing_cert,
    chain_genesis,
    chain_link,
    confirm_sas_both,
    demo_album_key,
    demo_bearer_key,
    ecdsa_sign,
    ecdsa_verify,
    finish_give,
    give_cancel,
    give_commit,
    give_offer,
    give_stage,
    handover_message,
    parse_pressing_cert,
    provision_pressing,
    read_art,
    read_witness,
    row_labels,
    run_give,
    run_pairing,
    run_press,
    upload_art,
    verify_chain,
    verify_possession,
    verify_witness,
)

# Short so a library row does not truncate it: the on-screen assertions compare
# the whole string.
TITLE = "Nocturne"
ARTIST = "Chopin"
EDITION = 5

SW_DENY = "6985"
SW_BAD_STATE = "b101"
SW_BAD_CERT = "b103"

# The back of the record, in order. Fixed: the page has room for four rows and
# for no fifth, whatever the device holds.
BACK_ROWS = ["Number", "Edition ID", "Device ID", "Learn more"]


@pytest.fixture
def two(pair):
    a, b = pair
    return Presse(a), Presse(b)


def a_sleeve(seed: int = 0) -> bytes:
    """A deterministic packed sleeve; only its hash ever matters."""
    return bytes(((i * 7 + seed) & 0xFF) for i in range(ART_LEN))


def fingerprint(devpub: bytes) -> str:
    return hashlib.sha256(devpub).hexdigest()[:8].upper()


def press_one(master: Presse, receiver: Presse, art: bytes | None = None) -> bytes:
    """Cut, pair, press: the state every transfer test starts from."""
    if art is not None:
        upload_art(master, art, SLOT_MASTER)
    master.cut(TITLE, EDITION, ARTIST)
    run_pairing(master, receiver)
    confirm_sas_both(master, receiver)
    return run_press(master, receiver, carry_from=master if art is not None else None)


def give_once(giver: Presse, taker: Presse, carry_art: bool = False) -> bytes:
    """A whole transfer, pairing included. The giver drives the pairing, the
    same position a master takes for a press."""
    run_pairing(giver, taker)
    confirm_sas_both(giver, taker)
    return run_give(giver, taker, carry_art=carry_art)


# --- the happy path and what it leaves behind -------------------------------


def test_a_copy_moves_and_the_giver_keeps_nothing(two):
    """The copy arrives whole on the taker, and the giver is left with neither
    the record nor the ability to prove it ever held it."""
    a, b = two
    cert = press_one(a, b)
    assert b.get_info()["has_pressing"] is True

    give_once(b, a)

    assert b.get_info()["has_pressing"] is False, "the giver still holds the copy"
    info_a = a.get_info()
    assert info_a["has_pressing"] is True
    assert info_a["has_master"] is True, "giving must not disturb the master"

    # The certificate did not change hands in name only: the taker answers the
    # challenge under the very key the certificate binds.
    stored_cert = a.cmd(0x40, p1=0)
    stored_album = a.cmd(0x40, p1=1)
    assert stored_cert == cert, "the certificate that arrived is not the one sent"
    result = verify_chain(stored_album, stored_cert)
    assert (result["number"], result["edition"]) == (1, EDITION)
    verify_possession(a, stored_cert)


def test_the_giver_can_no_longer_prove_possession(two):
    """Possession is the bearer key, so a device that handed it on cannot sign
    for the copy any more. It does not fall back to its device key: an answer
    from the wrong key would be worse than no answer."""
    a, b = two
    press_one(a, b)
    nonce = bytes(range(32))
    b.cmd(INS_CHALLENGE, nonce)  # while it still holds the copy

    give_once(b, a)

    assert b.cmd_sw(INS_CHALLENGE, nonce) == SW_NO_PRESSING


def test_the_same_copy_cannot_be_given_twice(two):
    """The receipt erases, so a completed transfer leaves the giver with nothing
    to offer, from its very first read."""
    a, b = two
    press_one(a, b)
    give_once(b, a)

    run_pairing(b, a)
    confirm_sas_both(b, a)
    assert b.cmd_sw(INS_GIVE_ALBUM) == SW_NO_PRESSING
    assert b.cmd_sw(INS_GIVE_OFFER) == SW_NO_PRESSING


# --- an interrupted transfer is a retry, never a loss -----------------------


def test_a_transfer_interrupted_after_the_offer_resumes(two):
    """The whole point of the commitment. The link dies with the key in flight:
    the giver has committed, the taker has nothing. Re-running the ceremony with
    the same two devices completes it, and the giver's human is not asked again
    because the commitment already records their answer."""
    a, b = two
    cert = press_one(a, b)

    run_pairing(b, a)
    confirm_sas_both(b, a)
    give_stage(b, a)
    _, sw = a.cmd_gated(INS_TAKE_CONFIRM, b"", "Receive it", "Receive ")
    assert sw == SW_OK
    give_offer(b)  # the sealed key is produced, and then dropped on the floor

    info_b = b.get_info()
    assert info_b["has_pressing"] is True, "a committed copy stays in flash to be re-sent"
    assert info_b["committed"] is True
    assert a.get_info()["has_pressing"] is False

    # A second run, same pair. give_offer() re-sends silently: no second gate.
    run_pairing(b, a)
    confirm_sas_both(b, a)
    run_give(b, a)

    assert b.get_info()["has_pressing"] is False
    assert a.get_info()["has_pressing"] is True
    stored = a.cmd(0x40, p1=0)
    assert stored == cert
    verify_possession(a, stored)


def test_a_transfer_interrupted_after_the_accept_resumes(two):
    """The copy landed and only the acknowledgement was lost. Re-running asks
    the taker for a fresh receipt, which is built from what it holds rather than
    from anything staged, and the giver lets go."""
    a, b = two
    press_one(a, b)

    run_pairing(b, a)
    confirm_sas_both(b, a)
    give_stage(b, a)
    _, sw = a.cmd_gated(INS_TAKE_CONFIRM, b"", "Receive it", "Receive ")
    assert sw == SW_OK
    a.cmd(INS_TAKE_ACCEPT, give_offer(b))

    # Both devices now hold the copy, which is exactly the window the receipt
    # closes; the giver's is already unusable.
    assert a.get_info()["has_pressing"] is True
    assert b.get_info()["committed"] is True
    assert b.cmd_sw(INS_CHALLENGE, bytes(32)) == SW_NO_PRESSING

    run_pairing(b, a)
    confirm_sas_both(b, a)
    finish_give(b, a)

    assert b.get_info()["has_pressing"] is False
    verify_possession(a, a.cmd(0x40, p1=0))


def test_a_committed_giver_refuses_a_different_recipient(two):
    """The commitment names one recipient and is never widened. A second device
    asking for the same copy is refused, which is what keeps the retry window
    from being a double-spend window."""
    a, b = two
    press_one(a, b)

    # B commits its copy to A, then the link dies.
    run_pairing(b, a)
    confirm_sas_both(b, a)
    give_stage(b, a)
    _, sw = a.cmd_gated(INS_TAKE_CONFIRM, b"", "Receive it", "Receive ")
    assert sw == SW_OK
    give_offer(b)
    assert b.get_info()["committed"] is True

    # A stranger now pairs with B and asks for the copy. It gets as far as the
    # handover signature -- which is inert without the key -- and no further.
    stranger = pair_as_taker(b)
    req = stranger.pub + stranger.mac_send(INS_PRESS_REQUEST, stranger.pub)
    assert b.cmd_sw(INS_GIVE_HANDOVER, req) == SW_OK
    assert b.cmd_sw(INS_GIVE_OFFER) == SW_BAD_STATE
    assert b.get_info()["has_pressing"] is True


def test_a_committed_giver_answers_no_challenge(two):
    """A copy promised away stops being this device's to claim the moment the
    commitment is written, long before it is erased. It is still in flash and
    still refuses to sign for itself."""
    a, b = two
    press_one(a, b)
    nonce = bytes(range(32))
    b.cmd(INS_CHALLENGE, nonce)  # while it is still B's to prove

    run_pairing(b, a)
    confirm_sas_both(b, a)
    give_stage(b, a)
    _, sw = a.cmd_gated(INS_TAKE_CONFIRM, b"", "Receive it", "Receive ")
    assert sw == SW_OK
    give_offer(b)

    assert b.get_info()["has_pressing"] is True
    assert b.cmd_sw(INS_CHALLENGE, nonce) == SW_NO_PRESSING

    # The row states an unfinished act, not a finished one: the copy is still
    # here and a re-run of the ceremony with that same recipient delivers it,
    # so the recipient is named. A row reading "given away" would tell the
    # owner the opposite of the truth and cost them the copy.
    assert b.dev.wait_for_text("promised"), b.dev.screen_texts()
    assert b.dev.wait_for_text(f"reconnect {fingerprint(a.get_info()['devpub'])}"), (
        b.dev.screen_texts()
    )
    assert not any("given away" in t for t in b.dev.screen_texts()), b.dev.screen_texts()


def test_a_receipt_from_the_wrong_device_does_not_release_the_giver(two):
    """The receipt is the only way out of a commitment, so it must name the
    device that was committed to. A stranger's acknowledgement, MACed under its
    own perfectly valid session, releases nothing."""
    a, b = two
    press_one(a, b)

    run_pairing(b, a)
    confirm_sas_both(b, a)
    give_stage(b, a)
    _, sw = a.cmd_gated(INS_TAKE_CONFIRM, b"", "Receive it", "Receive ")
    assert sw == SW_OK
    a.cmd(INS_TAKE_ACCEPT, give_offer(b))

    stranger = pair_as_taker(b)
    album_id, number, _, _, _, _ = parse_pressing_cert(a.cmd(0x40, p1=0))
    forged = album_id + struct.pack("<H", number) + stranger.pub
    forged += stranger.mac_send(INS_TAKE_RECEIPT, forged)
    assert b.cmd_sw(INS_GIVE_FINISH, forged) == SW_BAD_STATE
    assert b.get_info()["has_pressing"] is True


def test_a_taker_that_already_holds_a_copy_refuses_and_costs_nothing(two):
    """One copy per device, on this path as on every other. The taker is asked
    first, so it refuses while the giver still holds its copy outright and has
    been asked nothing: the common failure is free."""
    a, b = two
    press_one(a, b)

    # Give A a copy of its own without a ceremony (it cannot press to itself).
    priv, albpub = demo_album_key("Discovery", ARTIST, 12)
    other_album = build_album_cert(priv, albpub, "Discovery", 12, bytes(32), ARTIST)
    bearer_priv, bearer_pub = demo_bearer_key("Discovery", ARTIST, 12, 3)
    other_pressing = build_pressing_cert(
        priv, hashlib.sha256(albpub).digest(), 3, 12, bearer_pub
    )
    provision_pressing(a, other_album, other_pressing, bearer_priv)
    assert a.get_info()["has_pressing"] is True

    run_pairing(b, a)
    confirm_sas_both(b, a)
    give_stage(b, a)
    assert a.cmd_sw(INS_TAKE_CONFIRM) == SW_BAD_STATE

    info_b = b.get_info()
    assert info_b["has_pressing"] is True, "a refused transfer must cost the giver nothing"
    assert info_b["committed"] is False, "the giver must not have committed"
    verify_possession(b, b.cmd(0x40, p1=0))


def test_a_taker_who_declines_costs_the_giver_nothing(two):
    """The human on the receiving end is asked first, so saying no is free. The
    giver keeps the copy, uncommitted, and can still prove it holds it."""
    a, b = two
    press_one(a, b)

    run_pairing(b, a)
    confirm_sas_both(b, a)
    give_stage(b, a)
    _, sw = a.cmd_gated(INS_TAKE_CONFIRM, b"", "Cancel", "Receive ")
    assert sw == SW_DENY

    info_b = b.get_info()
    assert info_b["has_pressing"] is True
    assert info_b["committed"] is False
    assert a.get_info()["has_pressing"] is False
    verify_possession(b, b.cmd(0x40, p1=0))


def test_the_cover_travels_with_the_copy(two):
    """The record moves whole. Without the sleeve the taker would hold a genuine
    copy and render the generative fallback for its cover."""
    a, b = two
    art = a_sleeve()
    press_one(a, b, art=art)
    assert read_art(b, SLOT_PRESSING) == art

    give_once(b, a, carry_art=True)

    assert read_art(a, SLOT_PRESSING) == art
    assert a.dev.wait_for_text(TITLE), a.dev.screen_texts()


# --- a promise is not yet a handover ----------------------------------------
#
# The commitment has two halves, and the line between them is whether the sealed
# key has left. Before it does the copy exists in exactly one place, so the
# promise can be taken back without anybody ever holding two; after it does, it
# cannot, and no amount of the owner's certainty changes that.


def test_a_promise_whose_key_never_left_can_be_taken_back(two):
    """The stuck-forever case, unstuck. B promises its copy to a device that
    never comes back, and because the key never reached the wire the promise is
    still B's to withdraw. What it gets back is not a consolation prize: the copy
    proves possession again and goes on to a real transfer."""
    a, b = two
    press_one(a, b)

    stranger = pair_as_taker(b)
    req = stranger.pub + stranger.mac_send(INS_PRESS_REQUEST, stranger.pub)
    b.cmd(INS_GIVE_HANDOVER, req)
    give_commit(b)

    info = b.get_info()
    assert info["committed"] is True
    assert info["key_flown"] is False, "nothing was released, so nothing has flown"
    # A promised copy is as silent as a flown one: the difference between them
    # is reversibility, never what the copy is allowed to claim.
    assert b.cmd_sw(INS_CHALLENGE, bytes(32)) == SW_NO_PRESSING
    assert b.dev.wait_for_text("promised"), b.dev.screen_texts()

    give_cancel(b)

    info = b.get_info()
    assert info["committed"] is False, "the promise is still on the device"
    assert info["has_pressing"] is True, "cancelling must not cost the copy"
    verify_possession(b, b.cmd(0x40, p1=0))

    # And fully givable again, to a device that is not the one it was promised to.
    give_once(b, a)
    assert b.get_info()["has_pressing"] is False
    assert a.get_info()["has_pressing"] is True
    verify_possession(a, a.cmd(0x40, p1=0))


def test_a_flown_key_cannot_be_taken_back(two):
    """The invariant the whole feature rests on. Once the sealed key is on the
    wire the copy may exist on the taker too, so a device that could still
    un-promise it would be a double-spend primitive. Refused with its own status
    word, and without asking a human anything: there is nothing to decide."""
    a, b = two
    press_one(a, b)

    run_pairing(b, a)
    confirm_sas_both(b, a)
    give_stage(b, a)
    _, sw = a.cmd_gated(INS_TAKE_CONFIRM, b"", "Receive it", "Receive ")
    assert sw == SW_OK
    give_offer(b)

    info = b.get_info()
    assert info["committed"] is True and info["key_flown"] is True
    assert b.cmd_sw(INS_GIVE_CANCEL) == SW_KEY_FLOWN

    info = b.get_info()
    assert info["key_flown"] is True, "a refused cancel must change nothing"
    assert info["has_pressing"] is True
    assert b.cmd_sw(INS_CHALLENGE, bytes(32)) == SW_NO_PRESSING

    # And the transfer it refused to undo still completes.
    run_pairing(b, a)
    confirm_sas_both(b, a)
    run_give(b, a)
    assert b.get_info()["has_pressing"] is False
    verify_possession(a, a.cmd(0x40, p1=0))


def test_a_cancel_with_nothing_promised_is_refused(two):
    """Two different nothings, told apart: a device with no copy, and a device
    whose copy is simply its own. Neither is a promise, and neither draws a
    screen asking about one."""
    a, b = two
    press_one(a, b)

    assert a.cmd_sw(INS_GIVE_CANCEL) == SW_NO_PRESSING
    assert b.cmd_sw(INS_GIVE_CANCEL) == SW_BAD_STATE
    assert b.get_info()["has_pressing"] is True
    verify_possession(b, b.cmd(0x40, p1=0))


# --- the checks a lying relay has to get past -------------------------------


def pair_as_giver(taker: Presse) -> MitmEndpoint:
    """Pair a Python endpoint into the giver's position and have the taker's
    human confirm. From here the endpoint can build any frame it likes: what the
    taker still refuses is what the transfer's own checks catch, not what the
    channel catches."""
    giver = MitmEndpoint()
    eb = taker.cmd(INS_PAIR_RESPOND, giver.commitment())
    taker.cmd(INS_PAIR_FINISH, giver.pub)
    giver.derive(eb, as_master=True)
    confirm_sas_alone(taker)
    return giver


def pair_as_taker(giver: Presse) -> MitmEndpoint:
    """The mirror image: a Python endpoint in the taker's position, pairing with
    a real giver. Used to play a device the giver never committed to."""
    taker = MitmEndpoint()
    giver.cmd(INS_PAIR_COMMIT)
    ea = giver.cmd(INS_PAIR_REVEAL, taker.pub)
    taker.derive(ea, as_master=False)
    confirm_sas_alone(giver)
    return taker


def confirm_sas_alone(p: Presse):
    """Tap through the four words on the one real device in the pairing."""
    since = len(p.dev.events())
    thread, result = p.dev.apdu_async_start(apdu_hex(INS_PAIR_SAS))
    assert p.wait_for_text_since("Words match", since), p.dev.screen_texts()[since:]
    p.tap_text("Words match", since=since)
    thread.join(timeout=30)
    assert result["data"][-4:] == SW_OK


def stage_a_copy(taker: Presse, giver: MitmEndpoint, number: int = 1,
                 sig: bytes | None = None, giverpub: bytes | None = None,
                 hops: int = 0, chain: bytes | None = None,
                 signed_chain: bytes | None = None):
    """Push a relay-built copy's four pieces into the taker's staging slots.

    `sig` and `giverpub` default to a correct handover from this endpoint; a
    test overrides one of them to aim at exactly one check. `chain` is the head
    the endpoint hands over and `signed_chain` the head it actually signs: they
    differ only when a test is forging a history. `hops` gives the copy a trail
    of that many earlier holders, which is how a copy that has changed hands
    more times than two emulators can is reached in one step.
    Returns the bearer key that actually owns the copy."""
    priv, albpub = demo_album_key(TITLE, ARTIST, EDITION)
    album = build_album_cert(priv, albpub, TITLE, EDITION, bytes(32), ARTIST)
    album_id = hashlib.sha256(albpub).digest()
    bearer_priv, bearer_pub = demo_bearer_key(TITLE, ARTIST, EDITION, number)
    pressing = build_pressing_cert(priv, album_id, number, EDITION, bearer_pub)

    if chain is None:
        chain = chain_genesis(album_id, number)
    if sig is None:
        msg = handover_message(
            album_id, number, giver.pub, taker.get_info()["devpub"],
            chain if signed_chain is None else signed_chain,
        )
        sig = ecdsa_sign(giver.sk.to_string(), msg)

    taker.cmd(INS_TAKE_ALBUM, album + giver.mac_send(INS_GIVE_ALBUM, album))
    taker.cmd(INS_TAKE_PRESSING, pressing + giver.mac_send(INS_GIVE_PRESSING, pressing))
    chain_wire = chain + bytes([hops])
    assert len(chain_wire) == CHAIN_WIRE_LEN
    taker.cmd(INS_TAKE_CHAIN, chain_wire + giver.mac_send(INS_GIVE_CHAIN, chain_wire))
    handover = (giverpub or giver.pub) + bytes([len(sig)]) + sig.ljust(SIG_MAX_LEN, bytes(1))
    taker.cmd(INS_TAKE_HANDOVER, handover + giver.mac_send(INS_GIVE_HANDOVER, handover))
    return bearer_priv


def sealed_frame(giver: MitmEndpoint, bearer: bytes) -> bytes:
    """The 64-byte frame TAKE_ACCEPT consumes, built by hand."""
    payload = giver.bearer_xor(INS_GIVE_OFFER, giver.send_seq, bearer)
    return payload + giver.mac_send(INS_GIVE_OFFER, payload)


def test_a_bearer_key_that_does_not_match_the_certificate_is_refused(two):
    """The check a lying relay cannot get past. Everything else is impeccable --
    valid MAC, valid certificates, valid handover, a human who said yes -- and
    the copy is still refused, because the scalar handed over is not the one the
    album key signed a point for."""
    a, _ = two
    giver = pair_as_giver(a)
    stage_a_copy(a, giver)
    _, sw = a.cmd_gated(INS_TAKE_CONFIRM, b"", "Receive it", "Receive ")
    assert sw == SW_OK

    substitute = bytes(range(1, BEARER_KEY_LEN + 1))
    assert a.cmd_sw(INS_TAKE_ACCEPT, sealed_frame(giver, substitute)) == SW_BAD_CERT
    assert a.get_info()["has_pressing"] is False


def test_a_tampered_handover_signature_is_refused(two):
    """The one proven step of provenance is proven or the transfer does not
    happen. Caught at the confirmation, before a human is even shown a question:
    a signature that does not verify under the key beside it is not a handover."""
    a, _ = two
    giver = pair_as_giver(a)
    album_id = hashlib.sha256(demo_album_key(TITLE, ARTIST, EDITION)[1]).digest()
    msg = handover_message(
        album_id, 1, giver.pub, a.get_info()["devpub"], chain_genesis(album_id, 1)
    )
    sig = bytearray(ecdsa_sign(giver.sk.to_string(), msg))
    sig[1] ^= 0xFF

    stage_a_copy(a, giver, sig=bytes(sig))
    assert a.cmd_sw(INS_TAKE_CONFIRM) == SW_BAD_CERT
    assert a.get_info()["has_pressing"] is False


def test_a_handover_signed_for_another_recipient_is_refused(two):
    """The signature names both devices, so one lifted from another transfer
    does not legitimise this one."""
    a, _ = two
    giver = pair_as_giver(a)
    album_id = hashlib.sha256(demo_album_key(TITLE, ARTIST, EDITION)[1]).digest()
    stranger = SigningKey.from_string(bytes(range(1, 33)), curve=SECP256k1)
    elsewhere = stranger.get_verifying_key().to_string("uncompressed")
    msg = handover_message(album_id, 1, giver.pub, elsewhere, chain_genesis(album_id, 1))

    stage_a_copy(a, giver, sig=ecdsa_sign(giver.sk.to_string(), msg))
    assert a.cmd_sw(INS_TAKE_CONFIRM) == SW_BAD_CERT
    assert a.get_info()["has_pressing"] is False


def test_the_giver_named_in_the_frame_cannot_be_rewritten(two):
    """The giver's public key rides inside the MACed payload, so a relay that
    swaps in a name of its own breaks the signature it sits beside. Provenance is
    what the peer said it was, not what the laptop typed."""
    a, _ = two
    giver = pair_as_giver(a)
    impostor = SigningKey.from_string(bytes(range(2, 34)), curve=SECP256k1)

    stage_a_copy(a, giver, giverpub=impostor.get_verifying_key().to_string("uncompressed"))
    assert a.cmd_sw(INS_TAKE_CONFIRM) == SW_BAD_CERT
    assert a.get_info()["has_pressing"] is False


def test_an_ungated_accept_stores_nothing(two):
    """The accept carries no screen of its own, so the confirmation is the only
    thing between an incoming copy and NVM. Skipping it stores nothing."""
    a, _ = two
    giver = pair_as_giver(a)
    bearer = stage_a_copy(a, giver)
    assert a.cmd_sw(INS_TAKE_ACCEPT, sealed_frame(giver, bearer)) == SW_BAD_STATE
    assert a.get_info()["has_pressing"] is False


# --- what the screens say afterwards ----------------------------------------


def test_the_library_says_the_copy_was_given_away(two):
    """An empty shelf reads two ways and the giver needs the right one: the
    device that just let a copy go says so, rather than inviting a first cut."""
    a, b = two
    press_one(a, b)
    give_once(b, a)

    assert b.dev.wait_for_text("No records here"), b.dev.screen_texts()
    assert b.dev.wait_for_text("gave your copy away"), b.dev.screen_texts()
    # ...and says which device it is, which is the only thing an empty-handed
    # device can still be identified by.
    assert b.dev.wait_for_text(fingerprint(b.get_info()["devpub"])), b.dev.screen_texts()


def test_an_empty_device_names_itself(two):
    """A committed row says "reconnect XXXXXXXX", and the device that answers to
    it holds nothing, so it has no record card to open and no other way to show
    its name. The empty shelf carries it instead."""
    a, _ = two
    assert a.dev.wait_for_text("No records yet"), a.dev.screen_texts()
    assert a.dev.wait_for_text("Device ID"), a.dev.screen_texts()
    assert a.dev.wait_for_text(fingerprint(a.get_info()["devpub"])), a.dev.screen_texts()


def back_of_record(p: Presse) -> int:
    """Open the record listed in the library and turn to its back envelope.
    Returns the index the caller must read the event log from to see only what
    that page drew."""
    p.tap_text(TITLE)
    assert p.dev.wait_for_text("1 of 2"), p.dev.screen_texts()
    since = len(p.dev.events())
    p.tap_text("1 of 2")
    assert p.wait_for_text_since("Learn more", since), p.dev.screen_texts()[since:]
    return since


def open_row(p: Presse, label: str) -> int:
    """Tap a back-envelope row and wait for its sub-page. Returns the index to
    read that page's own elements from."""
    since = len(p.dev.events())
    p.tap_text(label)
    assert p.wait_for_text_since("Back", since), p.dev.screen_texts()[since:]
    return since


def test_the_previous_holder_is_named_beside_who_holds_it_now(two):
    """One step of provenance, on the page that already answers "which device
    holds this record": who holds it now, and who handed it over. A copy that
    came straight from a press says so, rather than leaving the reader to work
    out what a missing line means."""
    a, b = two
    press_one(a, b)

    back_of_record(b)
    since = open_row(b, "Device ID")
    assert b.wait_for_text_since("Pressed onto this device", since), b.dev.screen_texts()[since:]
    b.tap_text("Back", since=since)
    b.tap_text("Back")

    give_once(b, a)
    give_once(a, b)

    assert b.dev.wait_for_text(TITLE), b.dev.screen_texts()
    back_of_record(b)
    since = open_row(b, "Device ID")
    assert b.wait_for_text_since("the one handover", since), b.dev.screen_texts()[since:]
    assert b.wait_for_text_since(fingerprint(a.get_info()["devpub"]), since), (
        b.dev.screen_texts()[since:]
    )


def test_the_back_of_the_record_is_four_rows_whatever_the_copy_has_been_through(two):
    """The back envelope's height cannot depend on what the device holds.

    Its list area is four touchable bars tall, and a fifth does not scroll or
    wrap: it is drawn under the split footer with the tops of its glyphs showing
    above the rule, which reads as a rendering fault. A row that appears only
    for a copy that changed hands therefore breaks the page on exactly the
    devices nobody checks, so both states are asserted here."""
    a, b = two
    press_one(a, b)

    since = back_of_record(b)
    assert row_labels(b.dev, since) == BACK_ROWS, b.dev.screen_texts()[since:]
    assert_page_fits(b.dev, since)
    b.tap_text("Back", since=since)

    give_once(b, a)
    give_once(a, b)
    assert b.dev.wait_for_text(TITLE), b.dev.screen_texts()

    since = back_of_record(b)
    assert row_labels(b.dev, since) == BACK_ROWS, "the back grew a row once the copy moved"
    assert_page_fits(b.dev, since)


def test_a_long_history_does_not_grow_the_page_that_carries_it(two):
    """A copy can change hands more often than a screen has lines. Stated as a
    count the trail is the same height at two hundred hops as at one; listed by
    name it walks off the bottom of the screen, which faults the draw rather
    than merely clipping.

    Two hundred is past the thirty-two the old display ring could remember: the
    chain has no such limit, so neither has the page."""
    a, _ = two
    giver = pair_as_giver(a)
    bearer = stage_a_copy(a, giver, hops=200)
    _, sw = a.cmd_gated(INS_TAKE_CONFIRM, b"", "Receive it", "Receive ")
    assert sw == SW_OK
    a.cmd(INS_TAKE_ACCEPT, sealed_frame(giver, bearer))
    assert a.get_info()["has_pressing"] is True

    since = back_of_record(a)
    assert row_labels(a.dev, since) == BACK_ROWS
    assert_page_fits(a.dev, since)

    since = open_row(a, "Device ID")
    assert a.wait_for_text_since("Where it came from", since), a.dev.screen_texts()[since:]
    assert a.wait_for_text_since("200 more", since), a.dev.screen_texts()[since:]
    assert_page_fits(a.dev, since)


# --- the provenance chain ----------------------------------------------------
#
# A copy is a bearer key, so a copy that ever reaches software can be cloned, and
# every clone answers the possession challenge. Nothing here prevents that. What
# the chain does is make the *history* unforgeable, so that two clones which keep
# circulating produce two histories that diverge at one link, and that link names
# the device where the copy split.
#
# The head is 32 bytes and never grows, which is the only reason it fits: one
# signature per hop would be 72, and this app has neither the flash nor the NVM
# for thirty-two of them. The price of the digest is that a receiver cannot
# replay a history it is handed -- it holds a commitment, not the witnesses -- so
# what is refused on-device and what is only detectable afterwards are two
# different lists, and both are pinned below.


def test_a_copy_from_the_press_starts_at_the_root_its_certificate_implies(two):
    """"Straight from the press" is a claim with a shape, not the absence of one.

    The root is derived from the copy's own signed identity, so every verifier
    computes it and no two copies share one. A device asserting a bare "no
    history" is therefore checkable against its own certificate."""
    a, b = two
    cert = press_one(a, b)
    album_id, number, _, _, _, _ = parse_pressing_cert(cert)

    witness = read_witness(b)
    assert witness["has_from"] is False
    assert witness["hops"] == 0
    assert witness["chain"] == chain_genesis(album_id, number)
    assert verify_witness(witness, album_id, number, b.get_info()["devpub"])


def test_every_hop_folds_into_the_head_and_none_of_them_is_ever_dropped(two):
    """Two transfers, and the head replayed from the root through both links.

    This is what the display ring could not do: it kept four bytes a holder and
    dropped the oldest past thirty-two, so a much-travelled copy lost its
    beginning. The head is the same 32 bytes at one hop and at a thousand, and
    every hop is still inside it."""
    a, b = two
    cert = press_one(a, b)
    album_id, number, _, _, _, _ = parse_pressing_cert(cert)
    a_pub = a.get_info()["devpub"]
    b_pub = b.get_info()["devpub"]

    give_once(b, a)
    first = read_witness(a)
    assert first["has_from"] is True
    assert first["hops"] == 1
    assert first["giverpub"] == b_pub
    assert first["chain_prev"] == chain_genesis(album_id, number)
    assert verify_witness(first, album_id, number, a_pub)

    give_once(a, b)
    second = read_witness(b)
    assert second["hops"] == 2
    assert second["giverpub"] == a_pub
    # The second link extends exactly the head the first produced: the chain is
    # a chain, not two unrelated claims.
    assert second["chain_prev"] == first["chain"]
    assert verify_witness(second, album_id, number, b_pub)

    replayed = chain_genesis(album_id, number)
    replayed = chain_link(replayed, b_pub, first["sig"], a_pub)
    replayed = chain_link(replayed, a_pub, second["sig"], b_pub)
    assert replayed == second["chain"], "the head is not the history it claims"


def test_a_handover_signed_over_a_different_head_is_refused(two):
    """The head is not taken on trust: it is a field of the message the giver
    signs. Hand over one head and sign another and the signature simply does not
    verify, which is what stops a giver -- or a relay -- inventing a past."""
    a, _ = two
    giver = pair_as_giver(a)
    album_id = hashlib.sha256(demo_album_key(TITLE, ARTIST, EDITION)[1]).digest()

    stage_a_copy(a, giver, chain=bytes(range(32)),
                 signed_chain=chain_genesis(album_id, 1))
    assert a.cmd_sw(INS_TAKE_CONFIRM) == SW_BAD_CERT
    assert a.get_info()["has_pressing"] is False


def test_a_link_from_an_earlier_moment_of_this_copys_history_is_refused(two):
    """The attack the unsigned ring could not even see: a link that really was
    signed, by the right device, for the right recipient, about the right copy --
    and belongs to an earlier point of that copy's history.

    Replaying it would rewrite the trail while every signature still verified.
    It fails because the message names the head it extends, so a link is bound to
    one moment and reordering or grafting a history means forging a signature."""
    a, _ = two
    giver = pair_as_giver(a)
    album_id = hashlib.sha256(demo_album_key(TITLE, ARTIST, EDITION)[1]).digest()
    root = chain_genesis(album_id, 1)
    # A head one hop further on, which is where the copy actually is.
    later = chain_link(root, giver.pub, bytes(range(1, 71)), a.get_info()["devpub"])

    stage_a_copy(a, giver, chain=later, signed_chain=root)
    assert a.cmd_sw(INS_TAKE_CONFIRM) == SW_BAD_CERT
    assert a.get_info()["has_pressing"] is False


def test_a_link_lifted_from_another_copy_of_the_same_edition_is_refused(two):
    """Roots differ per copy, and every link commits to its root through the head
    it signs, so a genuine link out of copy #2's history cannot be used to give
    copy #1 a past it did not have."""
    a, _ = two
    giver = pair_as_giver(a)
    album_id = hashlib.sha256(demo_album_key(TITLE, ARTIST, EDITION)[1]).digest()

    stage_a_copy(a, giver, number=1, chain=chain_genesis(album_id, 1),
                 signed_chain=chain_genesis(album_id, 2))
    assert a.cmd_sw(INS_TAKE_CONFIRM) == SW_BAD_CERT
    assert a.get_info()["has_pressing"] is False


def one_signed_offer(giver: Presse):
    """Pair a Python taker with a real giver and take the two frames a fork is
    read out of: the head the giver is at, and its signature over the handover.

    Nothing on the giver changes here -- phase one is free to abandon -- which is
    exactly why a single signed link proves an offer and not a duplication."""
    taker = pair_as_taker(giver)
    chain_msg = giver.cmd(INS_GIVE_CHAIN)
    head = chain_msg[:32]
    req = taker.pub + taker.mac_send(INS_PRESS_REQUEST, taker.pub)
    handover = giver.cmd(INS_GIVE_HANDOVER, req)
    sig_len = handover[PUBKEY_LEN]
    return {
        "head": head,
        "giverpub": handover[:PUBKEY_LEN],
        "sig": handover[PUBKEY_LEN + 1 : PUBKEY_LEN + 1 + sig_len],
        "takerpub": taker.pub,
    }


def test_two_continuations_of_one_head_fork_the_chain_and_name_the_giver(two):
    """The evidence the whole design exists to produce.

    A device that duplicated a copy has to hand each duplicate on, and each
    handover is a link extending the head the copy was at. Two links out of one
    head, signed by one device key, toward two different recipients, is that
    device signing two futures for one copy. It is attributable and it does not
    rest on trusting anybody's story: both signatures verify under the giver's
    own key, over the same copy and the same head.

    What it proves on its own is a *fork*, not a duplication: an honest giver may
    offer a copy to several candidates before committing to one, and phase one
    changes nothing on either device. Duplication is proven when both branches go
    on to show possession, because a device signs a handover only for a copy it
    holds. The signatures are the attribution; possession is the proof."""
    a, b = two
    cert = press_one(a, b)
    album_id, number, _, _, _, _ = parse_pressing_cert(cert)
    b_pub = b.get_info()["devpub"]

    left = one_signed_offer(b)
    right = one_signed_offer(b)

    assert left["head"] == right["head"], "the copy did not move between the two"
    assert left["giverpub"] == right["giverpub"] == b_pub
    assert left["takerpub"] != right["takerpub"]

    for branch in (left, right):
        msg = handover_message(
            album_id, number, b_pub, branch["takerpub"], branch["head"]
        )
        assert ecdsa_verify(b_pub, msg, branch["sig"]), "a branch does not verify"

    forked = {
        chain_link(br["head"], br["giverpub"], br["sig"], br["takerpub"])
        for br in (left, right)
    }
    assert len(forked) == 2, "the two branches did not diverge"


def test_a_truncated_history_is_taken_here_and_only_a_comparison_catches_it(two):
    """The limit, stated rather than hidden.

    A receiver holds a digest, not the witnesses behind it, so it cannot replay
    what it is handed: one signature per hop is 72 bytes and thirty-two of them
    fit nowhere on this device. A giver running a modified app can therefore
    present its copy at the root and claim it never travelled, and this device
    will take it.

    What it cannot do is make that claim consistent. The link by which it
    received the copy names it as taker at a head of its own, and the copy it
    hands on carries a different one. The two witnesses are about the same copy
    and cannot both be true, and the device that signed both is named in each."""
    a, _ = two
    giver = pair_as_giver(a)
    album_id = hashlib.sha256(demo_album_key(TITLE, ARTIST, EDITION)[1]).digest()
    root = chain_genesis(album_id, 1)

    # What really happened: this endpoint received the copy from someone else,
    # so its true head is one link past the root.
    upstream = SigningKey.from_string(bytes(range(3, 35)), curve=SECP256k1)
    upstream_pub = upstream.get_verifying_key().to_string("uncompressed")
    true_head = chain_link(
        root,
        upstream_pub,
        ecdsa_sign(
            upstream.to_string(),
            handover_message(album_id, 1, upstream_pub, giver.pub, root),
        ),
        giver.pub,
    )

    # What it says: nothing ever happened.
    bearer = stage_a_copy(a, giver, chain=root)
    _, sw = a.cmd_gated(INS_TAKE_CONFIRM, b"", "Receive it", "Receive ")
    assert sw == SW_OK
    a.cmd(INS_TAKE_ACCEPT, sealed_frame(giver, bearer))
    assert a.get_info()["has_pressing"] is True

    witness = read_witness(a)
    # Internally impeccable: the device stored a chain that checks out.
    assert verify_witness(witness, album_id, 1, a.get_info()["devpub"])
    assert witness["hops"] == 1
    # And contradicted the moment the upstream link is put beside it: the giver
    # gave from the root, having received at a head that is not the root.
    assert witness["chain_prev"] == root
    assert true_head != root
    assert witness["chain_prev"] != true_head, "the truncation is undetectable"
