#!/bin/bash
# Switch the C secure-sdk checkout to a given API_LEVEL branch, then rebuild.
# Default 26: that is the level this app builds against (see README, m5-hardware).
set -e
source "$(dirname "$0")/../env.sh"
BRANCH="API_LEVEL_${1:-26}"
cd "$FLEX_SDK"
git fetch --depth 1 origin "$BRANCH"
git checkout -q FETCH_HEAD
echo "secure-sdk now at: $(git log --oneline -1)"
