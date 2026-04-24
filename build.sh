#!/bin/bash
set -e

# 1. Ensure Cargo is in PATH
export PATH="$HOME/.cargo/bin:$PATH"

# 2. Check if worker-build is installed
if ! command -v worker-build &> /dev/null
then
    echo "worker-build not found, installing..."
    cargo install worker-build
fi

# 3. Run the build
worker-build --release
