#!/bin/bash
# Patch the installed Speculos so it loads the whole of a Rust app's .nvm_data.
#
# Stock Speculos maps the app from the PT_LOAD segment that holds .text and one
# spare page past it. `.nvm_data` is a separate segment, so between 4096 and
# 7680 of its bytes reach emulated memory and the rest never arrives. For this
# app that means a build whose load size is a multiple of 4096 panics before
# its first APDU, and every other build runs with part of its NVM missing
# (`storage_b`, `install_parameters`). See docs/speculos-nvm-loading.md.
#
# The emulator is patched in place, in $HOME/venv-ledger, because everything
# that runs it here (pytest, emu-up.sh, boottest.sh, relay/demo_emu.py, a human
# typing `speculos`) resolves it from that venv's PATH. A wrapper would only fix
# the callers that go through the wrapper.
#
#   patch-speculos.sh            apply if missing (idempotent)
#   patch-speculos.sh --check    report only; exit 0 patched, 1 not, 2 absent
#   patch-speculos.sh --revert   put the stock file back
#   patch-speculos.sh --quiet    apply, print only on change or failure
#
# `pip install -U speculos` reverts this. env.sh checks on every source and the
# build guard in build.sh/boottest.sh catches the fatal case regardless.
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PATCH="$HERE/speculos-nvm-data.patch"
MARKER="PRESSE-SPECULOS-NVM-PATCH"

MODE=apply
QUIET=0
for a in "$@"; do
  case "$a" in
    --check) MODE=check ;;
    --revert) MODE=revert ;;
    --quiet) QUIET=1 ;;
    *) echo "unknown argument: $a" >&2; exit 64 ;;
  esac
done
say() { [ "$QUIET" = 1 ] || echo "$@"; }

# The venv layout is the one env.sh puts on PATH; a speculos found on PATH by
# any other means is used as a fallback.
MAIN=""
for c in "$HOME"/venv-ledger/lib/python*/site-packages/speculos/main.py; do
  [ -f "$c" ] && MAIN="$c" && break
done
if [ -z "$MAIN" ] && command -v python3 >/dev/null 2>&1; then
  MAIN=$(python3 - <<'EOF' 2>/dev/null
import pathlib
try:
    import speculos
except Exception:
    raise SystemExit(0)
print(pathlib.Path(speculos.__file__).with_name("main.py"))
EOF
)
  [ -f "$MAIN" ] || MAIN=""
fi

if [ -z "$MAIN" ]; then
  echo "speculos not found (no \$HOME/venv-ledger, nothing importable)" >&2
  exit 2
fi

PATCHED=1
grep -q "$MARKER" "$MAIN" && PATCHED=0

case "$MODE" in
  check)
    if [ "$PATCHED" = 0 ]; then
      say "speculos patched: $MAIN"
      exit 0
    fi
    echo "speculos NOT patched: $MAIN" >&2
    echo "  .nvm_data is only partly loaded; run scripts/patch-speculos.sh" >&2
    exit 1
    ;;
  revert)
    if [ "$PATCHED" != 0 ]; then echo "already stock: $MAIN"; exit 0; fi
    patch -R -p1 -d "$(dirname "$(dirname "$MAIN")")" <"$PATCH" || exit 1
    echo "reverted, speculos is stock again: $MAIN"
    exit 0
    ;;
esac

if [ "$PATCHED" = 0 ]; then
  say "speculos already patched: $MAIN"
  exit 0
fi

if [ ! -f "$PATCH" ]; then
  echo "missing patch file: $PATCH" >&2
  exit 1
fi

SITE="$(dirname "$(dirname "$MAIN")")"
if ! patch -p1 --forward --dry-run -d "$SITE" <"$PATCH" >/dev/null 2>&1; then
  echo "cannot apply $PATCH to $MAIN" >&2
  echo "  speculos has moved get_elf_infos; re-cut the patch against this version" >&2
  echo "  and check whether upstream fixed the .nvm_data mapping itself" >&2
  exit 1
fi
patch -p1 --forward -b -d "$SITE" <"$PATCH" >/dev/null || exit 1
find "$SITE/speculos" -name '__pycache__' -prune -exec rm -rf {} + 2>/dev/null
echo "patched speculos so it loads all of .nvm_data: $MAIN"
echo "  stock file kept as $MAIN.orig, undo with scripts/patch-speculos.sh --revert"
