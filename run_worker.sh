#!/bin/bash
set -oeu pipefail
server="${1:-127.0.0.1:8080}"
key="${2:-foobar}"
source pyenv/bin/activate
cargo run --release pytf-worker -- "${server}" "${key}"
