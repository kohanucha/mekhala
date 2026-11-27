#!/bin/bash

# --- Configuration ---
PORT=8787
LOG_FILE="test/wrangler.log"

# --- Pre-flight: kill stale processes from prior runs ---
# `wrangler dev` runs as `node .../wrangler-dist/cli.js dev ...`, so match that
# cmdline too: a server left behind on another port keeps the shared build
# output busy and the suite then hangs at "Starting local relay".
pkill -9 -f "wrangler-dist/cli.js dev" 2>/dev/null || true
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
    pkill -9 -f "wrangler-dist/cli.js dev" 2>/dev/null || true
    pkill -9 -f "wrangler dev" 2>/dev/null || true
    pkill -9 -f "workerd" 2>/dev/null || true
    # Port 8788 cleanup (from testMaxConnections in JS)
    lsof -ti :8788 2>/dev/null | xargs kill -9 2>/dev/null || true
    # Port 8789/8791/8792 cleanup (from connection-limits tests in JS)
    lsof -ti :8789 2>/dev/null | xargs kill -9 2>/dev/null || true
    lsof -ti :8791 2>/dev/null | xargs kill -9 2>/dev/null || true
    lsof -ti :8792 2>/dev/null | xargs kill -9 2>/dev/null || true
    # Port 8790 cleanup (from testRestartPersistence in JS)
    lsof -ti :8790 2>/dev/null | xargs kill -9 2>/dev/null || true
    # Port 8793/8794 cleanup (from info-cache tests in JS)
    lsof -ti :8793 2>/dev/null | xargs kill -9 2>/dev/null || true
    lsof -ti :8794 2>/dev/null | xargs kill -9 2>/dev/null || true
    # Port 8802 cleanup (from the message budget test in JS)
    lsof -ti :8802 2>/dev/null | xargs kill -9 2>/dev/null || true
    # Port 8803 cleanup (from the orphaned connection test in JS)
    lsof -ti :8803 2>/dev/null | xargs kill -9 2>/dev/null || true
    # Port 8804 cleanup (from the relay-without-secret test in JS)
    lsof -ti :8804 2>/dev/null | xargs kill -9 2>/dev/null || true
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
# The connection gates are switched off for the functional suite: every test
# socket arrives from 127.0.0.1 (one address, one network) and several tests
# hold sockets open while waiting, so the gate would refuse healthy traffic.
# Each gate has a dedicated test that starts its own relay — see
# test/connection-limits.test.js.
npx wrangler dev --port $PORT --ip 127.0.0.1 \
    --var CONN_MAX_PER_IP:0 \
    --var CONN_MAX_PER_ASN:0 \
    --var CONN_IDLE_TIMEOUT_SECONDS:0 \
    --var CONN_SWEEP_SECONDS:0 \
    --var AUTH_RELAY_SECRET:test-secret > "$LOG_FILE" 2>&1 &
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
npm test -- "127.0.0.1:$PORT" test-secret
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
