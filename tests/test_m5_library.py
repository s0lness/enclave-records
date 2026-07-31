"""M5: the library landing screen, the record card, and the sleeve hash bound
into the album certificate.

The library is now the app's home screen. These tests drive it the way a
finger would (Speculos touch events located by OCR) and check the parts that
can be asserted without pixel comparison: the text the certificate drives, the
navigation, and -- cryptographically, through the independent verifier -- that
the sleeve is part of the signed identity of the edition."""

import struct

import pytest

from presse_client import (
    Presse,
    apdu_hex,
    assert_page_fits,
    current_screen,
    split_sw,
    parse_album_cert,
    verify_sleeve,
    upload_art,
    run_pairing,
    confirm_sas_both,
    run_press,
    ART_LEN,
    INS_COLLECTION,
    INS_CUT,
    SLEEVE_HASH_LEN,
    SW_OK,
)

TITLE = "Nocturne"
ARTIST = "Chopin"
EDITION = 5


def open_card_pages(dev, p, title=None):
    """From the library, open a record by tapping its row, then walk
    card -> back of record. The library is a list at any count, so the row is
    always the tap target and the footer carries only "Quit"."""
    p.tap_text(title or TITLE)
    assert dev.wait_for_text("1 of 2"), dev.screen_texts()  # the card pager
    p.tap_text("1 of 2")  # pager -> back of record
    assert dev.wait_for_text("Edition ID"), dev.screen_texts()


def a_sleeve(seed: int = 0) -> bytes:
    """A deterministic packed sleeve. The device only ever hashes these bytes,
    so the content is irrelevant; determinism keeps the test reproducible."""
    return bytes(((i * 7 + seed) & 0xFF) for i in range(ART_LEN))


# --- the library landing screen -----------------------------------------


def test_library_empty_state(device):
    """A fresh device opens on the library with the empty-state message."""
    assert device.wait_for_text("Enclave Records"), device.screen_texts()
    assert device.wait_for_text("No records yet"), device.screen_texts()


def test_library_lists_the_master_after_cut(device):
    """After a cut the library redraws to show the record: title from the
    certificate, plus the master status lines."""
    p = Presse(device)
    p.cut(TITLE, EDITION)
    assert device.wait_for_text(TITLE), device.screen_texts()
    # The row's status line: the "Master" role and "N of M left" on one line.
    # Only the master carries a role label; a pressing shows its "#N of M".
    assert device.wait_for_text("Master"), device.screen_texts()
    assert device.wait_for_text("5 of 5 left"), device.screen_texts()


def test_opening_the_record_opens_the_card(device):
    """Opening the record (by tapping its library row) shows its card (page 1
    of 2, with the pager chevrons), and Back returns to the library."""
    p = Presse(device)
    p.cut(TITLE, EDITION)
    assert device.wait_for_text(TITLE)
    p.tap_text(TITLE)
    # The card is page 1 of 2: the pager chevrons show.
    assert device.wait_for_text("1 of 2"), device.screen_texts()
    p.tap_text("Back")
    # Back on the library: the record is shown again.
    assert device.wait_for_text(TITLE), device.screen_texts()


def test_record_title_comes_from_the_certificate(device):
    """The title shown on the card is the certificate's, not baked into the
    bitmap: it survives whatever art (or none) is uploaded."""
    p = Presse(device)
    upload_art(p, a_sleeve())
    album_cert = p.cut(TITLE, EDITION, ARTIST)
    _, cert_title, cert_artist, _, _, _, _ = parse_album_cert(album_cert)
    assert cert_title == TITLE
    assert cert_artist == ARTIST
    p.tap_text(TITLE)
    assert device.wait_for_text(TITLE), device.screen_texts()
    p.tap_text("Back")


def test_cut_confirmation_names_the_artist(device):
    """The cut review reads "Cut master of <title> by <artist>?" so the artist
    is confirmed on-device before it is sealed."""
    title_b, artist_b = TITLE.encode(), ARTIST.encode()
    data = struct.pack("<HB", EDITION, len(title_b)) + title_b + artist_b
    thread, result = device.apdu_async_start(apdu_hex(INS_CUT, data))
    assert device.wait_for_text("Cut master of"), device.screen_texts()
    assert device.wait_for_text("by " + ARTIST), device.screen_texts()
    Presse(device).tap_text("Cut the master")
    thread.join(timeout=30)
    assert split_sw(result["data"])[1] == SW_OK


