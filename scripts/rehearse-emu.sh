#!/bin/bash
# Répétition de la cérémonie sur DEUX Speculos avec l'ELF de CE checkout.
# Aucun Flex physique n'est touché, aucun master réel consommé.
# demo_emu.py démarre lui-même les deux émulateurs (ports 5001 / 5002).
#   rehearse-emu.sh --auto   # il tape tout seul
#   rehearse-emu.sh          # vous tapez dans http://localhost:5001 et :5002
# Note: demo_emu.py n'uploade PAS la pochette avant le cut, contrairement au
# chemin matériel de relay/demo.py. La répétition couvre le protocole, pas le
# repaint de la pochette sur le receveur.
set -e
source "$(dirname "$0")/env.sh"
pkill -f "speculos.*--api-port 500[12]" 2>/dev/null || true
sleep 1
cd "$PRESSE_ROOT"
exec python3 relay/demo_emu.py "$@"
