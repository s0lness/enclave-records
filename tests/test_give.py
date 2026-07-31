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
    SAS_WORDS,
    SIG_MAX_LEN,
    SLOT_MASTER,
    SLOT_PRESSING,
    SW_KEY_FLOWN,
    SW_NO_PRESSING,
    SW_OK,
    apdu_hex,
    assert_back_rows_are_one_line,
    assert_page_fits,
    build_album_cert,
    build_pressing_cert,
    chain_genesis,
    chain_link,
    confirm_sas_both,
    current_screen,
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
    head_words,
    parse_pressing_cert,
    provision_pressing,
    read_art,
    read_witness,
    row_labels,
    row_value,
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
BACK_ROWS = ["Number", "Edition ID", "Provenance", "Learn more"]


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


def test_a_receipt_from_a_clone_does_not_erase_an_undelivered_copy(two):
    """The receipt is built from what the taker *holds*, not from the transfer
    in progress, so a device that already holds a copy of the same edition and
    number can answer with that one while the ceremony has delivered nothing.
    The committed recipient is right, the album and number are right, the
    session MAC is right, and the copy still has to stay: what is missing is
    the delivery, and the giver reads it off its own state machine. Erasing
    here would hand the holder of a clone the power to destroy the original,
    and with it the fork in the provenance chain that names the duplicate."""
    a, b = two

    # One copy, held twice: B honestly, A as a clone. A copy that reaches
    # software can be cloned (docs/threat-model.md), which is the entry
    # condition and the only thing the attacker needs.
    priv, albpub = demo_album_key(TITLE, ARTIST, EDITION)
    album = build_album_cert(priv, albpub, TITLE, EDITION, bytes(32), ARTIST)
    album_id = hashlib.sha256(albpub).digest()
    bearer_priv, bearer_pub = demo_bearer_key(TITLE, ARTIST, EDITION, 1)
    pressing = build_pressing_cert(priv, album_id, 1, EDITION, bearer_pub)
    provision_pressing(b, album, pressing, bearer_priv)
    provision_pressing(a, album, pressing, bearer_priv)

    # B's human approves the give to A, and the relay then simply never asks
    # for the key: B stops at promised, and nothing has crossed to A.
    run_pairing(b, a)
    confirm_sas_both(b, a)
    b.cmd(INS_GIVE_HANDOVER, a.cmd(INS_PRESS_REQUEST))
    give_commit(b)
    info = b.get_info()
    assert info["committed"] is True
    assert info["key_flown"] is False, "nothing was released, so nothing has flown"

    # A acknowledges out of its own holding. Every field checks out.
    receipt = a.cmd(INS_TAKE_RECEIPT)
    assert b.cmd_sw(INS_GIVE_FINISH, receipt) == SW_BAD_STATE
    info = b.get_info()
    assert info["has_pressing"] is True, "a copy that never left must not be erased"
    assert info["key_flown"] is False

    # The promise is still B's to take back, and what comes back is the copy.
    give_cancel(b)
    verify_possession(b, b.cmd(0x40, p1=0))


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


def take_a_staged_copy(taker: Presse, giver: MitmEndpoint, hops: int = 0):
    """Stage a relay-built copy and let it land: confirm, then accept.

    `hops` is the trail the endpoint hands over, so the copy the taker ends up
    holding stands at `hops + 1` -- the endpoint's own handover is the newest
    one. This is how a copy that has changed hands more often than two emulators
    can reach is put on a device in one step."""
    bearer = stage_a_copy(taker, giver, hops=hops)
    _, sw = taker.cmd_gated(INS_TAKE_CONFIRM, b"", "Receive it", "Receive ")
    assert sw == SW_OK
    taker.cmd(INS_TAKE_ACCEPT, sealed_frame(giver, bearer))


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
    """One step of provenance, on the page the Provenance row opens: who holds it
    now, and who handed it over. A copy that came straight from a press says so,
    rather than leaving the reader to work out what a missing line means.

    The device that holds it is named first and by fingerprint, one tap from the
    envelope: it is the value a reader matches against another screen (a press's
    "For device", a promised row's "reconnect", an empty library's own name), and
    the page it now sits on is titled for the question it answers."""
    a, b = two
    press_one(a, b)
    # Both names read before the card is opened. GET_INFO is an APDU, the card
    # yields the screen to any APDU that arrives, and asking a device who it is
    # while its own card is up therefore closes the page under the assertion.
    a_fp = fingerprint(a.get_info()["devpub"])
    b_fp = fingerprint(b.get_info()["devpub"])

    back_of_record(b)
    since = open_row(b, "Provenance")
    assert "Provenance" in screen_texts_of(b, since), b.dev.screen_texts()[since:]
    assert b.wait_for_text_since(b_fp, since), b.dev.screen_texts()[since:]
    assert b.wait_for_text_since("Pressed onto this device", since), b.dev.screen_texts()[since:]
    assert_page_fits(b.dev, since)
    b.tap_text("Back", since=since)
    b.tap_text("Back")

    give_once(b, a)
    give_once(a, b)

    assert b.dev.wait_for_text(TITLE), b.dev.screen_texts()
    back_of_record(b)
    since = open_row(b, "Provenance")
    assert "Provenance" in screen_texts_of(b, since), b.dev.screen_texts()[since:]
    # Still the holder's own fingerprint at the top, then the device it came from.
    assert b.wait_for_text_since(b_fp, since), b.dev.screen_texts()[since:]
    assert b.wait_for_text_since("the one handover", since), b.dev.screen_texts()[since:]
    assert b.wait_for_text_since(a_fp, since), b.dev.screen_texts()[since:]
    assert_page_fits(b.dev, since)
    # And the way into the history is still the footer's right half.
    open_row(b, "History")


