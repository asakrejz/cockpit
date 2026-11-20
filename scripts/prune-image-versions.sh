#!/usr/bin/env bash
# Deletes every version of the container package except the ones that belong to the image just pushed:
# the digest of the index (KEEP_DIGESTS, first entry) and the digests of its platform manifests.
# Deleting the platform manifests would break `docker pull` for that architecture, so they stay.
#
#   OWNER=acme PACKAGE=cockpit KEEP_DIGESTS="sha256:index sha256:arm64 sha256:amd64" scripts/prune-image-versions.sh
#   DRY_RUN=1 only lists what would be deleted.
#
# Safety: it aborts without deleting anything if the pushed index is not among the versions or the
# list is empty. Versions are never deleted by tag guess: only by version id of a digest not in KEEP_DIGESTS.
set -euo pipefail

: "${OWNER:?}" "${PACKAGE:?}" "${KEEP_DIGESTS:?}"
owner="$(printf '%s' "$OWNER" | tr '[:upper:]' '[:lower:]')"

kind=$(gh api "users/$owner" --jq '.type')
case "$kind" in
  Organization) base="orgs/$owner/packages/container/$PACKAGE/versions" ;;
  *) base="users/$owner/packages/container/$PACKAGE/versions" ;;
esac

versions=$(gh api --paginate "$base?per_page=100" --jq '.[] | {id, name, tags: .metadata.container.tags}' | jq -s '.')
index_digest="${KEEP_DIGESTS%% *}"
keep_json=$(printf '%s\n' $KEEP_DIGESTS | jq -R . | jq -s .)

if ! jq -e --arg d "$index_digest" 'any(.[]; .name == $d)' <<< "$versions" > /dev/null; then
  echo "The pushed version $index_digest is not in the package list; deleting nothing." >&2
  exit 1
fi

to_delete=$(jq -c --argjson keep "$keep_json" '[.[] | select((.name as $n | $keep | index($n)) == null) | select((.tags // []) | index("latest") == null)]' <<< "$versions")
kept=$(jq -r --argjson keep "$keep_json" '[.[] | select((.name as $n | $keep | index($n)) != null)] | length' <<< "$versions")
echo "Keeping $kept version(s); deleting $(jq length <<< "$to_delete")."

jq -r '.[] | "\(.id)\t\(.name)\t\((.tags // []) | join(","))"' <<< "$to_delete" | while IFS=$'\t' read -r id name tags; do
  if [ "${DRY_RUN:-0}" = 1 ]; then
    echo "would delete $id $name [$tags]"
  else
    echo "deleting $id $name [$tags]"
    gh api -X DELETE "$base/$id" > /dev/null
  fi
done
