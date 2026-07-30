#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
shared_root="$(cd -- "${script_dir}/.." && pwd)"
project_root=
package_path=
release_tag=
pkgver=
pkgrel=
aur_repository=
ssh_dir=
git_user_name=
git_user_email=
container_runtime=docker

usage() {
  echo "usage: $0 --project-root PATH --package PATH --release-tag TAG --pkgver VERSION --pkgrel NUMBER --aur-repository URL --ssh-dir PATH --git-user-name NAME --git-user-email EMAIL [--container-runtime COMMAND]" >&2
}

while (( $# > 0 )); do
  case "$1" in
    --project-root)
      project_root="${2:-}"
      shift 2
      ;;
    --package)
      package_path="${2:-}"
      shift 2
      ;;
    --release-tag)
      release_tag="${2:-}"
      shift 2
      ;;
    --pkgver)
      pkgver="${2:-}"
      shift 2
      ;;
    --pkgrel)
      pkgrel="${2:-}"
      shift 2
      ;;
    --aur-repository)
      aur_repository="${2:-}"
      shift 2
      ;;
    --ssh-dir)
      ssh_dir="${2:-}"
      shift 2
      ;;
    --git-user-name)
      git_user_name="${2:-}"
      shift 2
      ;;
    --git-user-email)
      git_user_email="${2:-}"
      shift 2
      ;;
    --container-runtime)
      container_runtime="${2:-}"
      shift 2
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [[ -z "${project_root}" || -z "${package_path}" || -z "${release_tag}" || -z "${pkgver}" || -z "${pkgrel}" || -z "${aur_repository}" || -z "${ssh_dir}" || -z "${git_user_name}" || -z "${git_user_email}" ]]; then
  usage
  exit 2
fi

project_root="$(cd -- "${project_root}" && pwd)"
package_path="$(realpath -e -- "${package_path}")"
ssh_dir="$(cd -- "${ssh_dir}" && pwd)"
case "${package_path}" in
  "${project_root}"/*) ;;
  *)
    echo 'Package path must be inside the consuming project checkout' >&2
    exit 1
    ;;
esac
container_package_path="/workspace/${package_path#"${project_root}/"}"

container_args=()
if [[ "$(basename -- "${container_runtime}")" == podman ]]; then
  container_args+=(--userns=keep-id --user 0)
fi

"${container_runtime}" run --rm "${container_args[@]}" \
  --env "HOST_UID=$(id -u)" \
  --env "HOST_GID=$(id -g)" \
  --volume "${project_root}:/workspace" \
  --volume "${shared_root}:/tauri-packaging:ro" \
  --volume "${ssh_dir}:/run/aur-ssh:ro" \
  --workdir /workspace \
  archlinux:base-devel \
  bash /tauri-packaging/aur/container-entrypoint.sh \
    --project-root /workspace \
    --package "${container_package_path}" \
    --release-tag "${release_tag}" \
    --pkgver "${pkgver}" \
    --pkgrel "${pkgrel}" \
    --aur-repository "${aur_repository}" \
    --git-user-name "${git_user_name}" \
    --git-user-email "${git_user_email}"