def test_the_back_of_the_record_is_four_rows_whatever_the_copy_has_been_through(two):
    """The back envelope's height cannot depend on what the device holds.

    Its list area is four touchable bars tall, and a fifth does not scroll or
    wrap: it is drawn under the split footer with the tops of its glyphs showing
    above the rule, which reads as a rendering fault. A row that appears only
    for a copy that changed hands therefore breaks the page on exactly the
    devices nobody checks, so both states are asserted here.

    The Provenance row is what moves with the copy, so its value is read in both
    states too: a copy still on the device it was pressed onto, then one that has
    been round the houses twice. The row carries the distance as a count, which
    is what keeps a travelled copy inside four rows."""
    a, b = two
    press_one(a, b)

    since = back_of_record(b)
    assert row_labels(b.dev, since) == BACK_ROWS, b.dev.screen_texts()[since:]
    assert row_value(b.dev, "Provenance", since) == "Pressed", b.dev.screen_texts()[since:]
    assert_back_rows_are_one_line(b.dev, since)
    assert_page_fits(b.dev, since)
    b.tap_text("Back", since=since)

    give_once(b, a)
    give_once(a, b)
    assert b.dev.wait_for_text(TITLE), b.dev.screen_texts()

    since = back_of_record(b)
    assert row_labels(b.dev, since) == BACK_ROWS, "the back grew a row once the copy moved"
    assert row_value(b.dev, "Provenance", since) == "2 hops", b.dev.screen_texts()[since:]
    assert_back_rows_are_one_line(b.dev, since)
    assert_page_fits(b.dev, since)


def test_a_long_history_does_not_grow_the_page_that_carries_it(two):
    """A copy can change hands more often than a screen has lines. Stated as a
    count the trail is the same height at two hundred hops as at one; listed by
    name it walks off the bottom of the screen, which faults the draw rather
    than merely clipping.

    Two hundred is past the thirty-two the old display ring could remember: the
    chain has no such limit, so neither has the page.

    This is also the widest the Provenance row ever gets: the label plus a
    three-digit count, right-aligned into a column that ends short of the
    chevron. `label_value_row` drops its padding once the pair is already that
    wide, so a row that outgrew the bar's 368px text area would wrap here first,
    and a wrapped row is taller than the three beside it, which moves them. This
    is the state that decided the row's wording: "201 handovers" measures 376px
    and wraps, where "201 hops" stays inside the value column."""
    a, _ = two
    take_a_staged_copy(a, pair_as_giver(a), hops=200)
    assert a.get_info()["has_pressing"] is True

    since = back_of_record(a)
    assert row_labels(a.dev, since) == BACK_ROWS
    assert row_value(a.dev, "Provenance", since) == "201 hops", a.dev.screen_texts()[since:]
    assert_back_rows_are_one_line(a.dev, since)
    assert_page_fits(a.dev, since)

    since = open_row(a, "Provenance")
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


# --- the head, read out loud -------------------------------------------------
#
# A chain is comparative evidence, so it is worth nothing until a human can put
# one copy's head beside what somebody else wrote down. The device renders the
# head's first eight bytes as eight words: 64 bits, which is what survives a
# forger grinding a fabricated history until it agrees with the original over
# whatever prefix a screen shows. Thirty-two bits, the width of a Device ID,
# does not.
#
# Every word asserted below is computed in this file: the head from the
# certificate (SHA-256 re-implemented in presse_client) and the words from this
# file's own copy of the list. Nothing here asks the device what it should be
# showing, so a screen that agrees with these agrees with an independent
# verifier, which is the only agreement worth having.


def history_page(p: Presse) -> int:
    """Card -> back envelope -> Provenance -> History. Returns the index the
    history page's own elements start at."""
    back_of_record(p)
    open_row(p, "Provenance")
    return open_row(p, "History")


