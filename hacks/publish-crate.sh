#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <crate-path>" >&2
  exit 2
fi

crate_path="$1"
crate_name=$(basename "$crate_path")
max_retries=3
retry_delay=30

publish_log=$(mktemp)
trap 'rm -f "$publish_log"' EXIT

echo "Publishing $crate_name..."
cd "$crate_path"

for attempt in $(seq 1 "$max_retries"); do
  echo "Attempt $attempt/$max_retries for $crate_name"

  if cargo publish 2>&1 | tee "$publish_log"; then
    echo "Successfully published $crate_name"
    exit 0
  fi

  if grep -qE "already uploaded|already exists" "$publish_log"; then
    echo "$crate_name already published, skipping"
    exit 0
  fi

  if [ "$attempt" -lt "$max_retries" ]; then
    echo "Retrying $crate_name in ${retry_delay}s... (attempt $((attempt + 1))/$max_retries)"
    sleep "$retry_delay"
    retry_delay=$((retry_delay * 2))
  fi
done

echo "Failed to publish $crate_name after $max_retries attempts" >&2
exit 1
