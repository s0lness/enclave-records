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

When those two conflict, and they do conflict (see [Giving it
on](../README.md#giving-it-on) for the three states where they do),
**this design sacrifices the second**. Scarcity is the object: a system that
occasionally loses a record is a system with a bad day, a system that
occasionally duplicates one has nothing left to sell. So every ambiguous moment
resolves toward "possibly lost, definitely not doubled", and the engineering
effort goes into making the windows where that can happen small and visible
rather than into closing them with a mechanism that could also un-spend a copy.

## What the security actually rests on

Two things, and only one of them is cryptography.

**The secure element genuinely erasing the key.** When a giver completes a
handover, the protocol cannot prove to anyone that the scalar is gone from the
giver's flash. Nothing in a certificate, a signature or a receipt can establish
that. It is the chip's guarantee, not the protocol's: the app writes the erase
in a single power-loss-atomic NVM update, and BOLOS plus the secure element are
what make that write mean what it says. Extract a key from a Ledger secure
element and the whole thing falls over. That is the explicit bet, taken openly.

**Two humans reading four words aloud.** The pairing is commit-then-reveal ECDH
with a 32-bit short authentication string rendered as four words. A relay in the
middle running two handshakes ends up with two different session keys, hence two
different sets of words, and the humans are supposed to notice. Grinding is
blocked twice (the commitment forbids choosing an ephemeral key after seeing the
peer's, and pairing attempts are capped at 8 per power cycle), so an attacker
cannot search the 32-bit space online.

Say it plainly: **this is a human link, and it is the only way a second copy of
a record can ever come into existence.** A relay in the middle plus two people
who confirm without really comparing means the relay holds a session key on each
side, unmasks the bearer key in flight, and keeps a working copy of it. That
copy is permanent, undetectable, and indistinguishable from the real one,
because it *is* the real one. There is no revocation, no log, nothing to notice
later.

This is a real cost of transferability, and it is worth naming as such. In the
earlier device-bound design the same attack merely wasted a press. A copy that
can be handed on by sending a key can be stolen by reading that key. Which is
why the four words are the moment the whole ceremony is built around, and the
one moment nobody should rush.

## Why there is no attestation, and what that costs

The gap: nothing proves to a peer that the device across the cable is a genuine
Ledger running this unmodified app. A collector's Flex will happily verify a
certificate chain minted by a laptop pretending to be a master, and a
*modified* app could in principle press a sixth copy of an edition of five.

The reason this is not fixed is not that it was overlooked. It is that a Ledger
app cannot do it, and the honest version of it would have changed what the
project is:

- **A Ledger app cannot prove to a peer that it is a Ledger.** The endorsement
  mechanism (two slots, `ENDORSEMENT_SLOT_1` and `_2`) gives an app a key it can
  sign with, but the certificate an app can read back over that key is the one
  written by whichever host ran the provisioning. It attests to the *endorser*,
  not to Ledger. There is no syscall at this API level that hands an app a
  Ledger-signed device certificate to show a peer.
- **Ledger's own endorsement path is dead.** The HSM that used to sign
  endorsements, `hsmprod.hardwarewallet.com`, does not resolve at all (verified:
  NXDOMAIN).
- **Provisioning is a one-way door.** The revoke syscall is gone from the API 26
  bindings this app builds against. Two slots, no way back: burn them wrong and
  the device is done.

So the strongest true claim available was never "attested by Ledger". It was
"attested by Enclave Records, which checked this device's Ledger certificate at
press time" (option A), and it comes with a business model attached: Enclave
would have to be the party that provisions and ships the device with the record
on it, because that check can only happen while Enclave physically holds it. No
buying your own Flex and receiving a copy onto it.

**The choice made here was option B: any Ledger works, and the four words remain
the guard.** It is a priced decision, not an omission. What is bought: anyone can
join the edition with hardware they already own, and there is no company in the
trust path, which is the point of the object outliving the company. What is
paid: a modified app is not detectable by a peer, and the fallback against
over-pressing is fraud evidence rather than prevention (two certificates bearing
the same number are mutually incriminating, and a transparency log would make
that instant).

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

**And the one that is not a loss, though it looks like one:** *any* interruption
of a ceremony. A dropped frame, a yanked cable, a device that goes to sleep, a
take that ends mid-transfer. Re-run the ceremony with the same two devices and
it completes, and the screen tells you which device to go and get. Nothing about
an interrupted transfer costs the copy.

## Explicitly out of scope

- **Breaking the secure element.** Extracting a key from the chip breaks
  everything, and it is a stated bet, not a defended boundary.
- **The development provisioning path.** `relay/provision.py` plus
  `PROVISION_ALBUM` / `PROVISION_PRESSING` let a laptop act as the master: it
  mints the album key and the bearer key itself, signs both certificates, and
  pushes the result onto a device. It exists to stage a filmed scene (the honest
  route to "copy #15 of 20" is fourteen earlier presses to fourteen devices),
  and what it fabricates is *authorship*, documented as fictional. The device
  still runs every check a real receive runs, and still refuses to overwrite a
  copy it holds, so this adds a holding and never removes one. But note: it is
  **compiled into the current build, not behind a feature flag, and not gated by
  a confirmation on screen**, and its bearer key crosses the USB cable in the
  clear with no session to seal it. This is a lab prototype. Do not read the
  current binary as a shippable product.

## Deliberate non-goals

- **The cover is public, not secret.** The sleeve travels through the untrusted
  relay; integrity comes from the signed hash, not from secrecy. A swapped cover
  fails the hash and the device silently falls back to generative art. Fine for
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
receipt, over-pressing, and a swapped sleeve.
