# Threat model

What this object promises, what those promises actually rest on, and every way it
can go wrong. Three documents, one job each: the
[README](../README.md) says what happens and why, [protocol.md](protocol.md)
carries the wire formats, the APDU map and the state machine, and this file
carries the case analysis. The README's threat-model section is the summary of
this one.

## The two promises

1. **A copy is never duplicated.**
2. **A copy is never lost by accident.**

They conflict (see [Giving it on](../README.md#giving-it-on) for the three states
where), and **this design sacrifices the second**. Scarcity is the object, so a
duplicate costs more than a loss. Every ambiguous moment resolves toward
"possibly lost, definitely not doubled", and the engineering effort goes into
making the windows where that can happen small and visible rather than into
closing them with a mechanism that could also un-spend a copy.

## What the security actually rests on

Two things: the secure element erasing a key, and two humans reading four words.

**The secure element genuinely erasing the key.** When a giver completes a
handover, the protocol cannot prove to anyone that the scalar is gone from the
giver's flash. Nothing in a certificate, a signature or a receipt can establish
that. That guarantee belongs to the chip: the app writes the erase
in a single power-loss-atomic NVM update, and BOLOS plus the secure element are
what make that write mean what it says. In RAM the scalar is overwritten with
volatile stores everywhere it is handled, so an optimiser cannot delete the erase;
`crypto::scrub` and its call sites in `press.rs` and `give.rs` are the whole of
that, and it takes `&mut` so a `Copy` array cannot be scrubbed by value. Extract a key from a Ledger secure
element and the whole thing falls over. That is the explicit bet, taken openly.

**Two humans reading four words aloud.** The pairing is commit-then-reveal ECDH
with a 32-bit short authentication string rendered as four words. A relay in the
middle running two handshakes ends up with two different session keys, hence two
different sets of words, and the humans are supposed to notice. Grinding is
blocked twice (the commitment forbids choosing an ephemeral key after seeing the
peer's, and pairing attempts are capped at 8 per power cycle), so an attacker
cannot search the 32-bit space online.

**This is a human link, and it is the one route to a second copy that the
ceremony itself can close.** A relay in the middle plus two people who confirm
without really comparing means the relay holds a session key on each side,
unmasks the bearer key in flight, and keeps a working copy of it. That copy is
permanent, undetectable, and indistinguishable from the real one, because it *is*
the real one. There is no revocation, no log, nothing to notice later.

Two other routes stay open and no amount of care at the ceremony touches them: a
modified app at either end, since nothing checks what runs there ([attestation is
not implemented](#attestation-and-why-it-is-not-implemented)), and a bearer key
that ever reaches ordinary software, where it can be copied at will. The
provenance chain addresses what happens to those copies afterwards, and the next
section is precise about how little that is.

This is a real cost of transferability. In the earlier device-bound design the
same attack merely wasted a press. A copy that can be handed on by sending a key
can be stolen by reading that key. Which is why the four words are the moment the
whole ceremony is built around, and the one moment nobody should rush.

## What the provenance chain adds, and what it does not

Nothing above changes: a copy that reaches software can be cloned, every clone
answers the possession challenge, and no protocol can tell them apart by looking
at one of them. The chain does not prevent that and is not offered as prevention.
It attacks the *second* half of the problem: a clone that never moves again is
worth little, and a clone that circulates has to be handed on, which is where it
becomes attributable.

**What is refused at the door.** The chain head is a field of the message the
giver signs, so a giver cannot hand over one head and sign another, and a link is
bound to one moment of one copy's history. Forging a link, editing a head in
flight, replaying a genuine link from earlier in that history, and grafting one
from another copy all need a signature over a head that never occurred. Those
four are the ones the old unsigned ring could not even see, since any holder
could simply rewrite its own trail.

**What is only caught afterwards, and why.** Storing the chain would mean 72
bytes of signature per hop; thirty-two hops is 2.3 KB, and this app has neither
the NVM nor the flash for it (see the window in AGENTS.md). So a device keeps a
32-byte rolling hash, and a receiver therefore holds a commitment rather than the
witnesses behind it: it cannot replay a history it is handed. Two consequences,
both accepted openly:

- **A modified giver can truncate.** It can present its copy at the root and
  claim it never travelled. This device will take it. The lie is not consistent
  with the link by which that device received the copy, which names it as taker
  at a head of its own, so the two witnesses are about one copy and cannot both
  be true. Detection happens by comparison, always after the fact.
- **A fork is an accusation, and a verdict takes more.** Two links out of one head,
  signed by one device key, toward two different recipients, is that device
  signing two futures for one copy, and both signatures verify under its own key,
  so no testimony is involved. But phase one of a give changes nothing on either
  device, so an honest holder shopping a copy around several candidates produces
  the same shape. **Duplication is proven when both branches show possession**: a
  challenge answered, or a further link, since a device signs a handover only
  for a copy it holds. The signatures give the attribution; possession gives the
  proof.

This is the same posture v1 already takes on over-pressing, where [attestation is
not implemented](#attestation-and-why-it-is-not-implemented) and the fallback is
fraud evidence after the fact. The chain moves
duplication from "permanent, undetectable, and indistinguishable from the real
one" to "detectable the moment two histories of one copy are put side by side,
and attributable to the device where they split". A transparency log of witnesses
would make that instant; nothing here needs one to exist.

**What would close the truncation hole**, and what it costs: binding the first
recipient into the PressingCert, so that "straight from the press" is a claim
only one device can make. It is four bytes in a signed certificate that today
deliberately names no device (`docs/protocol.md`, Press semantics), and it would
make every copy reveal its first owner to anyone who reads `GET_BUNDLE`. Not
taken here.

## A lone chain proves nothing

The chain is comparative evidence, and reading it as an attestation is the
mistake it invites. Handed one copy with a head and a hop count, you cannot tell
whether a clone of it is circulating: the clone's head is equally valid and
equally signed, and it descends from the same root, which is public anyway, since
the genesis is `SHA256("presse-chain" || album_id || number)` over two fields the
certificates already carry. A single chain is unfalsifiable in both directions.

What its holder knows on their own, with nothing to compare against:

- **the copy is a genuine member of the edition**: AlbumCert, PressingCert, and a
  live CHALLENGE answered by the bearer key the certificate names;
- **the last hop really happened**, where there has been one: the giver's
  signature over both device keys and over the head it received, checked at TAKE
  and stored beside the copy.

Both hold in isolation. Everything behind that last hop is a commitment whose
witnesses sit on other devices.

What the chain adds is time. It turns a duplication that was permanent and
undetectable into one that surfaces the day a second chain appears, which changes
the cloner's incentives without giving the buyer certainty today. You never judge
a painting's provenance from the painting.

## What a holder does with a chain

Three operations, each with a place it belongs.

**Compare two copies claiming the same number.** Two heads, one comparison, and
each device renders its own as eight words on the History page so the comparison
can happen out loud, once the copy has changed hands at least once. A freshly
pressed copy renders no words, because every copy of that number would render the
same ones. Identical heads with both devices answering CHALLENGE is
duplication proven: one history, two holders. Divergent heads say the lineages
split, and the hop where they split is found by walking the witnesses back from
each side (`GET_BUNDLE p1=2` on every device that held the copy). Where the two
witnesses already share `chain_prev`, the split is the last hop and both
branches carry one device's signature.

**Record what you saw.** A buyer, a gallery or a shop keeps the 204-byte witness
it read at the moment of a transaction. The registry is the sum of those records,
distributed across the people who made them; the public form it could take is
[designed and not built](../README.md#designed-but-not-built).

**Prove your own lineage when selling.** The head, the hop count, and the signed
link behind the last hop, which is the one hop a device proves alone. The count
is display only and no signature covers it: it arrives under the session MAC at
TAKE and the taker increments it, so it is each giver's claim in turn.

**Where this belongs.** The device holds and proves; comparing is a verifier's
job. `GET_BUNDLE p1=2` answers any host with no confirmation screen, which is
what makes all three possible off-device. Chain forensics on a 480x600 screen
that does not scroll would be the wrong thing to build. What the screen does
carry is the head itself, as eight words, because the first of the three
operations is two people reading to each other and neither of them is holding a
laptop.

**Decided: eight words of the head, on a page of their own.** How much of the
head a screen shows is an adversarial choice, because a forger invents every key
in his clone's fabricated history and can therefore grind links offline until
the clone's head agrees with the original's over whatever prefix is displayed.
Eight hex characters, the width of a Device ID, is 32 bits and falls to that
grind in minutes, at which point the two copies read alike to a human. **The
device renders the head's first eight bytes as eight words from the 256-word SAS
list**: 64 bits, the same as sixteen hex characters, and past the reach of an
offline grind. Words rather than hex because words are what people actually
compare, which is why the pairing renders words and not a digest, and because
reading them to each other is a ritual this object already has.

The derivation is in [protocol.md](protocol.md#the-provenance-chain), byte for
byte, so an independent verifier holding the 32-byte head from `GET_BUNDLE p1=2`
renders the same eight words. That matters more than the on-device page does:
the comparison is between a copy in someone's hand and a witness somebody else
wrote down, and only one of those two sides is a device.

They live on a sub-page of their own, reached from the Provenance page, for a
reason about page height. Every page here is fixed
height, and eight words wrap; a block whose height follows what the device holds
is the bug that puts a row under the footer, or past the bottom of the screen,
where the draw faults outright. So the words are a fixed grid of four lines by
two, the page is two tag/value pairs like the one it hangs off, and
`assert_page_fits` covers it at a fresh press and at two hundred hops. A master
is offered no such page: it is never handed on, its head stays at the all-zero
sentinel, and eight words derived from that would read identically on every
device ever made.

## Attestation, and why it is not implemented

The gap: nothing proves to a peer that the device across the cable is a genuine
Ledger running this unmodified app. A collector's Flex will happily verify a
certificate chain minted by a laptop pretending to be a master, and a
*modified* app could in principle press a sixth copy of an edition of five.

**The mechanism exists and works.** A Ledger app can prove to a peer that it is a
genuine Ledger running a specific binary, over three pieces: the **Issuer
Certificate** (Ledger's issuer key over the device public key, written at the
factory), the **Device Certificate** (the device's own key over the endorsement
public key), and a fresh **scheme-1 signature**, which is
`ECDSA(priv1, SHA256(message || app_code_hash))` where the hash is the *running*
app's, from `sys_endorsement_get_code_hash`. A verifier checks the peer's
signature against *its own* code hash, so a check that passes proves the peer
runs an identical binary, and no hash is ever transmitted. `rsksmart/rsk-powhsm`
ships a production instance of exactly this chain, and no Ledger service is
involved at any point.

**Correction, kept visible so nobody re-derives it.** An earlier version of this
section claimed a Ledger app cannot prove it is a Ledger, and blamed the dead
HSM at `hsmprod.hardwarewallet.com` plus a withdrawn revoke syscall. All three
are wrong. The call that reads back a certificate written by whichever host ran
the provisioning is `sys_endorsement_get_public_key_signature`, which returns
the **Owner Certificate**: unvalidated host bytes written verbatim at
`INS_ENDORSE_SET_COMMIT`, proving nothing at all. Generalising that one call to
the whole mechanism was the root error. The dead HSM sits nowhere in the chain
above. No endorsement revoke syscall has ever existed at any API level, and the
one-way property comes from the OS refusing to overwrite a slot already set,
which a full device wipe very likely frees (`bolos_erase_all` is closed source,
so that last point is inference). The same section concluded that the strongest
available claim would have forced Enclave to provision and ship every device;
peer-to-peer attestation drops that requirement, since any two devices check
each other.

**Why it is not implemented here:** cost, and three obstacles, heaviest first.

1. **The multi-issuer problem.** Ledger signs with several issuer keypairs. A
   device announces a 4-byte signer serial at `E0 50` naming which one signed its
   certificate, and no public tooling maps that serial to a key. ledgerblue reads
   the serial, discards it, and ships a single hardcoded constant labelled "batch
   1"; Ledger Live sidesteps the question by delegating the genuine check to
   Ledger's servers. A verifier compiled with one issuer key silently rejects
   genuine devices from other batches, with no diagnostic to hand their owners.
   That is an availability failure with no fix available from public sources.
2. **Ledger becomes a trust root.** Pinning an issuer key keeps verification
   offline forever: Ledger can vanish and the check still runs. It does require
   that Ledger was honest when it signed, and that the pinned key is the one it
   actually used. The README sells "no blockchain, no account, no server, the
   object outlives the company that made it", and a pinned issuer key puts a
   company back into that sentence. This is the real argument for the decision
   below.
3. **None of it can be exercised in Speculos.** The emulator implements neither
   scheme 2 nor `key1_get_app_secret` nor `get_metadata`, returns `0` (which
   reads as success) for the syscalls it does not implement while leaving the
   output buffer untouched, and hands out a constant fake code hash. The one
   thing attestation asserts, the identity of the running binary, is the one
   thing the emulator fakes. Development and testing would both happen on
   hardware or nowhere.

**And the bill, once those are cleared.** Two endorsement slots per device,
ever. Provisioning is one-way for as long as the device is not wiped, so a slot
burned wrong is spent. And the Device Certificate is returned exactly once, in
the response to `INS_ENDORSE_SET_START` (0xC0), and the OS never stores it:
capture it at provisioning and carry it beside the device, or that device can
never attest. The Issuer Certificate is out of an app's reach as well: a *host*
reads it over dashboard APDU `E0 52` inside an SCP session, which anyone can
open with a throwaway self-signed root and one confirmation on the device, with
no Ledger credential anywhere. Both certificates are public and self-verifying,
so a hostile relay can carry them safely.

**The decision: any Ledger works, and the four words remain the guard.** It is a
priced decision. What is bought: anyone can join the edition with hardware they
already own, no owner of a genuine Flex is turned away because their batch is
unknown, and no company sits in the trust path, which is the point of an object
that outlives the company that made it. What is paid: a modified app is
undetectable by its peer, and the guard against over-pressing is fraud evidence
after the fact (two certificates bearing the same number are mutually
incriminating, and a transparency log would make that instant).

## How a copy can be lost

Each of these is real, each has a window, and each has an ordinary physical
analogue. None of them can result in two copies.

- **The device is destroyed while the copy sits at rest.** Window: forever.
  Permanent, unrecoverable, and it is the holder's choice: this app declares no
  backup, so there is nothing to restore. Like a fire in a record collection.
- **The recipient's chip dies between the key flying and the receipt.** Window:
  a few seconds, cable connected, both devices in front of you. The giver is
  stuck at "flown" with a recipient that will never answer. Like the post losing
  a parcel while you watch.
- **A recipient who commits and never comes back.** Window: social, not
  technical. The giver's copy is promised and silent, and if the key has not
  flown yet it can be taken back with one tap. If it has, the copy is stuck for
  good. Like handing something to someone who walks off: nothing technical will
  fix it, and the fix that would (letting the giver un-promise it unilaterally)
  is exactly a double spend.
- **A firmware or app update with no succession.** Window: whenever you press
  the update button. App NVM does not survive a reinstall, and every update is a
  reinstall, so routine maintenance destroys the record. The procedure that
  avoids it is [Updates and
  survivability](../README.md#updates-and-survivability): succession, not backup.

**And the interruption that costs nothing:** *any* interruption of a ceremony. A
dropped frame, a yanked cable, a device that goes to sleep, a take that ends
mid-transfer. Re-run the ceremony with the same two devices and it completes, and
the screen tells you which device to go and get. Nothing about an interrupted
transfer costs the copy.

## What a copy reveals

There is no registry, so an edition's copies cannot be enumerated: nobody can
list the holders, or learn that a given copy exists at all, until its holder
produces it. The master's chip is the one exception, and a partial one. It holds
the press counter, so the artist knows how many were pressed, and it logs the
recipient fingerprints of the first eight pressings, which name the devices
those copies were pressed *onto* rather than whoever holds them now (the Flex
build has no screen that shows the log, and no command exports it).

What a copy does reveal, it reveals to whoever is holding it:

- **The number and the Edition ID**, public by design. `#N of M` and the
  fingerprint of the album key are what a buyer checks against the artist's
  channel.
- **The previous holder**, named by fingerprint on the Provenance page and proven
  by that device's signed handover record.
- **How far the copy has travelled**: a count of the holders before that one.
  The count is display only and saturates at 255; the chain head behind it is a
  32-byte commitment to every hop, in order, and forgets nothing however far the
  copy has gone. The head's first 64 bits are shown as eight words on the
  History page, and the whole of it to any host over `GET_BUNDLE p1=2`, so a
  holder can be compared against a witness. The names of the earlier holders are
  *not* revealed: a digest is not a roll, and only someone already holding a
  link can check it against one.
- **The two sides of a transfer learn each other.** The taker gets the giver's
  fingerprint inside the MACed handover frame, and the giver confirms the
  taker's on screen. A give that completes leaves the giver with a flag saying a
  copy went out, and nothing naming where.
- **A device on a cable answers `GET_INFO` and `GET_BUNDLE` to any host**, with
  no confirmation screen, so physical possession is enough to read the
  certificates, the title and the master's counter.

A copy is therefore closer to a record with a name written in the sleeve than to
a public ledger or to an anonymous token.

## Explicitly out of scope

- **Breaking the secure element.** Extracting a key from the chip breaks
  everything, and it is a stated bet.
- **Replaying a history from a digest.** A receiver cannot verify the hops
  behind the head it is handed. That is a size constraint, taken knowingly:
  see *What the provenance chain adds*. Auditing a history means collecting the
  witnesses (`GET_BUNDLE p1=2` on each device that held the copy, or a relay's
  log), which is off-device work this project does not automate.
- **The development provisioning path.** `relay/provision.py` plus
  `PROVISION_ALBUM` / `PROVISION_PRESSING` let a laptop act as the master: it
  mints the album key and the bearer key itself, signs both certificates, and
  pushes the result onto a device. It exists to stage a filmed scene (the honest
  route to "copy #15 of 20" is fourteen earlier presses to fourteen devices),
  and what it fabricates is *authorship*, documented as fictional. The device
  still runs every check a real receive runs, and still refuses to overwrite a
  copy it holds, so this adds a holding and never removes one. It is **compiled
  into the current build, not behind a feature flag, and not gated by a
  confirmation on screen**, and its bearer key crosses the USB cable in the clear
  with no session to seal it. This is a lab prototype. Do not read the current
  binary as a shippable product.

## Deliberate non-goals

- **The cover is public.** The sleeve travels through the untrusted relay;
  integrity comes from the signed hash, and nothing here is secret. A swapped cover fails
  the hash and the device silently falls back to generative art. Fine for
  artwork, not a model for private payloads.
- **Losing the master ends the edition.** The album key exists only in the
  master's chip and is never backed up. A master cannot be handed on either.
  This is "the plates are destroyed", chosen on purpose.
- **One pressing per device.** A device can hold its own master and one
  pressing, not two pressings.
- **Sideload only.** Not in Ledger's catalog.
- **The relay is assumed hostile and left that way.** No effort goes into
  making it trustworthy, because the design does not need it to be.
- **No backup, by construction.** BOLOS offers a place for app data to survive
  an update and this app declares none, because a restorable snapshot of its NVM
  would be a state-rollback primitive. The reasoning, and the succession
  procedure that replaces a backup, are in [Updates and
  survivability](../README.md#updates-and-survivability).

## Attack by attack, and whether it is tested

Each attack above has a line in [protocol.md's threat-model
status](protocol.md#threat-model-status), naming the defence and the test that
exercises it: relay key substitution, bearer-key substitution, rewriting who
gave, the fooled-humans MITM, replay, commitment cheating, giving the same copy
twice, a dropped frame mid-transfer, cancel used as a double spend, a forged
receipt, over-pressing, a swapped sleeve, rewriting a copy's history (forged,
head-substituted, replayed and grafted links), truncating one, and the fork a
circulating duplicate leaves behind.
