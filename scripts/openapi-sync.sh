#!/bin/sh
# The Rust backend exports this with: cargo run -p holuto -- openapi
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
backend_dir=${BACKEND_DIR:-"$root/../backend"}
mkdir -p "$root/openapi"
cp -- "$backend_dir/openapi/agent.json" "$root/openapi/agent.json"
printf 'Copied %s/openapi/agent.json\n' "$backend_dir"
