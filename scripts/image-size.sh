#!/usr/bin/env bash
# Compressed size of every platform of an image in an OCI layout (`buildx ... --output type=oci,dest=x.tar`):
# manifest + config + all layers, as a registry stores them. Fails if one platform is >= LIMIT_MB (default 450).
#   scripts/image-size.sh image.tar
set -euo pipefail

tar_file="${1:?usage: image-size.sh <oci-layout.tar>}"
limit_mb="${LIMIT_MB:-450}"
dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
tar -xf "$tar_file" -C "$dir"

blob() { echo "$dir/blobs/${1/://}"; }

# index.json -> either an image index (several platforms) or a single manifest.
failed=0
check() {
  local manifest="$1" label="$2" total
  total=$(jq '[.config.size] + [.layers[].size] | add' "$(blob "$manifest")")
  total=$((total + $(stat -c %s "$(blob "$manifest")" 2>/dev/null || stat -f %z "$(blob "$manifest")")))
  printf '%s: %d bytes (%d MB)\n' "$label" "$total" $((total / 1000000))
  if [ "$total" -ge $((limit_mb * 1000000)) ]; then
    echo "::error::Image $label is ${total} bytes, limit ${limit_mb} MB" >&2
    failed=1
  fi
}

top=$(jq -r '.manifests[0].digest' "$dir/index.json")
if jq -e '.manifests' "$(blob "$top")" > /dev/null 2>&1; then
  while IFS=$'\t' read -r digest platform; do
    check "$digest" "$platform"
  done < <(jq -r '.manifests[] | select(.platform.architecture != "unknown") | [.digest, (.platform.os + "/" + .platform.architecture)] | @tsv' "$(blob "$top")")
else
  check "$top" "single"
fi
exit "$failed"
