#!/usr/bin/env bash
set -euo pipefail

project_root=/workspace
pkgbuild=
pkgrel=
passthrough_vars=()

usage() {
  echo "usage: $0 --project-root PATH --pkgbuild PATH --pkgrel NUMBER [--passthrough-var NAME]..." >&2
}

while (( $# > 0 )); do
  case "$1" in
    --project-root)
      project_root="${2:-}"
      shift 2
      ;;
    --pkgbuild)
      pkgbuild="${2:-}"
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

if [[ -z "${pkgbuild}" || -z "${pkgrel}" ]]; then
  usage
  exit 2
fi

cd -- "${project_root}"
pkgver="$(
  sed -n 's/^[[:space:]]*"version":[[:space:]]*"\([^"]*\)".*/\1/p' \
    "${project_root}/src-tauri/tauri.conf.json"
)"
if [[ ! "${pkgver}" =~ ^[0-9]+([.][0-9]+)*$ ]]; then
  echo "Unsupported Arch pkgver derived from Tauri version: ${pkgver}" >&2
  exit 1
fi

export TAURI_PACKAGING_SOURCE_DIR="${project_root}"
export TAURI_PACKAGING_PKGVER="${pkgver}"
export TAURI_PACKAGING_PKGREL="${pkgrel}"
# shellcheck source=/dev/null
source "${pkgbuild}"

pacman-key --init
pacman-key --populate archlinux
pacman -Syu --needed --noconfirm \
  "${depends[@]}" \
  "${makedepends[@]}" \
  namcap \
  xorg-server-xvfb \
  xorg-xauth

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
artifact_output="${build_home}/artifact-path"
build_environment=(
  HOME="${build_home}"
  TAURI_PACKAGING_SOURCE_DIR="${project_root}"
  TAURI_PACKAGING_PKGVER="${pkgver}"
  TAURI_PACKAGING_PKGREL="${pkgrel}"
)

for variable in "${passthrough_vars[@]}"; do
  if [[ -v "${variable}" ]]; then
    build_environment+=("${variable}=${!variable}")
  fi
done

runuser -u "${build_user}" -- env HOME="${build_home}" \
  /tauri-packaging/aur/version-is-newer.test.sh

runuser -u "${build_user}" -- env HOME="${build_home}" \
  /tauri-packaging/aur/render.test.sh

runuser -u "${build_user}" -- env HOME="${build_home}" \
  /tauri-packaging/arch/metadata.test.sh \
    --project-root "${project_root}" \
    --pkgbuild "${pkgbuild}" \
    --pkgver "${pkgver}" \
    --pkgrel "${pkgrel}"

runuser -u "${build_user}" -- env "${build_environment[@]}" \
  /tauri-packaging/arch/build.sh \
    --project-root "${project_root}" \
    --pkgbuild "${pkgbuild}" \
    --pkgver "${pkgver}" \
    --pkgrel "${pkgrel}" \
    --artifact-output "${artifact_output}"

artifact="$(<"${artifact_output}")"

runuser -u "${build_user}" -- env "${build_environment[@]}" \
  /tauri-packaging/arch/verify.sh \
    --project-root "${project_root}" \
    --pkgbuild "${pkgbuild}" \
    --artifact "${artifact}" \
    --pkgver "${pkgver}" \
    --pkgrel "${pkgrel}"

pacman -U --noconfirm "${artifact}"

runuser -u "${build_user}" -- env "${build_environment[@]}" \
  /tauri-packaging/arch/verify-installed.sh \
    --pkgbuild "${pkgbuild}" \
    --pkgver "${pkgver}" \
    --pkgrel "${pkgrel}"
