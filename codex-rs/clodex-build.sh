#!/bin/sh
# Clodex: detached build. Usage: setsid nohup ./clodex-build.sh <name> [profile] &
# profile: dev-small (default, unoptimized, ~40 s incremental) or fast (opt-level 3, no LTO).
name="${1:-clodex-build}"
profile="${2:-dev-small}"
cd "$(dirname "$0")" || exit 1
start=$(date +%s)
RUSTFLAGS="-C link-arg=-fuse-ld=mold" cargo build -p codex-cli --bin codex --profile "$profile" > "/tmp/$name.log" 2>&1
code=$?
echo "exit=$code secs=$(( $(date +%s) - start ))" > "/tmp/$name.done"
