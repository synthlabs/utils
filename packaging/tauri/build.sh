#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root=
toke=toke
pnpm=pnpm
container_runtime=
passthrough_vars=()

usage() {
  echo "usage: $0 --project-root PATH [--toke COMMAND] [--pnpm COMMAND] [--container-runtime COMMAND] [--passthrough-var NAME]..." >&2
}

while (( $# > 0 )); do
  case "$1" in
    --project-root)
      project_root="${2:-}"
      shift 2
      ;;
    --toke)
      toke="${2:-}"
      shift 2
      ;;
    --pnpm)
      pnpm="${2:-}"
      shift 2
      ;;
    --container-runtime)
      container_runtime="${2:-}"
      shift 2
      ;;
    --passthrough-var)
      passthrough_vars+=("${2:-}")
      shift 2
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [[ -z "${project_root}" ]]; then
  usage
  exit 2
fi
project_root="$(cd -- "${project_root}" && pwd)"

# shellcheck source=platform.sh
source "${script_dir}/platform.sh"
platform="$(tauri_packaging_detect_platform)"

cd -- "${project_root}"

case "${platform}" in
  windows)
    exec "${toke}" -v "${pnpm}" tauri build --bundles nsis
    ;;
  debian)
    exec "${toke}" -v "${pnpm}" tauri build --bundles deb
    ;;
  arch)
    tauri_packaging_require_arch_x86_64
    args=(--project-root "${project_root}")
    if [[ -n "${container_runtime}" ]]; then
      args+=(--container-runtime "${container_runtime}")
    fi
    for variable in "${passthrough_vars[@]}"; do
      args+=(--passthrough-var "${variable}")
    done
    exec "${script_dir}/arch/container-build.sh" "${args[@]}"
    ;;
  macos | linux | unsupported)
    exec "${toke}" -v "${pnpm}" tauri build
    ;;
esac
