#!/bin/bash
set -e

# 1. Ensure Cargo is in PATH
export PATH="$HOME/.cargo/bin:$PATH"

# 2. Ensure Rust is installed
if ! command -v cargo &> /dev/null
then
    echo "Rust/Cargo not found, installing..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

# 3. Add Wasm target
rustup target add wasm32-unknown-unknown

# 4. Check if worker-build is installed
if ! command -v worker-build &> /dev/null
then
    echo "worker-build not found, installing..."
    cargo install worker-build
fi

# 5. Run the build
worker-build --release
