# M5: live demo on the two real Flexes

Everything up to here is green in emulation (11 pytest, `scripts/test.sh`).
This is the checklist for the hardware finale. Steps marked [manual] need a
human at the machine; the rest is scripted.

## One-time setup

1. **[manual] Update both Flexes** via Ledger Live to the latest firmware,
   then CLOSE Ledger Live (it locks the USB interface).
   The app is built against API_LEVEL 26 (`scripts/dev/sdk-checkout.sh 26`); if the
   device firmware runs another API level, re-run `dev/sdk-checkout.sh <level>` +
   `build.sh`. Level mismatch = app refuses to install/run.
2. **[manual, admin] Install usbipd-win** (UAC prompt):
   `winget install dorssel.usbipd-win`
   Then one-time per device, from an ADMIN PowerShell, with the Flex plugged:
   `usbipd list` (find the 2c97 busid), `usbipd bind --busid <ID>`.
3. **Attach USB to WSL** (each session): `scripts/windows/attach-usb.ps1`
   (both Flexes plugged in and unlocked).
4. **Sideload** (per device): `bash scripts/load.sh` from the repo root (on
   Windows: `wsl -d Ubuntu -- bash <repo>/scripts/load.sh`)
   Approve "Allow unsafe manager" on the device. Optional, to remove that
   prompt forever: `scripts/install-ca.sh` first.
   Load device A, then swap the attach to device B (or attach both and use
   `ledgerctl --device`), load again.

## The demo

Both Flexes unlocked, presse app open, both visible to the host (on Windows:
attached to WSL):

```
bash scripts/ceremony.sh
```

Beats, in order:
1. Cut: Flex A confirms "Cut master of Nuits Roses, edition of 5".
2. Pairing: both screens show the same 4 words. Say them out loud. Tap
   "Words match" on both. (To show the MITM defense live: run
   `tests/test_m4_adversarial.py` in emulation, or just explain it.)
3. Press: A confirms "Press Nuits Roses 1 of 5", B confirms "Receive".
4. Sold out (optional, repeat presses until refusal at 0).
5. Finale: turn OFF wifi, run `relay/demo.py --verify` with only Flex B
   plugged: "GENUINE: pressing 1 of 5, bound to this device, possession
   proven live." No network, no server, no chain.

## Known unknowns to watch at first contact

- API level drift between firmware and our build (step 1).
- OCR/UI layout differences between Speculos and the real panel don't matter
  here (no taps are simulated on hardware), but APDU timeouts do: UI-gated
  commands block until the human taps; the HID read has no timeout by design.
- Two devices enumerating: `relay/hid_device.py enumerate_ledgers()` should
  return 2 paths. If Windows/usbipd only exposes one at a time, attach both
  busids (scripts/windows/attach-usb.ps1 loops over all 2c97 devices).
- BOLOS endorsement (attestation, "layer 2") is NOT in v1: the demo's trust
  statement is layers 0+1 (captive keys + fraud-evident numbering).
