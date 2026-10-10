#!/bin/sh
# Clodex: detached thin build. Usage: setsid nohup ./clodex-build.sh <name> &
name="${1:-clodex-build}"
cd "$(dirname "$0")" || exit 1
start=$(date +%s)
RUSTFLAGS="-C link-arg=-fuse-ld=mold" cargo build -p codex-cli --bin codex --profile dev-small > "/tmp/$name.log" 2>&1
code=$?
echo "exit=$code secs=$(( $(date +%s) - start ))" > "/tmp/$name.done"
