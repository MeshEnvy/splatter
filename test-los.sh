#!/usr/bin/env bash
# Build splatter and run propagation for one viewshed workspace under ``.cache/viewsheds/<digest>/``.
#
# Usage: DIGEST="<64 hex chars>" ./test-los.sh
#    or: SITE=/abs/path/to/viewshed/dir ./test-los.sh   (workspace contains request.json …)
#
# Obtain ``DIGEST`` from ``peaky`` stdout (``Viewshed cache root``) plus the folder created for your preset,
# or ``ls ~/.cache/viewsheds`` (default cache base when preset has no cache_path).
#
# Progress goes to stderr ([splatter] …). Quiet: VERBOSE=0 ./test-los.sh …
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE="${IMAGE:-splatter:latest}"
MIRROR="${MIRROR:-${ROOT}/../.cache/splat_tiles}"
VERBOSE="${VERBOSE:-1}"

if [[ -n "${SITE:-}" ]]; then
  SITE="$(cd "$(dirname "${SITE}")" && pwd)/$(basename "${SITE}")"
elif [[ -n "${DIGEST:-}" ]]; then
  SITE="${ROOT}/../.cache/viewsheds/${DIGEST}"
else
  printf '%s\n' "Set DIGEST=<viewshed_workspace_digest> or SITE=/path/to/workspace (see script header)." >&2
  exit 1
fi

docker build -t "${IMAGE}" "${ROOT}"

RUN_ARGS=(run --work-dir /work)
[[ "${VERBOSE}" != "0" ]] && RUN_ARGS+=(--verbose)

docker run --rm \
  -v "${SITE}:/work" \
  -v "${MIRROR}:/splat_cache" \
  -e "SPLAT_CACHE=/splat_cache" \
  "${IMAGE}" \
  "${RUN_ARGS[@]}"
