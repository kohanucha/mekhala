#!/bin/bash
set -e

# 1. Find existing KV ID by name (Wrangler names it <PROJECT>-<BINDING>)
echo "Checking for existing KV namespace on Cloudflare..."
KV_ID=$(npx wrangler kv namespace list | jq -r '.[] | select(.title == "mekhala-MEKHALA_NWC_KV") | .id' | head -n 1)

# 2. Create if not found
if [ -z "$KV_ID" ] || [ "$KV_ID" == "null" ]; then
  echo "Creating new KV namespace: MEKHALA_NWC_KV"
  # Create outputs a text block with the ID
  CREATE_OUT=$(npx wrangler kv namespace create MEKHALA_NWC_KV)
  KV_ID=$(echo "$CREATE_OUT" | grep 'id =' | sed 's/.*id = "\(.*\)".*/\1/')
fi

if [ -z "$KV_ID" ] || [ "$KV_ID" == "null" ]; then
  echo "Error: Could not obtain KV ID from Cloudflare."
  exit 1
fi

echo "Using KV_ID: $KV_ID"

# 3. Inject into wrangler.toml
# This pattern finds the binding line and replaces the 'id' on the very next line.
if [[ "$OSTYPE" == "darwin"* ]]; then
  # macOS (BSD sed)
  sed -i '' -e '/binding = "MEKHALA_NWC_KV"/{n;s/id = ".*"/id = "'"$KV_ID"'"/;}' wrangler.toml
else
  # Linux (GNU sed)
  sed -i '/binding = "MEKHALA_NWC_KV"/{n;s/id = ".*"/id = "'"$KV_ID"'"/}' wrangler.toml
fi

echo "wrangler.toml updated successfully."
