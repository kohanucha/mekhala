#!/bin/bash

# Generate a cryptographically secure 32-character hexadecimal string
if command -v openssl >/dev/null 2>&1; then
    SECRET=$(openssl rand -hex 16)
else
    # Fallback for systems without openssl
    SECRET=$(head -c 16 /dev/urandom | xxd -p | tr -d '\n')
fi

echo "===================================================="
echo "            MEKHALA SECRET GENERATOR                "
echo "===================================================="
echo ""
echo "COPY THIS SECRET:"
echo "----------------------------------------------------"
echo "$SECRET"
echo "----------------------------------------------------"
echo ""
echo "HOW TO USE:"
echo "1. Go to your Cloudflare Dashboard."
echo "2. Workers & Pages -> click your 'mekhala'."
echo "3. Settings -> Variables and Secrets."
echo "4. Click 'Add'."
echo "5. Type: Select 'Secret'."
echo "6. Name: Type 'RELAY_SECRET'."
echo "7. Value: Paste the secret from above."
echo "8. Click 'Deploy'."
echo ""
echo "YOUR RELAY URL WILL BE:"
echo "wss://your-domain.com/$SECRET"
echo ""
echo "===================================================="
