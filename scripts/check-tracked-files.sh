#!/usr/bin/env bash
# Fails if Git tracks a file that holds credentials or local configuration:
#   * any `.env` file except `.env.example`
#   * anything under `config/` except `config/config.example.toml`
# Used by the CI job `no-env-files`; run it in a repo (or pass the repo folder) to check locally.
set -euo pipefail

cd "${1:-.}"

tracked=$(git ls-files | grep -E '(^|/)\.env($|\.)' | grep -vE '(^|/)\.env\.example$' || true)
config=$(git ls-files -- config | grep -vxE 'config/config\.example\.toml' || true)

if [ -n "$tracked" ]; then
  echo "::error::These files must not be committed (remove with git rm --cached, rotate the keys):"
  echo "$tracked"
fi
if [ -n "$config" ]; then
  echo "::error::These files in config/ must not be committed (remove with git rm --cached, only config/config.example.toml is allowed):"
  echo "$config"
fi
[ -z "$tracked$config" ]
