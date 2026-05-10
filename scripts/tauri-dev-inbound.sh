#!/usr/bin/env bash
# Run `pnpm tauri dev` with inbound webhook secrets injected.
#
# Usage:
#   ./utils/scripts/tauri-dev-inbound.sh
#   ./utils/scripts/tauri-dev-inbound.sh -- --features foo

set -euo pipefail

usage() {
    cat <<'EOF'
tauri-dev-inbound - run pnpm tauri dev with inbound webhook secrets

usage:
  ./utils/scripts/tauri-dev-inbound.sh [--] [tauri dev args...]

The script must be run from a supported app checkout (pepo or scrybe).
It loads .env.secrets through utils/scripts/secrets.sh, then verifies that the
app-specific inbound webhook variable was injected before starting Tauri.
EOF
}

die() {
    printf 'error: %s\n' "$1" >&2
    exit "${2:-1}"
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

if [[ "${1:-}" == "--" ]]; then
    shift
fi

ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || die "run this inside a git checkout"
cd "$ROOT"

[[ -f package.json ]] || die "package.json not found at $ROOT"
[[ -f .env.secrets ]] || die ".env.secrets not found at $ROOT"

SECRETS_SCRIPT="$ROOT/utils/scripts/secrets.sh"
[[ -x "$SECRETS_SCRIPT" || -f "$SECRETS_SCRIPT" ]] || die "missing utils/scripts/secrets.sh"

command -v node >/dev/null 2>&1 || die "node is required to read package.json"
command -v pnpm >/dev/null 2>&1 || die "pnpm is required to run Tauri dev"
command -v secrets-init >/dev/null 2>&1 || die "secrets-init is required by utils/scripts/secrets.sh"

APP_NAME=$(node -p "require('./package.json').name" 2>/dev/null) || die "failed to read package name from package.json"

case "$APP_NAME" in
    pepo)
        REQUIRED_WEBHOOK_VAR="INBOUND_WEBHOOK_PEPO"
        REQUIRED_WEBHOOK_SECRET="gcp:secretmanager:projects/synth-labs/secrets/inbound_webhook_pepo"
        ;;
    scrybe)
        REQUIRED_WEBHOOK_VAR="INBOUND_WEBHOOK_SCRYBE"
        REQUIRED_WEBHOOK_SECRET="gcp:secretmanager:projects/synth-labs/secrets/inbound_webhook_scrybe"
        ;;
    *)
        die "unsupported app '$APP_NAME'; expected pepo or scrybe"
        ;;
esac

env "$REQUIRED_WEBHOOK_VAR=$REQUIRED_WEBHOOK_SECRET" \
    REQUIRED_INBOUND_WEBHOOK_VAR="$REQUIRED_WEBHOOK_VAR" \
    bash "$SECRETS_SCRIPT" bash -lc '
set -euo pipefail

required="${REQUIRED_INBOUND_WEBHOOK_VAR:?missing required webhook variable name}"
if [[ -z "${!required:-}" ]]; then
    printf "error: %s was not injected by secrets-init\n" "$required" >&2
    exit 1
fi

exec pnpm tauri dev "$@"
' tauri-dev-inbound "$@"
