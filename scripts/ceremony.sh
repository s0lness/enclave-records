#!/bin/bash
# Relais de la cérémonie live (deux Flex physiques), sur le checkout où vit ce
# script. La cérémonie ne lit ni APP_DIR ni APP_ELF: elle ne fait que du HID.
# Tous les arguments sont passés tels quels à relay/demo.py.
#   ceremony.sh                                  # cut + pair + press + verify
#   ceremony.sh --verify                         # vérification offline seule
#   ceremony.sh --collection a                   # parcourir la collection de A
#   ceremony.sh --title "..." --artist "..." --edition 5
set -e
source "$(dirname "$0")/env.sh"
cd "$PRESSE_ROOT"
exec python3 relay/demo.py "$@"
