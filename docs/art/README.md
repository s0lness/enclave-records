# Sleeves

A *sleeve* is an album's cover art: a packed 1-bit bitmap uploaded to a device
and drawn twice on Flex, as a decimated thumbnail on the record's library row and
at its full 128 px, with a mirror reflection under it, on the record card.

The sleeve carries **no typography**. The device draws the title itself, at
runtime, from the signed AlbumCert. A title baked into the artwork could
disagree with the certificate, and the certificate is the only part anyone
signed, so the bitmap stays pure art.

Everything here is produced by `scripts/sleeve.py`.

## Format

| | |
|---|---|
| size | **128 x 128** pixels |
| depth | 1 bit per pixel |
| length | **2048 bytes**, no header, no palette |
| polarity | bit `1` = white (lit), `0` = black |

128 is fixed. The device keeps **one art slot per record it can hold**, a
master and a pressing, because each sleeve is checked against the hash signed
into its own album certificate; a single shared region means the second cut
invalidates the first record's cover. The edge must be a multiple of 32,
because the device writes the art in 64-byte cells and `N*N/8` therefore has to
divide by 64, which rules out every value between 128 and 160. That cell rule
is the whole of it. Flash has room to spare: the per-app region is 400 KiB and
the app uses about 20% of it. The claim that used to sit here, a `data_size`
ceiling around 18432..19456 that stopped two 160-wide sleeves from booting, was
an artifact of a Speculos loading bug; see AGENTS.md and
docs/speculos-nvm-loading.md. `--size 160` and `--size 192` still work for
bench experiments; only 128 ships.

### Scan order (the part to cross-check)

**Not row-major.** For a pixel `(x, y)` of the image the device should
display, counting `x` right and `y` down from the top-left:

```
bit_index = (N - 1 - x) * N + y
byte      = bit_index // 8
bit       = 7 - (bit_index % 8)        # MSB = first pixel of its byte
```

Equivalently: the buffer is the intended image **rotated 90 degrees
counter-clockwise**, then scanned row-major, MSB first. The device rotates it
back when it draws.

Two things went into that, and they have different confidence levels:

* **The 90 degree rotation is measured, not assumed.** Two on-device renders
  agree:
  * `docs/screens/19-art-test.png`: the ART_TEST prototype fills a row-major
    buffer whose dark 16x16 square sits top-**left**; the Flex draws that
    square top-**right**.
  * A second render, of the 4bpp cover the project shipped at the time, agreed:
    decoding it column-major and high-nibble-first reproduced the screenshot
    exactly (correlation 1.0000 over the 64x64 art area; every other combination
    of row/column order and flip scored below 0.95). That screenshot is no longer
    in `docs/screens/`, so the ART_TEST render above is the evidence that
    survives in the tree.

  Both say: *what the screen shows is the row-major decode of the buffer,
  rotated 90 degrees clockwise.* So we pre-rotate counter-clockwise.

* **MSB-first inside a byte is an assumption.** It is the 1bpp analogue of
  the high-nibble-first order the 4bpp screenshot confirmed, but a reversed
  bit order shuffles pixels only within groups of 8 and is nearly invisible
  on real artwork, so no existing screenshot settles it.

  `test-pattern.bin` settles it. It is asymmetric under every flip, rotation
  and bit reversal:

  | feature | where it must appear |
  |---|---|
  | solid 20x20 square | top-left corner, starting on row 2 |
  | one lit pixel | row 1, two columns in from the right |
  | diagonal | top-left down to bottom-right |
  | 8-on / 8-off ruler | row 0, starting **lit** at x=0 |
  | solid 3-pixel band | bottom edge only |

  The ruler is one byte per run, so it is the bit-order test: if the square
  lands anywhere but the top-left, the rotation is wrong; if the square is
  right but row 0 starts dark instead of lit (the runs shifted by half a
  period), the bit order inside the byte is reversed, so flip the
  `bit = 7 - (bit_index % 8)` line in `pack_1bpp` to `bit = bit_index % 8`.

  Polarity is the third independent unknown: if the pattern comes out as a
  photographic negative, regenerate assets with `--invert`.

## Assets

Packed at 128x128 (2048 bytes each), the width `state.rs` pins: the device
keeps one art slot per record it can hold, and the 64-byte cell rule allows no
width between 128 and 160.

