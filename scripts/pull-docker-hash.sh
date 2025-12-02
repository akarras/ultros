#!/bin/bash
set -e

if [ -z "$1" ]; then
    echo "Usage: $0 <git-hash>"
    exit 1
fi

HASH=$1
IMAGE_NAME="ghcr.io/akarras/ffxiv-playground"
TAG="sha-${HASH}"

echo "Pulling ${IMAGE_NAME}:${TAG}..."
docker pull "${IMAGE_NAME}:${TAG}"

echo "Retagging as ultros:latest..."
docker tag "${IMAGE_NAME}:${TAG}" ultros:latest

echo "Done."
