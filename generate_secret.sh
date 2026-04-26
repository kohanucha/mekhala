#!/bin/bash

# Generate a cryptographically secure 32-character hexadecimal string
if command -v openssl >/dev/null 2>&1; then
    SECRET=$(openssl rand -hex 16)
else
    # Fallback for systems without openssl
    SECRET=$(head -c 16 /dev/urandom | xxd -p | tr -d '\n')
fi

echo "===================================================="
echo "          NWC EDGE RELAY SECRET GENERATOR           "
echo "===================================================="
echo ""
echo "COPY THIS SECRET:"
echo "----------------------------------------------------"
echo "$SECRET"
echo "----------------------------------------------------"
echo ""
echo "HOW TO USE:"
echo "1. Go to your Cloudflare Dashboard."
echo "2. Workers & Pages -> your-relay -> Settings -> Variables."
echo "3. Add a Variable named 'RELAY_SECRET' and paste this secret."
echo "4. Your relay URL will be:"
echo "   wss://your-relay-name.your-subdomain.workers.dev/$SECRET"
echo ""
echo "===================================================="
