# Speculos loads only part of a Rust app's `.nvm_data`

Written as an upstream bug report for LedgerHQ/speculos, kept here because the
repo carries the fix locally (`scripts/patch-speculos.sh`,
`scripts/speculos-nvm-data.patch`) and the workaround in its build scripts.
Nothing has been filed on GitHub.

Measured on speculos 0.26.10 (`g93df48d34`), model flex, API level 26, host
Linux aarch64 (WSL Ubuntu on Windows 11 ARM), against a Rust app built with
`ledger_device_sdk` 1.36.0 and `ledger_secure_sdk_sys` 1.16.2.

Every offset below belongs to one build. `load_size` moves a few hundred bytes
per commit and the shortfall moves with it; the mechanism and the formula are
what generalise.

## Summary

For a Rust application, Speculos maps the ELF from the `PT_LOAD` segment that
contains `.text`, plus one page. `.nvm_data` is emitted as a *separate*
`PT_LOAD`, so it falls outside that size and only the bytes that happen to
share the spare page reach emulated memory: between 4096 and 7680 of them,
whatever the section's real size. The rest reads as unset memory.

Two consequences:

1. **The app can fail to start, silently.** An app whose `AtomicStorage` has
   both `SafeStorage` validity flags past the boundary reads 0 for both,
   `AtomicStorage::which()` hits `panic!("invalidated atomic storage")`
   (`ledger_device_sdk-1.36.0/src/nvm.rs:226`), and the app exits before
   answering its first APDU. The log shows `exit called (0)` and nothing else.
   The same binary boots on a physical device, where the `.hex` covers the
   whole region.
2. **When it does start, part of its NVM is quietly missing.** Everything past
   the boundary is zero-initialised, so an app reading it sees plausible empty
   data. In our app that hides the tearing-recovery copy (`storage_b`) and
   `install_parameters` in most builds.

The second one is the more dangerous of the two, because it produces green
tests over code paths that were never exercised.

## Mechanism

`speculos/main.py`, in `get_elf_infos`:

```python
        for seg in elf.iter_segments():
            if seg['p_type'] != 'PT_LOAD':
                continue
            if seg.section_in_segment(text_section):
                text_seg = seg
                break
        ...
        ei.text_offset = text_seg['p_offset']
        ei.text_size = text_seg['p_filesz']       # main.py:92
```

`text_size` is passed to the launcher as the `main:` argument
(`main:<path>:<text_offset>:<text_size>:...`), and `src/launcher.c` maps it:

```c
  /* load code
   * map an extra page in case the _install_params are mapped in the beginning
   * of a new page so that they can still be accessed */
  code = mmap(LOAD_ADDR, size + page_size, PROT_READ | PROT_EXEC,
              MAP_PRIVATE | MAP_FIXED, app->fd, app->elf.load_offset);
```

`mmap` rounds the length up to a page, so the bytes of `.nvm_data` that get
mapped are:

```
mapped = 4096 + ((-p_filesz) mod 4096)          # 4096 when p_filesz % 4096 == 0
```

between 4096 and 7680 on a 4096-byte host page. The block at `main.py:124-126`
that tests for `.nvm_data` only refuses `--load-nvram` / `--save-nvram` for Rust
apps; it leaves `app_nvram_addr` and `app_nvram_size` at 0 and has no effect on
what is mapped.

## Reproduction

The app used here has `.nvm_data` of 7582 bytes at file offset `0x22a00`, laid
out as `ART_MASTER` (2048), `ART_PRESSING` (2048), `AtomicStorage` (2560, its
two `SafeStorage` flags at section offsets 4096 and 5376), then
`install_parameters` (414 at offset 7168).

Program headers:

```
  Type   Offset   VirtAddr   PhysAddr   FileSiz MemSiz  Flg
  LOAD   0x010000 0xc0de0000 0xc0de0000 0x12a00 0x12a00 R E   .text .rel_flash .rodata
  LOAD   0x022a00 0xc0df2a00 0xc0df2a00 0x01d9e 0x01d9e RW    .nvm_data
```

Run it and read the host mapping (qemu-user maps the guest into its own address
space, so `/proc/<launcher pid>/maps` shows the real extent):

```
$ speculos --model flex --display headless --api-port 5099 --apdu-port 0 app.elf &
$ grep app.elf /proc/$(pgrep -f launcher)/maps
40000000-40014000 r--p 00010000 ...      # file 0x10000..0x24000
```

`.nvm_data` runs to file offset `0x2479e`, so 1950 bytes of it, including all of
`install_parameters`, never arrive. With `p_filesz` a multiple of 4096 the
mapping stops at section offset 4096 and the app exits at once.

Sweeping the app's size in 512-byte steps over 16 sizes, 96 boot runs, gives a
clean signature: every size with `p_filesz % 4096 == 0` fails, every other size
boots. Since `p_filesz` is `_erodata - _text` and includes the padding the
linker inserts between `.rel_flash` and `.rodata`, the failing sizes move
whenever the relocation count moves, which is what makes the failure look like
an unstable band of sizes rather than a periodic one.

## Fix

Extend the mapped size to the end of `.nvm_data`, after `main.py:92`:

```python
        nvm_section = elf.get_section_by_name('.nvm_data')
        if nvm_section is not None:
            nvm_end = nvm_section['sh_offset'] + nvm_section['sh_size']
            ei.text_size = max(ei.text_size, nvm_end - ei.text_offset)
```

Verified on this app: `text_size` goes from `0x12a00` to `0x1479e` and the
mapping covers `.nvm_data` whole. Re-run against the sweep above, all 16 sizes
boot 3/3, including the ones that died before. The spare page in `launcher.c` stays useful for the
C-app layout it was written for.

A generic version would take the maximum end over every `PT_LOAD` segment
rather than naming `.nvm_data`, which would also cover any future section that
lands past `.text`.
