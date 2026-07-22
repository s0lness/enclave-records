# presse protocol v1 (as implemented)

Two roles per ceremony: **master** (device A, artist's plate) and **receiver**
(device B). All traffic goes through an untrusted relay (laptop, later a
phone/reader). The protocol stays secure when the relay lies, drops, replays,
reorders, or substitutes messages. The reference verifier is
tests/presse_client.py (python-ecdsa, shares no code with the device app).

Curve: secp256k1. Hash: SHA-256. MAC/KDF: HMAC-SHA256.
Pubkeys: 65-byte uncompressed SEC1. Signatures: deterministic ECDSA (RFC6979),
DER-encoded, length-prefixed, in a zero-padded 72-byte field.
Multi-byte integers little-endian.

## Keys

- **Device key** `devkey`: TRNG-generated at first use, secret scalar only in
  app NVRAM. Never seed-derived: the owner knows their 24 words, and a
  seed-derived key could re-press off-device. Public part `devpub`.
- **Album key** `albkey`: TRNG-generated at CUT, only in the master's NVRAM.
  `album_id = SHA256(albpub)`. Losing the master destroys the plates, by
  design.

## Certificates

### AlbumCert (223 bytes, signed by albkey)
```
magic        u8[4]   "PRA1"
albpub       u8[65]
title_len    u8
title        u8[32]  (utf-8, zero-padded)
edition      u16     (fixed forever at CUT)
sleeve_hash  u8[32]  SHA256 of the canonical sleeve bytes; all-zero = no sleeve
artist_len   u8
artist       u8[13]  (utf-8, zero-padded)
sig_len      u8
sig          u8[72]  (DER, zero-padded; covers bytes 0..150)
```

The signature covers `sleeve_hash` and `artist`, so both the cover art and the
artist name are part of the signed identity of the edition, fixed at CUT. The
art itself is public and travels separately (SET_ART / GET_ART); a device
renders it only when its bytes hash to this field, otherwise it shows generative
label art. See **Sleeve art**.

**Artist field and the APDU-size constraint.** `artist` is sealed at CUT beside
the title. The whole AlbumCert travels in one PRESS_LOAD_ALBUM command as
`cert || MAC(32)`, and a command's `Lc` may not exceed 255. That budget fixes
the artist cap: `223 + 32 == 255` exactly, so `artist` is capped at **13 bytes**
(a longer name is refused at CUT). The alternative, chunking GET_ALBUM /
PRESS_LOAD_ALBUM into two frames to carry a full ~32-byte artist, was rejected
for this lot: it would complicate the security-sensitive ceremony (per-frame MAC
and reassembly) for a cosmetic field. The cap keeps the ceremony single-frame
and unchanged. `artist` is placed after `sleeve_hash`, so every earlier offset
(albpub, title, edition, sleeve_hash) is unchanged from the 209-byte layout.

### PressingCert (178 bytes, signed by albkey)
```
magic      u8[4]   "PRP1"
album_id   u8[32]
number     u16     (1-based, unique, monotonic)
edition    u16
recvpub    u8[65]  (binds the pressing to one device's silicon)
sig_len    u8
sig        u8[72]  (covers bytes 0..105)
```

A verifier accepts a pressing iff: AlbumCert self-verifies, PressingCert
verifies under albpub, album_id == SHA256(albpub), editions match,
1 <= number <= edition, and the presenting device proves live possession of
recvpub via CHALLENGE. Because the album signature covers `sleeve_hash`, the
sleeve is authenticated transitively: a verifier that also holds the art bytes
accepts them iff SHA256(art) == sleeve_hash (and rejects an all-zero hash as
"no sleeve").

## Pairing (commit-then-reveal ECDH, 4-word SAS)

```
A: eph a, EA        A -> B : C = SHA256("presse-commit" || EA)
B: eph b, EB        B -> A : EB          (B stores C first)
A -> B : EA                              (B checks the hash, aborts hard on mismatch)
both:
  S  = ECDH_x(eph, peer_eph)
  T  = SHA256("presse-sas" || EA || EB)
  K  = HMAC(S, "presse-session" || T)    (session MAC key)
  SAS = HMAC(S, "presse-sas" || T)[0..4] -> 4 words (256-word list)
```

Both devices display the words and wait for a tap. A MITM relay running two
handshakes yields two different S, hence different words: the humans are the
authentication. Grinding is blocked twice: the commitment forbids choosing an
ephemeral after seeing the peer's, and pairing attempts are capped at 8 per
power cycle, so brute-forcing the 32-bit SAS online is out of reach.

After SAS confirmation, every ceremony payload carries
`HMAC(K, [ins, seq] || payload)` with per-direction sequence numbers. Any MAC
failure, SAS rejection, or power cycle kills the session.

## APDU map (CLA 0xB5)

```
0x01 GET_INFO       -> flags(1) devpub(65) edition(2) counter(2) title_len(1) title(32)
0x02 COLLECTION            [UI] -> ok   (opens the record card on screen)
0x10 CUT            data = edition(2) title_len(1) title(1..32) artist(0..13) [UI] -> AlbumCert
0x21 PAIR_COMMIT    (master)                    -> C(32)
0x22 PAIR_RESPOND   (receiver) data=C(32)       -> EB(65)
0x23 PAIR_REVEAL    (master)   data=EB(65)      -> EA(65)
0x24 PAIR_FINISH    (receiver) data=EA(65)      -> ok
0x25 PAIR_SAS       (both)                 [UI] -> sas(4)
0x30 GET_ALBUM      (master, paired)            -> AlbumCert || mac(32)
0x31 PRESS_REQUEST  (receiver, paired)          -> devpub(65) || mac(32)
0x32 PRESS_OFFER    (master)   data=devpub||mac [UI] -> PressingCert || mac
0x33 PRESS_LOAD_ALBUM (receiver) data=AlbumCert||mac  -> ok (staged)
0x34 PRESS_ACCEPT   (receiver) data=PressingCert||mac [UI] -> ok (stored)
0x40 GET_BUNDLE     p1=0 PressingCert, p1=1 its AlbumCert (public)
0x41 CHALLENGE      data=nonce(32) -> sig_len(1) || DER sig by devkey
                    over SHA256("presse-verify" || nonce)
0x50 RESET_MASTER   [UI, scary] -> wipes the master
0x62 SET_ART        data = offset(2 LE) || chunk(64)   -> ok   (public art upload)
0x64 GET_ART        p1 = chunk index                   -> chunk(<=64)
```
[UI] = blocks on an explicit user confirmation on the device screen, drawn
over the library (the landing screen), which yields to the incoming APDU.
Album + pressing certs travel in separate APDUs because both together exceed
the 255-byte APDU data limit.

Art must be uploaded (SET_ART) **before** CUT: the cut hashes the current art
region into the certificate's `sleeve_hash`. There is no separate seal step.
For a pressing, the sleeve is carried across with SET_ART and validated against
the `sleeve_hash` already inside the album cert the receiver stored.

## Sleeve art

A sleeve is the album cover: a square **1bpp** bitmap, `N x N`, `N*N/8` bytes,
no header. `N = 160` (3200 bytes) is the largest that fits the device's NVRAM
data budget (which tops out at 32256 bytes, 63 pages of 512). The device draws
the title itself, at runtime, from the certificate; the bitmap carries no
typography, so a baked-in title can never disagree with the signed one.

**Packing** (host side, `scripts/sleeve.py`; device rendering is the inverse).
The display decodes the buffer row-major and shows it rotated 90 degrees
clockwise, so the packer pre-rotates 90 degrees counter-clockwise. For image
pixel `(x, y)` of an `N x N` sleeve:

```
bit  = (N - 1 - x) * N + y
byte = bit / 8      mask = 0x80 >> (bit % 8)      # MSB = first pixel
```

This convention (rotate, MSB-first) was measured against on-device renders,
not assumed: `scripts/check-packing.py` correlates a screenshot against every
candidate and reports 1.0 only for this one.

**Polarity.** The canonical bytes are white-art-on-black: a **set bit is lit
(white) art**, matching every preview `sleeve.py` emits. These canonical bytes
are what SHA-256 hashes into `sleeve_hash`. This 1bpp render path, however,
draws a set bit *black*, so the device inverts the bits at draw time only; the
stored and hashed bytes are never touched. Hash the canonical asset, render its
complement.

**Binding and fallback.** `sleeve_hash` in the AlbumCert is SHA256 of the
canonical bytes, or all-zero when the edition was cut with no sleeve. A device
renders stored art iff the region is non-blank and `SHA256(art) == sleeve_hash`;
otherwise it draws generative label art derived from the album id. A mismatch
is rendered honestly (generative), never surfaced as an error.

## On-device UI

The app opens on the **library**: a list of the records the device holds, each
row a decimated sleeve thumbnail, the title from the certificate, and a status
line, over a "Quitter" footer that exits. Row status uses the "#N / M" family:
a pressing row reads `#1 / 5`, and the master's own row reads
`Your master · N of M left` (or `sold out`). The library runs an APDU-aware
event loop: it is the screen present when a ceremony begins, so it yields the
instant a command arrives, lets the main loop serve it, and redraws from fresh
NVM afterward. A ceremony therefore proceeds unchanged with the library on
screen. A command that draws its own screen (a UI-gated command, or the record
card) is served with the library first dropped, so the card gets the whole RAM
budget; a data-plane burst (a bulk sleeve transfer) leaves the library standing
and does not repaint per chunk.

Tapping a row opens the **record card**, a two-page generic review:

- **Page 1 of 2 — the record card.** A large `#N` on the left, the 160px cover
  to its right with a short Cover Flow mirror reflection (the cover flipped,
  ordered-Bayer dithered from ~0.55 at the seam to 0, strict 1-bit), and the
  album title in bold below, the block vertically centred. The number, cover and
  reflection are composited in RAM at draw time; nothing extra is stored in NVM.
  A master's card omits the `#N` (a plate is not a numbered copy). When no
  verified sleeve is loaded, the generative label art stands in. The `< N of 2 >`
  pager and "Back" sit in the footer.
- **Page 2 of 2 — the back of the record.** A tag/value list: Copy (`#N of M`,
  or `Master plate of M`), Artist, Album, and Edition ID (the first 8 hex of
  `SHA256(albpub)`). The Edition ID row carries a **compiled** circled-i glyph
  (`include_gif`, baked by build.rs — a runtime heap icon faults under PIC
  relocation on this target); tapping the row opens the authenticity page.

The **authenticity** page states what the device proves (this is copy #N of a
sealed edition of M, the artwork is genuine and unaltered, it is bound to this
device) and what it cannot (that the album key belongs to the real artist), with
the note to confirm the Edition ID on the artist's official channel, because a
copycat could reuse the same artwork under a different Edition ID.

The CUT confirmation names the artist: `Cut master of <title> by <artist>?`.

## Press semantics

- The master decrements its NVM counter (AtomicStorage, power-loss atomic)
  BEFORE the certificate leaves the device: a power cut burns a number, never
  duplicates one. Numbers are monotonic, never reused; no un-press.
- `number = edition - counter + 1`, refused at counter == 0 ("sold out").
- A receiver holds at most one pressing (v1).

## Threat model status

- Relay key substitution -> different SAS words, humans abort. (tested, M4)
- Fooled humans + tampered/rebound cert -> dies on ECDSA verify, since the
  signature covers recvpub and the MITM lacks the album key. (tested, M4)
- Replay -> sequence counters. Commitment cheating -> hard abort. (tested, M4)
- Over-pressing by the honest app: impossible (counter in silicon).
  Enforcement against a MODIFIED app rests on attestation (BOLOS endorsement),
  NOT in v1: v1's fallback is fraud-evidence (two certs with the same number
  are mutually incriminating). Open question tracked in docs/m5-hardware.md.
- Swapped/tampered sleeve -> SHA256(art) != signed sleeve_hash -> the device
  refuses the bitmap and shows generative art, so a forged cover can never be
  passed off under a genuine edition's certificate. (tested, M5)
- Cloning a receiver = extracting a key from the SE: the explicit bet.