def test_front_of_card_shows_title_and_artist(device):
    """The front of the card carries the title with the artist under it (the
    artist coming straight off the certificate), over the big "#N" numeral."""
    p = Presse(device)
    p.cut(TITLE, EDITION, ARTIST)
    assert device.wait_for_text(TITLE)
    p.tap_text(TITLE)
    assert device.wait_for_text("1 of 2"), device.screen_texts()  # on the card
    assert device.wait_for_text(ARTIST), device.screen_texts()
    p.tap_text("Back")


def test_back_of_record_lists_the_envelope_info(device):
    """The back of the record is the envelope info, one (i) row per fact: the
    number ("#0 of 5" for a master), the Edition ID, the Device ID, and a
    Learn more row. No "Copy" tag, and the artist is not repeated here (it lives
    on the front)."""
    p = Presse(device)
    p.cut(TITLE, EDITION, ARTIST)
    assert device.wait_for_text(TITLE)
    open_card_pages(device, p)
    assert device.wait_for_text("#0 of 5"), device.screen_texts()  # master is #0
    assert device.wait_for_text("Edition ID"), device.screen_texts()
    assert device.wait_for_text("Device ID"), device.screen_texts()
    assert device.wait_for_text("Learn more"), device.screen_texts()
    p.tap_text("Back")


def test_edition_id_info_opens_its_page(device):
    """Tapping the Edition ID (i) row opens its sub-page: what it is and how to
    verify it, through the artist's official channels. Back returns to the
    record."""
    p = Presse(device)
    p.cut(TITLE, EDITION, ARTIST)
    assert device.wait_for_text(TITLE)
    open_card_pages(device, p)
    p.tap_text("Edition ID")  # the info affordance
    assert device.wait_for_text("How to verify"), device.screen_texts()
    assert device.wait_for_text("official channels"), device.screen_texts()
    p.tap_text("Back")  # sub-page -> back of record
    assert device.wait_for_text("Device ID"), device.screen_texts()


def test_device_id_info_opens_its_page(device):
    """Tapping the Device ID (i) row opens its sub-page: the fingerprint of the
    device that holds the record, and where the record came from. A master was
    cut here and is never handed on, so that is what its page says."""
    p = Presse(device)
    p.cut(TITLE, EDITION, ARTIST)
    assert device.wait_for_text(TITLE)
    open_card_pages(device, p)
    since = len(device.events())
    p.tap_text("Device ID")
    assert p.wait_for_text_since("Where it came from", since), device.screen_texts()[since:]
    assert p.wait_for_text_since("Cut on this device", since), device.screen_texts()[since:]
    assert_page_fits(device, since)
    p.tap_text("Back", since=since)
    assert device.wait_for_text("Edition ID"), device.screen_texts()


def test_learn_more_opens_the_model_limits(device):
    """Learn more leaves the record for the model's limits: what the device can
    and cannot prove.

    Both halves are asserted, because both are claims a buyer acts on. What is
    established is a signature: one album key over this edition, over this
    number and this edition size, plus possession of the record's key by
    whatever answered. What is not is the two things above and below that
    signature, the album key's owner and the nature of the responder, and a
    software clone answering exactly as this does is the reason the page can
    never say the device holds the copy.

    The page is seven lines of value inside a list area that has room for seven,
    so it is measured here as well as read."""
    p = Presse(device)
    p.cut(TITLE, EDITION, ARTIST)
    assert device.wait_for_text(TITLE)
    open_card_pages(device, p)
    since = len(device.events())
    p.tap_text("Learn more")
    assert p.wait_for_text_since("This device proves", since), device.screen_texts()[since:]
    texts = [e.get("text", "") for e in current_screen(device, since)]
    # A wrapped value arrives one OCR element per line, each ending in the space
    # it wrapped on, so a sentence is only readable once the runs are collapsed.
    joined = " ".join(" ".join(texts).split())
    assert "One album key signed this edition" in joined, texts
    # The number and the edition size, as the certificate signed them.
    assert "#0 of 5" in joined, texts
    assert "Whatever answers holds this record's key now" in joined, texts
    assert "It cannot prove" in texts, texts
    assert "That the album key is the artist's" in joined, texts
    assert "Software can answer exactly as this does" in joined, texts
    assert_page_fits(device, since)
    p.tap_text("Back", since=since)
    assert device.wait_for_text("Learn more"), device.screen_texts()


# --- the library redraws only when it could have changed ----------------


def test_bulk_art_upload_leaves_the_library_intact(device):
    """A full sleeve is ~50 SET_ART chunks. Streamed while the library is the
    screen on display, none of them disturbs the record it lists nor stalls the
    device: the library yields to each chunk but does not repaint per chunk (the
    on-hardware flicker/slowness this gating removes). Correctness is what we can
    assert here; the absence of a repaint is confirmed by screenshot on device."""
    p = Presse(device)
    p.cut(TITLE, EDITION)
    assert device.wait_for_text(TITLE), device.screen_texts()

    upload_art(p, a_sleeve())  # the burst, served with the library on screen

    # The library still lists the record, unchanged, and the device is still
    # serving commands: the burst neither corrupted the screen nor wedged it.
    assert device.wait_for_text(TITLE), device.screen_texts()
    assert device.wait_for_text("Master"), device.screen_texts()
    assert p.get_info()["title"] == TITLE


