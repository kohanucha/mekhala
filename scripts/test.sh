#!/bin/bash
set -e

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
    if [ ! -z "$WRANGLER_PID" ]; then
        kill $WRANGLER_PID 2>/dev/null || true
        sleep 1
        kill -9 $WRANGLER_PID 2>/dev/null || true
    fi
    pkill -9 -f "wrangler dev" 2>/dev/null || true
    pkill -9 -f "workerd" 2>/dev/null || true
    lsof -ti :8788 2>/dev/null | xargs kill -9 2>/dev/null || true
    echo "Cleanup complete."
    echo "Wrangler logs saved to: $LOG_FILE"
}
trap cleanup EXIT

echo "🚀 Starting Mekhala Test Suite"
echo "=============================="

# 1. Type-check TypeScript
echo "Step 1: Type-checking TypeScript..."
npx tsc --noEmit
echo "✅ Type-check passed."

# 2. Check port availability
echo "Checking port $PORT..."
PIDS=$(lsof -ti :$PORT 2>/dev/null || true)
if [ ! -z "$PIDS" ]; then
    BLOCKED_PIDS=""
    for PID in $PIDS; do
        CMD=$(ps -p $PID -o command= 2>/dev/null || true)
        if ! echo "$CMD" | grep -qi "wrangler"; then
            BLOCKED_PIDS="$BLOCKED_PIDS $PID"
        fi
    done
    if [ ! -z "$BLOCKED_PIDS" ]; then
        echo "❌ Port $PORT is in use by non-wrangler process(es):"
        lsof -i :$PORT 2>/dev/null || true
        exit 1
    fi
    echo "Waiting for port $PORT to be released..."
    RETRY=0
    while [ $RETRY -lt 5 ]; do
        if lsof -ti :$PORT 2>/dev/null | grep -q . 2>/dev/null; then
            sleep 1
            RETRY=$((RETRY + 1))
        else
            break
        fi
    done
    PIDS_AFTER=$(lsof -ti :$PORT 2>/dev/null || true)
    if [ ! -z "$PIDS_AFTER" ]; then
        echo "❌ Port $PORT still occupied after cleanup:"
        lsof -i :$PORT 2>/dev/null || true
        exit 1
    fi
    echo "Port $PORT is now free."
fi

# 3. Start Local Relay
echo "Step 2: Starting local relay on port $PORT..."
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
echo "Step 3: Running Node.js integration tests..."
node test/setup-kv.js "127.0.0.1:$PORT"
node test/run-all.js "127.0.0.1:$PORT"
TEST_RESULT=$?

if [ $TEST_RESULT -eq 0 ]; then
    echo ""
    echo "✅ ALL TESTS PASSED!"
else
    echo ""
    echo "❌ TESTS FAILED! Check the output above."
    echo "--- Wrangler Logs ---"
    tail -50 "$LOG_FILE"
    echo "---------------------"
fi

exit $TEST_RESULT
