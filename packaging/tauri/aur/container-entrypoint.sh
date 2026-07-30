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

usage() {
  echo "usage: $0 --project-root PATH --package PATH --release-tag TAG --pkgver VERSION --pkgrel NUMBER --aur-repository URL --git-user-name NAME --git-user-email EMAIL" >&2
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

build_gid="${HOST_GID:-1000}"
build_uid="${HOST_UID:-1000}"
build_group="$(getent group "${build_gid}" | cut -d: -f1 || true)"
if [[ -z "${build_group}" ]]; then
  build_group=builder
  groupadd --gid "${build_gid}" "${build_group}"
fi

build_user="$(getent passwd "${build_uid}" | cut -d: -f1 || true)"
if [[ -z "${build_user}" ]]; then
  build_user=builder
  useradd --uid "${build_uid}" --gid "${build_group}" --create-home "${build_user}"
fi

build_home=/tmp/tauri-packaging-builder-home
install -d -m755 -o "${build_user}" -g "${build_group}" "${build_home}"
install -d -m700 -o "${build_user}" -g "${build_group}" "${build_home}/.ssh"
cp -a /run/aur-ssh/. "${build_home}/.ssh/"
chown -R "${build_user}:${build_group}" "${build_home}/.ssh"

runuser -u "${build_user}" -- env HOME="${build_home}" \
  /tauri-packaging/aur/publish.sh \
    --project-root "${project_root}" \
    --package "${package_path}" \
    --release-tag "${release_tag}" \
    --pkgver "${pkgver}" \
    --pkgrel "${pkgrel}" \
    --aur-repository "${aur_repository}" \
    --git-user-name "${git_user_name}" \
    --git-user-email "${git_user_email}"
