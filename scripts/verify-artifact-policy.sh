#!/usr/bin/env bash
# Verify tracked paths referenced by frozen audit docs (readiness + invalidated RQM).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
FAIL=0

check_file() {
  local rel="$1"
  local ctx="$2"
  if [[ ! -f "$rel" ]]; then
    echo "MISSING ($ctx): $rel" >&2
    FAIL=1
  fi
}

# docs/release/2026-10-24-release-readiness.md → evidence bundle
READINESS_DIR="docs/release/evidence/readiness-2026-10-24"
for f in pinned-quality-gates.log live-provider-suites.log packaged-runtime.log \
  observations.json query-light.png query-dark.png restart-light.png; do
  check_file "$READINESS_DIR/$f" "release-readiness"
done

# RUN_INVALIDATION §11 — worker retained (invalidated run)
RQM="docs/plans/active/release-qualification-matrix/runs/rqm-20261024T104824Z"
for f in \
  "workers/L0/ENV-01__macos-arm64__none__dark__1280x800__normal__default/attempt-1/result.claimed-pass-invalid.json" \
  "workers/L1/REL-01__macos-arm64__none__dark__1280x800__normal__default/attempt-1/result.claimed-pass-invalid.json" \
  "workers/L2/LIF-01__macos-arm64__sqlite__dark__1280x800__normal__default/attempt-1/result.claimed-pass-invalid.json" \
  "workers/L2/LIF-01__macos-arm64__sqlite__dark__1280x800__normal__default/attempt-1/evidence/launch.png" \
  "workers/L2/LIF-01__macos-arm64__sqlite__dark__1280x800__normal__default/attempt-1/evidence/orca-app-state.json" \
  "coordinator/RUN_INVALIDATION.md"; do
  check_file "$RQM/$f" "RQM invalidated"
done

# Metadata for local-only tarball (must stay in git)
check_file "$RQM/artifacts/macos-arm64/extracted/README-INSTALL.txt" "RQM artifact metadata"
check_file "$RQM/artifacts/macos-arm64/extracted/DB Pro.app/Contents/Info.plist" "RQM artifact metadata"

if [[ "$FAIL" -ne 0 ]]; then
  echo "verify-artifact-policy: FAIL" >&2
  exit 1
fi
echo "OK: verify-artifact-policy"
