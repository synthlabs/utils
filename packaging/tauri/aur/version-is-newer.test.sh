#!/usr/bin/env bash
# Shared Tauri AUR version comparison tests.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

"${script_dir}/version-is-newer.sh" 0.2.26-102 0.2.26-101
"${script_dir}/version-is-newer.sh" 0.2.27-1 0.2.26-999

if "${script_dir}/version-is-newer.sh" 0.2.26-101 0.2.26-101; then
  echo 'Equal versions must not be publishable' >&2
  exit 1
fi

if "${script_dir}/version-is-newer.sh" 0.2.26-100 0.2.26-101; then
  echo 'Older versions must not be publishable' >&2
  exit 1
fi