def test_press_repaints_the_receiver_with_the_pressing(pair):
    """The receiver repaints its library only when the pressing lands. Carrying
    the sleeve across before PRESS_ACCEPT means that single repaint shows the
    real, hash-verified cover. The pressing reads "#1 of 5" both in the library
    and on the back of the record, with the artist on the front."""
    a, b = pair
    master, receiver = Presse(a), Presse(b)

    art = a_sleeve()
    upload_art(master, art)  # sealed into the cut's signed sleeve hash
    master.cut(TITLE, EDITION, ARTIST)
    run_pairing(master, receiver)
    confirm_sas_both(master, receiver)
    run_press(master, receiver, carry_from=master)

    # B's library repainted on accept and now lists the pressing as "#1 of 5".
    assert b.wait_for_text(TITLE), b.screen_texts()
    assert b.wait_for_text("#1 of 5"), b.screen_texts()

    thread, res = b.apdu_async_start(apdu_hex(INS_COLLECTION))
    assert b.wait_for_text("1 of 2"), b.screen_texts()  # the card
    assert b.wait_for_text(ARTIST), b.screen_texts()    # artist on the front
    receiver.tap_text("1 of 2")  # pager -> back of record
    assert b.wait_for_text("#1 of 5"), b.screen_texts()
    receiver.tap_text("Back")
    thread.join(timeout=30)
    assert split_sw(res["data"])[1] == SW_OK


# --- the sleeve hash inside the album certificate -----------------------


def test_cut_binds_the_uploaded_sleeve_hash(device):
    """Art uploaded before the cut is hashed into the signed certificate: a
    third party can confirm the sleeve bytes against it, and a single flipped
    byte fails."""
    p = Presse(device)
    art = a_sleeve()
    upload_art(p, art)
    album_cert = p.cut(TITLE, EDITION)

    assert verify_sleeve(album_cert, art), "genuine sleeve must verify"

    tampered = bytearray(art)
    tampered[0] ^= 0x01
    assert not verify_sleeve(album_cert, bytes(tampered)), "tampered sleeve must fail"


def test_cut_without_art_binds_no_sleeve(device):
    """With nothing uploaded, the cut records the all-zero sentinel: the
    edition is signed as having no sleeve, and verification of any bytes
    against it fails."""
    p = Presse(device)
    album_cert = p.cut(TITLE, EDITION)
    _, _, _, _, sleeve_hash, _, _ = parse_album_cert(album_cert)
    assert sleeve_hash == b"\x00" * SLEEVE_HASH_LEN
    assert not verify_sleeve(album_cert, a_sleeve())


def test_mismatched_art_still_renders_the_record(device):
    """A device holding art whose hash does not match the certificate must
    fall back to generative art, not error: the record screen still opens and
    shows the title. (The fallback itself is visual; here we assert the flow
    does not fail closed on a benign mismatch.)"""
    p = Presse(device)
    upload_art(p, a_sleeve(seed=1))
    p.cut(TITLE, EDITION)
    # Overwrite the art so its hash no longer matches the sealed certificate.
    upload_art(p, a_sleeve(seed=99))

    thread, result = device.apdu_async_start(apdu_hex(INS_COLLECTION))
    # The card still opens (page 1 of 2) rather than failing closed.
    assert device.wait_for_text("1 of 2"), device.screen_texts()
    assert device.wait_for_text(TITLE), device.screen_texts()
    p.tap_text("Back")
    thread.join(timeout=30)
    assert split_sw(result["data"])[1] == SW_OK


# --- the ceremony still works with the library on screen ----------------


def test_ceremony_runs_with_the_library_as_home(pair):
    """The cut is a UI-gated APDU that arrives while the library is the screen
    on display. It must yield, run the review, and leave the master recorded:
    the whole reason the library runs an APDU-aware event loop."""
    a, b = pair
    master = Presse(a)
    assert a.wait_for_text("No records yet"), a.screen_texts()

    album_cert = master.cut(TITLE, EDITION)
    assert len(album_cert) > 0
    info = master.get_info()
    assert info["has_master"] and info["title"] == TITLE
    # The library redrew from fresh NVM and now lists the record.
    assert a.wait_for_text(TITLE), a.screen_texts()