# The value column of a tag/value page, measured on the emulator: it starts at
# x=32 and a full line runs to 448. A word line that reached past this would not
# error, it would wrap, and a wrapped line is a fifth line on a page whose whole
# claim is that it has four.
VALUE_COLUMN_LEFT = 32
VALUE_COLUMN_RIGHT = 448


def word_events(p: Presse, since: int) -> list:
    """The drawn elements whose text is nothing but entries of the list."""
    return [
        e
        for e in current_screen(p.dev, since)
        if e.get("text", "").split()
        and all(w in SAS_WORDS for w in e["text"].replace("\n", " ").split())
    ]


def word_lines(p: Presse, since: int) -> list:
    """The word lines drawn on the page, in order. OCR delivers a multi-line
    value either whole or one event per line, so both shapes are flattened."""
    lines = []
    for e in word_events(p, since):
        for line in e["text"].split("\n"):
            if line.split():
                lines.append(" ".join(line.split()))
    return lines


def two_per_line(words: list) -> list:
    """The eight words as the page lays them out: four lines of two."""
    return [" ".join(words[i : i + 2]) for i in range(0, len(words), 2)]


def test_the_word_rule_is_a_lookup_on_the_first_eight_bytes_of_the_head():
    """The rule an off-device verifier has to reproduce, pinned to literals.

    Each of the first eight bytes indexes the 256-word list, in the head's own
    order, and the ninth byte onward is not shown. Written out rather than
    derived, so the rendering cannot quietly drift on both sides of the
    comparison at once."""
    assert head_words(bytes(range(8)) + bytes(24)) == [
        "acrobat", "adrift", "almond", "amber", "anchor", "antique", "apple", "arena",
    ]
    assert head_words(bytes([255]) * 32) == ["walnut"] * 8
    # The ninth byte onward is not shown, so it cannot change what is.
    assert head_words(bytes(8) + bytes([7]) * 24) == ["acrobat"] * 8


def screen_texts_of(p: Presse, since: int) -> list:
    """The exact text of every element the page on display drew, so a line can
    be asserted whole. `wait_for_text` matches a substring, which cannot tell
    "1 handover" from "1 handovers"."""
    return [e.get("text", "") for e in current_screen(p.dev, since)]


def test_a_copy_that_has_not_moved_is_offered_no_words_to_compare(two):
    """A copy straight from the press sits at `chain_genesis(album_id, number)`,
    which is SHA-256 over an album id and a number anyone who has read the
    bundle already holds. Every copy claiming this number therefore renders the
    same eight words, and a reader comparing them would find a clone agreeing
    with an original.

    So the page withholds the words for as long as they say nothing, and states
    why. It stays reachable through the same footer: an affordance that appears
    and vanishes with the device's state is its own puzzle, and this page is the
    one place the answer can be written."""
    a, b = two
    cert = press_one(a, b)
    album_id, number, _, _, _, _ = parse_pressing_cert(cert)
    # The head really is the public root here: that is the whole reason the
    # words are withheld, and it is checked rather than assumed.
    assert read_witness(b)["chain"] == chain_genesis(album_id, number)

    since = history_page(b)
    assert word_lines(b, since) == [], b.dev.screen_texts()[since:]
    texts = screen_texts_of(b, since)
    assert "No handover yet" in texts, texts
    assert "Nothing to compare" in texts, texts
    assert any("starts on the same words" in t for t in texts), texts
    assert_page_fits(b.dev, since)


def test_the_words_appear_at_the_first_handover_and_name_the_hop_they_stand_for(two):
    """The boundary of the rule above, and the reason the words are dated.

    One handover folds a link into the head, which moves it off the root and
    makes it say something about this copy alone. The head is replayed here from
    the root through the link the copy really took, so the screen has to agree
    with the fold. The tag counts the handovers those words stand for: a head is
    a moment of a copy, and the next handover replaces it.

    One is also where the Provenance row turns singular, which is the only place
    the row's wording can be wrong without a count being wrong with it."""
    a, _ = two
    giver = pair_as_giver(a)
    take_a_staged_copy(a, giver)

    album_id = hashlib.sha256(demo_album_key(TITLE, ARTIST, EDITION)[1]).digest()
    root = chain_genesis(album_id, 1)
    witness = read_witness(a)
    assert witness["hops"] == 1
    replayed = chain_link(root, giver.pub, witness["sig"], a.get_info()["devpub"])
    assert witness["chain"] == replayed, "the head is not the history it claims"
    assert replayed != root, "one handover left the head on the public root"

    back = back_of_record(a)
    assert row_value(a.dev, "Provenance", back) == "1 hop", a.dev.screen_texts()[back:]
    assert_back_rows_are_one_line(a.dev, back)
    assert_page_fits(a.dev, back)

    open_row(a, "Provenance")
    since = open_row(a, "History")
    assert word_lines(a, since) == two_per_line(head_words(replayed)), (
        a.dev.screen_texts()[since:]
    )
    assert "Words after 1 handover" in screen_texts_of(a, since), (
        a.dev.screen_texts()[since:]
    )
    assert_page_fits(a.dev, since)
    # Each line inside the value column, which is the other half of "four
    # lines": a pair too wide for it wraps, and a wrapped pair is a fifth.
    for e in word_events(a, since):
        assert e["x"] >= VALUE_COLUMN_LEFT and e["x"] + e["w"] <= VALUE_COLUMN_RIGHT, e


