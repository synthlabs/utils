#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
shared_root="$(cd -- "${script_dir}/.." && pwd)"
project_root=
container_runtime=
pkgrel="${GITHUB_RUN_NUMBER:-1}"
passthrough_vars=()

usage() {
  echo "usage: $0 --project-root PATH [--container-runtime COMMAND] [--pkgrel NUMBER] [--passthrough-var NAME]..." >&2
}

while (( $# > 0 )); do
  case "$1" in
    --project-root)
      project_root="${2:-}"
      shift 2
      ;;
    --container-runtime)
      container_runtime="${2:-}"
      shift 2
      ;;
    --pkgrel)
      pkgrel="${2:-}"
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
pkgbuild="${project_root}/packaging/arch/PKGBUILD"
if [[ ! -f "${pkgbuild}" ]]; then
  echo "Arch PKGBUILD not found: ${pkgbuild}" >&2
  exit 1
fi
if [[ ! "${pkgrel}" =~ ^[1-9][0-9]*$ ]]; then
  echo "Arch pkgrel must be a positive integer: ${pkgrel}" >&2
  exit 1
fi

if [[ -z "${container_runtime}" ]]; then
  if command -v docker >/dev/null 2>&1; then
    container_runtime=docker
  elif command -v podman >/dev/null 2>&1; then
    container_runtime=podman
  else
    echo 'docker or podman is required' >&2
    exit 1
  fi
fi

mkdir -p -- "${project_root}/package"

container_args=()
if [[ "$(basename -- "${container_runtime}")" == podman ]]; then
  container_args+=(--userns=keep-id --user 0)
fi

environment_args=(
  --env "HOST_UID=$(id -u)"
  --env "HOST_GID=$(id -g)"
  --env "TAURI_PACKAGING_PKGREL=${pkgrel}"
)
entrypoint_args=(
  --project-root /workspace
  --pkgbuild /workspace/packaging/arch/PKGBUILD
  --pkgrel "${pkgrel}"
)

for variable in "${passthrough_vars[@]}"; do
  if [[ ! "${variable}" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]]; then
    echo "invalid passthrough variable name: ${variable}" >&2
    exit 2
  fi
  entrypoint_args+=(--passthrough-var "${variable}")
  if [[ -v "${variable}" ]]; then
    environment_args+=(--env "${variable}=${!variable}")
  fi
done

"${container_runtime}" run --rm "${container_args[@]}" "${environment_args[@]}" \
  --volume "${project_root}:/workspace" \
  --volume "${shared_root}:/tauri-packaging:ro" \
  --workdir /workspace \
  archlinux:base-devel \
  bash /tauri-packaging/arch/container-entrypoint.sh "${entrypoint_args[@]}"
