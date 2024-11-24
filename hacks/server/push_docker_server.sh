#!/bin/bash

if [ -z "$1" ]; then
  echo "Usage: $0 <tag>"
  exit 1
fi

TAG=$1

CURRENT_DIR=$(pwd)
KFTRAY_SERVER_DIR="$CURRENT_DIR/crates/kftray-server"


cd "$KFTRAY_SERVER_DIR" || exit

docker buildx build --platform linux/amd64 -t ghcr.io/hcavarsan/kftray-server:"$TAG" --load .


cd "$CURRENT_DIR" || exit


docker push ghcr.io/hcavarsan/kftray-server:"$TAG"
