#!/bin/bash

# --- Configuration ---
PORT=8787
LOG_FILE=$(mktemp)

# --- Cleanup Function ---
cleanup() {
    echo ""
    echo "Stopping wrangler dev server..."
    PID=$(lsof -ti :$PORT)
    if [ ! -z "$PID" ]; then
        kill -9 -$PID 2>/dev/null || kill -9 $PID 2>/dev/null
    fi
    echo "Cleanup complete."
    echo "Wrangler logs saved to: $LOG_FILE"
}
trap cleanup EXIT

echo "🚀 Starting Mekhala Test Suite"
echo "=============================="

# 1. Run Rust Unit Tests
echo "Step 1: Running Rust unit tests..."
cargo test
if [ $? -ne 0 ]; then
    echo "❌ Rust unit tests failed!"
    exit 1
fi

# 2. Build for WASM
echo "Step 2: Building for WASM..."
./scripts/build.sh
if [ $? -ne 0 ]; then
    echo "❌ WASM build failed!"
    exit 1
fi

# 3. Start Local Relay (redirect output to log file)
echo "Step 3: Starting local relay on port $PORT..."
npx wrangler dev --port $PORT --ip 127.0.0.1 > "$LOG_FILE" 2>&1 &
WRANGLER_PID=$!

# 4. Wait for Relay to be Ready
echo "Waiting for relay to start..."
MAX_RETRIES=60
COUNT=0
while ! lsof -i :$PORT > /dev/null 2>&1; do
    sleep 1
    COUNT=$((COUNT + 1))
    if [ $COUNT -ge $MAX_RETRIES ]; then
        echo "❌ Error: Relay failed to start in time."
        echo "Last wrangler output:"
        tail -20 "$LOG_FILE"
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
