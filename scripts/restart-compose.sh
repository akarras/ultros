#!/bin/bash
set -e

TARGET_DIR=~/ultros

if [ ! -d "$TARGET_DIR" ]; then
    echo "Directory $TARGET_DIR does not exist."
    exit 1
fi

cd "$TARGET_DIR"

echo "Restarting services in $TARGET_DIR..."
docker compose down
docker compose up -d

echo "Services restarted."
