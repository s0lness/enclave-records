#!/bin/bash
# Alias de load.sh, gardé parce que le runbook vidéo l'appelle par ce nom.
# load.sh sideloade désormais le checkout où il vit: les deux sont identiques.
exec "$(dirname "$0")/load.sh" "$@"
