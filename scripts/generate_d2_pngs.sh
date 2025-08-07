#!/usr/bin/env bash
# Generate PNGs from all D2 diagrams in the architecture folder
set -e

if ! command -v d2 &> /dev/null; then
    echo "Error: d2 CLI tool not found. Please install d2 (https://d2lang.com/tour/install)." >&2
    exit 1
fi

find ./architecture -type f -name "*.d2" | while read -r d2file; do
    pngfile="${d2file%.d2}.png"
    echo "Generating $pngfile from $d2file..."
    d2 "$d2file" "$pngfile"
done
