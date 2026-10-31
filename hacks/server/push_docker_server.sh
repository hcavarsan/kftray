#!/bin/bash

if [ -z "$1" ]; then
  echo "Usage: $0 <tag>"
  exit 1
fi

TAG=$1

CURRENT_DIR=$(pwd)
KFTRAY_SERVER_DIR="$CURRENT_DIR/crates/kftray-server"


cd "$KFTRAY_SERVER_DIR" || exit

# Do not publish an existing tag if the build fails.
docker buildx build --platform linux/amd64 \
  --file "$KFTRAY_SERVER_DIR/Dockerfile" \
  -t ghcr.io/hcavarsan/kftray-server:"$TAG" --load "$CURRENT_DIR" || exit


cd "$CURRENT_DIR" || exit


docker push ghcr.io/hcavarsan/kftray-server:"$TAG"
