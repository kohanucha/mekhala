#!/bin/bash

# --- Configuration ---
PORT=8787
LOG_FILE="test/wrangler.log"

# --- Pre-flight: kill stale processes from prior runs ---
pkill -9 -f "wrangler dev" 2>/dev/null || true
pkill -9 -f "workerd" 2>/dev/null || true

# --- Cleanup Function ---
cleanup() {
    echo ""
    echo "Stopping wrangler dev server..."
    # Kill the background wrangler we started (SIGTERM then SIGKILL)
    if [ ! -z "$WRANGLER_PID" ]; then
        kill $WRANGLER_PID 2>/dev/null
        sleep 1
        kill -9 $WRANGLER_PID 2>/dev/null
    fi
    # Safety net: match any level of the wrangler dev process tree
    pkill -9 -f "wrangler dev" 2>/dev/null || true
    pkill -9 -f "workerd" 2>/dev/null || true
    # Port 8788 cleanup (from testMaxConnections in JS)
    lsof -ti :8788 2>/dev/null | xargs kill -9 2>/dev/null || true
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

# 2.5. Check port availability
echo "Checking port $PORT..."
PIDS=$(lsof -ti :$PORT 2>/dev/null)
if [ ! -z "$PIDS" ]; then
    BLOCKED_PIDS=""
    for PID in $PIDS; do
        CMD=$(ps -p $PID -o command= 2>/dev/null)
        if ! echo "$CMD" | grep -qi "wrangler"; then
            BLOCKED_PIDS="$BLOCKED_PIDS $PID"
        fi
    done

    if [ ! -z "$BLOCKED_PIDS" ]; then
        echo "❌ Port $PORT is in use by non-wrangler process(es):"
        lsof -i :$PORT 2>/dev/null
        echo ""
        echo "To free the port, run:"
        echo "  kill -9$BLOCKED_PIDS"
        exit 1
    fi

    echo "Waiting for port $PORT to be released..."
    RETRY=0
    while [ $RETRY -lt 5 ]; do
        if lsof -ti :$PORT 2>/dev/null | grep -q .; then
            sleep 1
            RETRY=$((RETRY + 1))
        else
            break
        fi
    done
    PIDS_AFTER=$(lsof -ti :$PORT 2>/dev/null)
    if [ ! -z "$PIDS_AFTER" ]; then
        echo "❌ Port $PORT still occupied after cleanup:"
        lsof -i :$PORT 2>/dev/null
        exit 1
    fi
    echo "Port $PORT is now free."
fi

# 3. Start Local Relay (redirect output to log file)
echo "Step 3: Starting local relay on port $PORT..."
npx wrangler dev --port $PORT --ip 127.0.0.1 > "$LOG_FILE" 2>&1 &
WRANGLER_PID=$!

# 4. Wait for Relay to be Ready
echo "Waiting for relay to start..."
MAX_RETRIES=60
COUNT=0
while ! curl -s http://127.0.0.1:$PORT > /dev/null 2>&1; do
    sleep 1
    COUNT=$((COUNT + 1))
    if [ $COUNT -ge $MAX_RETRIES ]; then
        echo "❌ Error: Relay failed to start in time."
        echo "Last wrangler output:"
        tail -50 "$LOG_FILE"
        exit 1
    fi
done
echo "Relay is up!"
sleep 2

# 5. Run Integration Tests
echo "Step 4: Running Node.js integration tests..."
cd test
npm test -- "127.0.0.1:$PORT"
TEST_RESULT=$?
cd ..

if [ $TEST_RESULT -eq 0 ]; then
    echo ""
    echo "✅ ALL TESTS PASSED!"
else
    echo ""
    echo "❌ TESTS FAILED! Check the output above."
    echo "--- Wrangler Logs ---"
    cat "$LOG_FILE"
    echo "---------------------"
fi

exit $TEST_RESULT
