#!/usr/bin/env bash
# docs-render-diagrams.sh: Render DOT files in the docs/ directory to PNG images.
# Place this script in the utils/ subdirectory of your repository.
#
# Usage: Run this script from the repository's main directory.
#
# Example: ./utils/docs-render-benchmark-diagram.sh

set -euo pipefail

# Define the directory containing the dot files and output location.

# Ensure docs directory exists
DOCS_DIR="docs"
if [ ! -d "$DOCS_DIR" ]; then
    echo "Make sure to run this script in the repository's main directory, where ${DOCS_DIR} is located."
    exit 1
fi

echo "Rendering DOT files in ${DOCS_DIR}..."

for dotfile in "${DOCS_DIR}"/*.dot; do
    filename=$(basename "${dotfile}" .dot)
    output="${DOCS_DIR}/${filename}.png"
    echo "Rendering ${dotfile} to ${output}"
    dot -Tpng "${dotfile}" -o "${output}"
done

echo "Rendering complete."
