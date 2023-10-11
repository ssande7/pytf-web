#!/bin/bash
set -oeu pipefail
users="${1:-test_users.csv}"
source pyenv/bin/activate

# Reload nginx config since we may have resized and want more workers
nginx -s reload || echo "WARNING: nginx not detected or failed to reload configuration"

# Start the server
redis-server&
cargo run --release --bin pytf-server -- -u "${users}"