| file | album | sha256 of the packed bytes |
|---|---|---|
| `ram-cover.bin` | Random Access Memories | `c8b6da83eae4899b204d1b141766bbc28e09b151fc4053baef05da5825de2c08` |
| `eclipse-cover.bin` | Solar Debt | `3609722277460b1b2442c4f1414a49ec78b36822cabac63a42efb5573dddbd45` |
| `monolith-cover.bin` | Concrete Sleep | `8e7ccffc26086378538426277f83808b868ad595b596c3eb1a1521f3a5a6fe62` |
| `transit-cover.bin` | Null Island | `e48f9f9ccea2d5d3d8bc22d084be6c36b1874670054d25653aa85e0cfed29a88` |
| `test-pattern.bin` | (packing probe, not an album) | `56ba5b0628bad7769f175d15ad215acfded1884e0424d35910c0591867782eb1` |

Each has a `-preview.png` beside it: the same bits at 4x, nearest-neighbour.

All four covers are original graphic work, built from primitive shapes and
gradients in `scripts/sleeve_art.py`. The Random Access Memories sleeve is a
homage (two robot helmets under a stage light), not a reproduction of the
album photograph.

> **History:** the covers were 64x64 **4bpp** once and are 128x128 **1bpp**
> now. Both pack to 2048 bytes, which is `ART_LEN`, so the length has never
> moved. `scripts/make-cover.py` was the old 4bpp generator,
> since deleted (its packing comment was also wrong: it transposes instead
> of rotating, and puts the first pixel of a pair in the low nibble, which is
> why the shipped 4bpp cover renders mirrored and pair-swapped on device).

## Composing a new sleeve

Run it from the repo root, with the Python that has Pillow (on Windows that
means WSL, prefixed with `wsl -d Ubuntu --`):

```sh
python3 scripts/sleeve.py \
  --in cover-source.jpg \
  --out docs/art/my-cover.bin \
  --preview docs/art/my-cover-preview.png
```

The source can be any image of any size: it is centre-cropped to a square
(never squashed), resampled to 128x128, tone-mapped and dithered.

```
--dither atkinson|floyd|threshold   default atkinson
--size N                            default 128, multiple of 32
--gamma G                           default 1.2 for --in, 1.0 for --design
--clip F                            autocontrast percentile per end (0 = off)
--threshold T                       black/white decision level, default 128
--invert                            pack 1 = black
--test-pattern                      emit the packing probe instead
--design NAME                       one of the generated covers; `list` names them
```

Choosing a dither, from the comparison across a photo, a flat graphic and a
smooth gradient:

* **atkinson** (default): only propagates 6/8 of the error, so blacks stay
  black and whites stay white instead of being smeared with stray dots. Open
  texture, visible dot structure, the Mac Plus look. Best on all three
  inputs, and the reason the covers look designed rather than converted.
  Its one bias: discarded error darkens midtones slightly, which is what the
  default `--gamma 1.2` on photographic input cancels.
* **floyd**: tonally more accurate on gradients, but it fills large flat
  darks with noise and rings around high-contrast edges in flat graphics.
  Reach for it when a source is mostly continuous tone and you want fidelity
  over crispness.
* **threshold**: no diffusion. Destroys photographs and gradients (a
  gradient becomes one hard edge). Correct only for artwork that is already
  pure black and white and must not acquire any texture at all.

Flat graphics survive all three: solid black and solid white are fixed points
of both the tone curve and the dither, so they pass through untouched.

## Determinism and verification

Same input bytes, same flags, byte-identical output. That is what makes an
asset checkable: anyone with the source file can recompute the sleeve and
compare hashes with what a device holds.

How it is made true:

* every arithmetic step (square crop, Lanczos-3 resample, tone curve,
  dither, packing) is plain Python float64 inside `scripts/sleeve.py`. No
  library version, SIMD path or CPU difference can move a pixel. Pillow only
  decodes the input file and writes the preview PNG; it never touches the
  packed bytes.
* no timestamps, no RNG (none is imported), no locale-dependent parsing or
  formatting, no reliance on dict or filesystem ordering.
* the preview is a nearest-neighbour blow-up of the very bits that were
  packed, so it cannot disagree with the asset.

The tool prints the SHA-256 of the packed bytes on every run:

```
$ ... sleeve.py --design ram
c8b6da83eae4899b204d1b141766bbc28e09b151fc4053baef05da5825de2c08  128x128 1bpp 2048 bytes
```

To verify a stored asset, recompose it from its source with the same flags
and check that hash against the table above (or against
`sha256sum ram-cover.bin`). Regenerating every cover twice and diffing the
`.bin` and the `.png` is the standing check that determinism has not rotted.
