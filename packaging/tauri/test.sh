#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

bash -n \
  "${script_dir}/build.sh" \
  "${script_dir}/install.sh" \
  "${script_dir}/platform.sh" \
  "${script_dir}/arch/build.sh" \
  "${script_dir}/arch/container-build.sh" \
  "${script_dir}/arch/container-entrypoint.sh" \
  "${script_dir}/arch/metadata.test.sh" \
  "${script_dir}/arch/verify.sh" \
  "${script_dir}/arch/verify-installed.sh" \
  "${script_dir}/aur/container-entrypoint.sh" \
  "${script_dir}/aur/container-publish.sh" \
  "${script_dir}/aur/publish.sh" \
  "${script_dir}/aur/render.sh" \
  "${script_dir}/aur/render.test.sh" \
  "${script_dir}/aur/version-is-newer.sh" \
  "${script_dir}/aur/version-is-newer.test.sh"

"${script_dir}/install.test.sh"

if command -v makepkg >/dev/null 2>&1; then
  "${script_dir}/aur/render.test.sh"
fi
if command -v vercmp >/dev/null 2>&1; then
  "${script_dir}/aur/version-is-newer.test.sh"
fi

if grep -R -E -n 'PEPO_|Pepo|pepo' "${script_dir}" \
  --exclude='README.md' \
  --exclude='test.sh'; then
  echo 'shared packaging contains a Pepo-specific identifier' >&2
  exit 1
fi

echo 'shared Tauri packaging tests passed'
