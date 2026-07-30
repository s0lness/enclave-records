#!/bin/bash
# Cession d'un pressage entre les deux Flex physiques, sur le checkout où vit ce
# script. La cession ne lit ni APP_DIR ni APP_ELF: elle ne fait que du HID.
# Tous les arguments sont passés tels quels à relay/give.py.
#   give.sh                # paths[0] donne à paths[1]
#   give.sh --swap         # l'inverse
#   give.sh --no-cover     # sans transporter la pochette
#   give.sh --cancel       # reprend la promesse de paths[0] si la clé n'est pas partie
#                          # (un seul appareil suffit; refusé une fois la clé envoyée)
set -e
source "$(dirname "$0")/env.sh"
cd "$PRESSE_ROOT"
exec python3 relay/give.py "$@"
