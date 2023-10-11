#!/bin/bash
set -oeu pipefail
users="${1:-test_users.csv}"
source pyenv/bin/activate

# Start the server
redis-server > redis.log 2>&1 &
cargo run --release --bin pytf-server -- -u "${users}"

# Make sure redis-server exits when the server shuts down
trap "trap - SIGTERM && kill -- -$$" SIGINT SIGTERM EXIT
