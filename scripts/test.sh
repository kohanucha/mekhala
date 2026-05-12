#!/bin/bash
set -e

# --- Configuration ---
PORT=8787

# --- Cleanup Function ---
cleanup() {
    echo ""
    sleep 1
    echo "Stopping wrangler dev server..."
    # Find the process listening on the port and kill its process group
    PID=$(lsof -ti :$PORT)
    if [ ! -z "$PID" ]; then
        # Use negative PID to kill the process group
        kill -9 -$PID 2>/dev/null || kill -9 $PID 2>/dev/null
    fi
    echo "Cleanup complete."
}

# Ensure cleanup runs on exit
trap cleanup EXIT

echo "🚀 Starting Mekhala Test Suite"
echo "=============================="

# 1. Run Rust Unit Tests
echo "Step 1: Running Rust unit tests..."
cargo test

# 2. Build for WASM
echo "Step 2: Building for WASM..."
./scripts/build.sh

# 3. Start Local Relay
echo "Step 3: Starting local relay on port $PORT..."
npx wrangler dev --port $PORT --ip 127.0.0.1 &
WRANGLER_PID=$!

# 4. Wait for Relay to be Ready
echo "Waiting for relay to start..."
MAX_RETRIES=60
COUNT=0
while ! lsof -i :$PORT > /dev/null; do
    sleep 1
    COUNT=$((COUNT + 1))
    if [ $COUNT -ge $MAX_RETRIES ]; then
        echo "❌ Error: Relay failed to start in time."
        exit 1
    fi
done
echo "Relay is up!"

# 5. Run Integration Tests
echo "Step 4: Running Node.js integration tests..."
cd test
npm test
TEST_RESULT=$?
cd ..

if [ $TEST_RESULT -eq 0 ]; then
    echo ""
    echo "✅ ALL TESTS PASSED!"
else
    echo ""
    echo "❌ TESTS FAILED! Check the output above."
fi

exit $TEST_RESULT
