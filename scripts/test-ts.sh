#!/bin/bash

# --- Configuration ---
PORT=8787
LOG_FILE="test/wrangler-ts.log"
WRANGLER_CONFIG="wrangler-ts.toml"
RELAY_SECRET="test-secret"

# --- Pre-flight: kill stale processes ---
pkill -9 -f "wrangler dev" 2>/dev/null || true
pkill -9 -f "workerd" 2>/dev/null || true

# --- Cleanup ---
cleanup() {
    echo ""
    echo "Stopping wrangler dev server..."
    if [ ! -z "$WRANGLER_PID" ]; then
        kill $WRANGLER_PID 2>/dev/null
        sleep 1
        kill -9 $WRANGLER_PID 2>/dev/null
    fi
    pkill -9 -f "wrangler dev" 2>/dev/null || true
    pkill -9 -f "workerd" 2>/dev/null || true
    lsof -ti :8788 2>/dev/null | xargs kill -9 2>/dev/null || true
    echo "Cleanup complete."
    echo "Wrangler logs saved to: $LOG_FILE"
}
trap cleanup EXIT

echo "🚀 Starting Mekhala TS Integration Test Suite"
echo "============================================="

# 1. Check port
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

# 2. Start TS Local Relay
echo "Step 1: Starting TS relay on port $PORT..."
npx wrangler dev --config "$WRANGLER_CONFIG" --port $PORT --ip 127.0.0.1 \
    --var RELAY_SECRET:"$RELAY_SECRET" \
    > "$LOG_FILE" 2>&1 &
WRANGLER_PID=$!

# 3. Wait for relay
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

# 4. Setup KV
echo "Step 2: Setting up KV entries..."
export WRANGLER_CONFIG="$WRANGLER_CONFIG"
npx wrangler kv key put --binding MEKHALA_NWC_KV --local --config "$WRANGLER_CONFIG" "testuser_relay" "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0303030303030303030303030303030303030303030303030303030303030303"
npx wrangler kv key put --binding MEKHALA_NWC_KV --local --config "$WRANGLER_CONFIG" "offline_user" "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0404040404040404040404040404040404040404040404040404040404040404"

# 5. Run Integration Tests
echo "Step 3: Running Node.js integration tests..."
cd test
npm test -- "127.0.0.1:$PORT" "$RELAY_SECRET"
TEST_RESULT=$?
cd ..

if [ $TEST_RESULT -eq 0 ]; then
    echo ""
    echo "✅ ALL TS INTEGRATION TESTS PASSED!"
else
    echo ""
    echo "❌ TESTS FAILED! Check the output above."
    echo "--- Wrangler Logs ---"
    cat "$LOG_FILE"
    echo "---------------------"
fi

exit $TEST_RESULT
