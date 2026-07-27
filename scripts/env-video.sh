#!/bin/bash
# Wrapper: presse-video worktree. Sources env.sh then overrides APP_DIR/APP_ELF.
source "$(dirname "$0")/env.sh"
export APP_DIR=/mnt/c/Users/sylve/projects/presse-video/device-app
export APP_ELF=$APP_DIR/target/flex/release/presse
