#!/bin/bash
# Répétition de la cérémonie sur DEUX Speculos avec l'ELF Lot 1 de CE worktree.
# Aucun Flex physique n'est touché, aucun master réel consommé.
# demo_emu.py démarre lui-même les deux émulateurs (ports 5001 / 5002).
#   rehearse-emu.sh --auto   # il tape tout seul
#   rehearse-emu.sh          # vous tapez dans http://localhost:5001 et :5002
# Note: demo_emu.py n'uploade PAS la pochette avant le cut, contrairement au
# chemin matériel de relay/demo.py. La répétition couvre le protocole, pas le
# repaint de la pochette sur le receveur.
set -e
source ~/.cargo/env
export PATH=~/venv-ledger/bin:$PATH
export APP_DIR=/mnt/c/Users/sylve/projects/presse-video/device-app
export APP_ELF=$APP_DIR/target/flex/release/presse
pkill -f "speculos.*--api-port 500[12]" 2>/dev/null || true
sleep 1
cd /mnt/c/Users/sylve/projects/presse-video
exec python3 relay/demo_emu.py "$@"
