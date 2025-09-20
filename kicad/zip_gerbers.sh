#!/bin/bash

# Script to zip Gerber files with current date
# Usage: ./zip_gerbers.sh

# Get current date in YYYY-MM-DD format
DATE=$(date +%Y-%m-%d)

# Define paths
GERBER_DIR="Export/gerbers"
ZIP_FILE="Export/gerbers_${DATE}.zip"
LOG_FILE="Export/log.txt"

# Clean up existing files
echo "Cleaning up existing files..."
if [ -f "$ZIP_FILE" ]; then
    echo "Removing existing zip file: $ZIP_FILE"
    rm -f "$ZIP_FILE"
fi

if [ -f "$LOG_FILE" ]; then
    echo "Removing existing log file: $LOG_FILE"
    rm -f "$LOG_FILE"
fi

# Check if Gerber directory exists
if [ ! -d "$GERBER_DIR" ]; then
    echo "Error: Gerber directory '$GERBER_DIR' not found!"
    exit 1
fi

# Check if there are any files in the Gerber directory
if [ -z "$(ls -A $GERBER_DIR)" ]; then
    echo "Warning: No files found in '$GERBER_DIR'"
    exit 0
fi

# Create zip file
echo "Creating $ZIP_FILE from files in $GERBER_DIR..."

# Zip all files in the gerbers directory
if zip -r "$ZIP_FILE" "$GERBER_DIR"/*; then
    echo "Successfully created $ZIP_FILE"
    echo "Contents:"
    unzip -l "$ZIP_FILE" | head -20  # Show first 20 lines to avoid too much output
else
    echo "Error: Failed to create zip file"
    exit 1
fi
