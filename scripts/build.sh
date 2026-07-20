#!/bin/bash
set -e
echo "Type-checking TypeScript..."
npx tsc --noEmit
echo "✅ Type-check passed."
