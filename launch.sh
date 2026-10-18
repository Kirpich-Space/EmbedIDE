#!/bin/bash
# EmbedIDE — Launch script
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

if [ ! -d "dist" ] || [ "src/" -nt "dist/" ] 2>/dev/null; then
    echo "Building frontend..."
    npx vite build
fi

echo "Starting EmbedIDE..."
exec ./node_modules/.bin/electron .
