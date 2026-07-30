#!/bin/bash
# Provisionne un pressage sur un Flex physique sans ceremonie (developpement).
# Le relais signe lui-meme les certificats: ce qu'il fabrique est l'ATTRIBUTION,
# documentee comme fictive (voir docs/threat-model.md, hors perimetre).
# Tous les arguments passent tels quels a relay/provision.py.
#   dev/provision.sh                                   # #15 sur 20, RAM, sur le Flex A
#   dev/provision.sh --device b --number 3 --edition 12
set -e
source "$(dirname "$0")/../env.sh"
cd "$PRESSE_ROOT"
exec python3 relay/provision.py "$@"
