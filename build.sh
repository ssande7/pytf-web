#!/bin/bash
set -oue pipefail

source pyenv/bin/activate
cargo build --release
cd pytf-viewer
npm install
npm run build