def test_the_words_follow_the_head_when_the_copy_changes_hands(two):
    """A transfer folds a link into the head, so the words change with it. The
    new ones are replayed here from the root through both links the copy really
    took: the screen has to agree with the fold, not merely with itself.

    The copy goes out and comes back so the device read at the end holds a
    pressing and nothing else, which is the only state where "open the record"
    is unambiguous.

    The instruction under the words is asserted here too, because it is the
    operation they support: two copies of one number, in front of one reader, at
    one time. A head recorded at an earlier hop disagrees with a copy that has
    simply moved on, so a comparison against a note made elsewhere fails on the
    honest case."""
    a, b = two
    cert = press_one(a, b)
    album_id, number, _, _, _, _ = parse_pressing_cert(cert)
    a_pub = a.get_info()["devpub"]
    b_pub = b.get_info()["devpub"]
    at_press = head_words(chain_genesis(album_id, number))

    give_once(b, a)
    out = read_witness(a)
    give_once(a, b)
    back = read_witness(b)

    replayed = chain_genesis(album_id, number)
    replayed = chain_link(replayed, b_pub, out["sig"], a_pub)
    replayed = chain_link(replayed, a_pub, back["sig"], b_pub)
    assert replayed == back["chain"], "the head is not the history it claims"
    after = head_words(replayed)
    assert after != at_press, "the head did not move across the transfers"

    since = history_page(b)
    assert word_lines(b, since) == two_per_line(after), b.dev.screen_texts()[since:]
    texts = screen_texts_of(b, since)
    assert "Words after 2 handovers" in texts, texts
    assert "Copy beside copy, now" in texts, texts
    assert any("Both answering" in t for t in texts), texts
    assert any("lineages split" in t for t in texts), texts
    assert_page_fits(b.dev, since)


def test_the_history_page_is_one_height_however_far_the_copy_has_travelled(two):
    """The page's shape cannot follow the copy's history, and this is the page
    where that temptation is strongest: a list of hops is exactly what a reader
    would like and exactly what runs off a screen that does not scroll.

    The head is 32 bytes at one hop and at two hundred, and the words are a
    fixed grid of four lines by two, so this page is the four lines a copy
    straight from the press draws (asserted line for line above) at any distance
    travelled. Two hundred is past the thirty-two the old display ring held."""
    a, _ = two
    giver = pair_as_giver(a)
    bearer = stage_a_copy(a, giver, hops=200)
    _, sw = a.cmd_gated(INS_TAKE_CONFIRM, b"", "Receive it", "Receive ")
    assert sw == SW_OK
    a.cmd(INS_TAKE_ACCEPT, sealed_frame(giver, bearer))
    assert a.get_info()["has_pressing"] is True

    since = history_page(a)
    assert len(word_lines(a, since)) == 4, a.dev.screen_texts()[since:]
    assert_page_fits(a.dev, since)


def test_a_master_is_offered_no_history_to_compare(two):
    """A plate is never handed on, so its chain stays at the all-zero sentinel
    and eight words drawn from it would read the same on every device ever made.
    A master's Provenance page therefore carries the plain Back footer and no way
    into a history it does not have.

    Its row says the same thing one level up. A plate is where it was made and
    goes nowhere, so the row names the record: "0 hops" on a master would read as
    a copy that has not moved yet."""
    a, _ = two
    a.cut(TITLE, EDITION, ARTIST)
    assert a.dev.wait_for_text(TITLE), a.dev.screen_texts()

    back = back_of_record(a)
    assert row_labels(a.dev, back) == BACK_ROWS, a.dev.screen_texts()[back:]
    assert row_value(a.dev, "Provenance", back) == "Master", a.dev.screen_texts()[back:]
    assert_back_rows_are_one_line(a.dev, back)
    assert_page_fits(a.dev, back)

    since = open_row(a, "Provenance")
    assert a.wait_for_text_since("Cut on this device", since), a.dev.screen_texts()[since:]
    assert not any(
        "History" in e.get("text", "") for e in current_screen(a.dev, since)
    ), a.dev.screen_texts()[since:]
    assert_page_fits(a.dev, since)
