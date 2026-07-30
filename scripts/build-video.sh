#!/bin/bash
# Alias de build.sh, gardé parce que le runbook vidéo l'appelle par ce nom.
# build.sh construit désormais le checkout où il vit: les deux sont identiques.
exec "$(dirname "$0")/build.sh" "$@"
