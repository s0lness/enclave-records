#!/bin/bash
# Provisionne un pressage sur un Flex physique sans ceremonie (developpement).
# Meme motif que ceremony.sh: on ne source PAS env.sh, seul le python3 du venv
# est mis dans le PATH. Tous les arguments passent tels quels a relay/provision.py.
#   provision.sh                                   # #15 sur 20, RAM, sur le Flex A
#   provision.sh --device b --number 3 --edition 12
set -e
export PATH=~/venv-ledger/bin:$PATH
cd /mnt/c/Users/sylve/projects/presse-video
exec python3 relay/provision.py "$@"
