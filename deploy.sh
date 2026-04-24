#!/bin/bash
set -e

# 1. Attempt to get the version
# Try to get an exact tag match for the current commit
GIT_VERSION=$(git describe --exact-match --tags 2>/dev/null || true)

# If no tag exists, use branch-commit format
if [ -z "$GIT_VERSION" ]; then
    BRANCH=$(git branch --show-current)
    COMMIT=$(git rev-parse --short HEAD)
    GIT_VERSION="${BRANCH}-${COMMIT}"
fi

# Append -dirty if there are uncommitted changes
if ! git diff --quiet; then
    GIT_VERSION="${GIT_VERSION}-dirty"
fi

echo "🚀 Deploying nwc-edge-relay version: $GIT_VERSION"

# 2. Deploy and inject the dynamic version
npx wrangler deploy --var RELAY_VERSION:"$GIT_VERSION"
