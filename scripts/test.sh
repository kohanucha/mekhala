#!/bin/bash

# --- Configuration ---
PORT=8787
LOG_FILE="test/wrangler.log"

# --- Cleanup Function ---
cleanup() {
    echo ""
    echo "Stopping wrangler dev server..."
    PID=$(lsof -ti :$PORT)
    if [ ! -z "$PID" ]; then
        kill -9 -$PID 2>/dev/null || kill -9 $PID 2>/dev/null
    fi
    PID2=$(lsof -ti :8788 2>/dev/null)
    if [ ! -z "$PID2" ]; then
        kill -9 $PID2 2>/dev/null
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

# 2.5. Check port availability
echo "Checking port $PORT..."
PIDS=$(lsof -ti :$PORT 2>/dev/null)
if [ ! -z "$PIDS" ]; then
    KILL_PIDS=""
    BLOCKED_PIDS=""
    for PID in $PIDS; do
        CMD=$(ps -p $PID -o command= 2>/dev/null)
        if echo "$CMD" | grep -qi "wrangler"; then
            KILL_PIDS="$KILL_PIDS $PID"
        else
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

    if [ ! -z "$KILL_PIDS" ]; then
        echo "Killing stale wrangler process(es) on port $PORT (PID$KILL_PIDS)..."
        for PID in $KILL_PIDS; do
            kill -9 $PID 2>/dev/null
        done
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
            echo "❌ Port $PORT still occupied after killing wrangler:"
            lsof -i :$PORT 2>/dev/null
            exit 1
        fi
        echo "Port $PORT is now free."
    fi
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
