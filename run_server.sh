#!/bin/bash
set -oeu pipefail

users="${1:-}"
mode="${2:---release}" # Override build mode. Use " " for debug build
if [ ! "${users}" ]; then
  users="test_users.hashed"
  cargo run ${mode} --bin pytf-hash-users -- test_users.csv -o "${users}"
fi
source pyenv/bin/activate

# Start the server
redis-server > redis.log 2>&1 &
cargo run ${mode} --bin pytf-server -- -u "${users}"

# Make sure redis-server exits when the server shuts down
trap "trap - SIGTERM && kill -- -$$" SIGINT SIGTERM EXIT
