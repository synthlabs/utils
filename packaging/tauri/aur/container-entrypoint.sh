#!/usr/bin/env bash
set -euo pipefail

project_root=
package_path=
release_tag=
pkgver=
pkgrel=
aur_repository=
git_user_name=
git_user_email=
check_access=()

usage() {
  echo "usage: $0 --project-root PATH --package PATH --release-tag TAG --pkgver VERSION --pkgrel NUMBER --aur-repository URL --git-user-name NAME --git-user-email EMAIL [--check-access]" >&2
}

while (( $# > 0 )); do
  case "$1" in
    --check-access)
      check_access=(--check-access)
      shift
      ;;
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
    --git-user-name)
      git_user_name="${2:-}"
      shift 2
      ;;
    --git-user-email)
      git_user_email="${2:-}"
      shift 2
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [[ -z "${project_root}" || -z "${package_path}" || -z "${release_tag}" || -z "${pkgver}" || -z "${pkgrel}" || -z "${aur_repository}" || -z "${git_user_name}" || -z "${git_user_email}" ]]; then
  usage
  exit 2
fi

pacman-key --init
pacman-key --populate archlinux
pacman -Syu --needed --noconfirm git openssh

build_home=/tmp/tauri-packaging-builder-home
build_user="$(
  bash /tauri-packaging/aur/prepare-home.sh \
    "${HOST_UID:-1000}" "${HOST_GID:-1000}" "${build_home}" /run/aur-ssh
)"

runuser -u "${build_user}" -- env HOME="${build_home}" \
  /tauri-packaging/aur/publish.sh \
    "${check_access[@]}" \
    --project-root "${project_root}" \
    --package "${package_path}" \
    --release-tag "${release_tag}" \
    --pkgver "${pkgver}" \
    --pkgrel "${pkgrel}" \
    --aur-repository "${aur_repository}" \
    --git-user-name "${git_user_name}" \
    --git-user-email "${git_user_email}"
