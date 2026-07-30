#!/bin/bash
# Run the pytest suite of this checkout, over one or two Speculos instances.
source "$(dirname "$0")/env.sh"
cd "$PRESSE_ROOT/tests"
exec pytest -x -q "$@"
