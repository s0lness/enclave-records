#!/bin/bash
# Relais de la cérémonie live (deux Flex physiques), build Lot 1.
# Ne PAS sourcer scripts/env.sh: il épingle APP_DIR sur le checkout ../presse.
# La cérémonie n'utilise ni APP_DIR ni APP_ELF, seulement le python3 du venv.
# Tous les arguments sont passés tels quels à relay/demo.py.
#   ceremony.sh                                  # cut + pair + press + verify
#   ceremony.sh --verify                         # vérification offline seule
#   ceremony.sh --collection a                   # parcourir la collection de A
#   ceremony.sh --title "..." --artist "..." --edition 5
set -e
export PATH=~/venv-ledger/bin:$PATH
cd /mnt/c/Users/sylve/projects/presse-video
exec python3 relay/demo.py "$@"
