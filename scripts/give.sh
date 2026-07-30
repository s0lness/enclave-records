#!/bin/bash
# Cession d'un pressage entre les deux Flex physiques (build Lot 1).
# Ne PAS sourcer scripts/env.sh: il épingle APP_DIR sur le checkout ../presse.
# La cession n'utilise ni APP_DIR ni APP_ELF, seulement le python3 du venv.
# Tous les arguments sont passés tels quels à relay/give.py.
#   give.sh                # paths[0] donne à paths[1]
#   give.sh --swap         # l'inverse
#   give.sh --no-cover     # sans transporter la pochette
#   give.sh --cancel       # reprend la promesse de paths[0] si la clé n'est pas partie
#                          # (un seul appareil suffit; refusé une fois la clé envoyée)
set -e
export PATH=~/venv-ledger/bin:$PATH
cd /mnt/c/Users/sylve/projects/presse-video
exec python3 relay/give.py "$@"
