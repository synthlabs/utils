#!/usr/bin/env bash
set -euo pipefail

project_root=
pkgbuild=
pkgver=
pkgrel=
artifact_output=

usage() {
  echo "usage: $0 --project-root PATH --pkgbuild PATH --pkgver VERSION --pkgrel NUMBER --artifact-output PATH" >&2
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
    --pkgver)
      pkgver="${2:-}"
      shift 2
      ;;
    --pkgrel)
      pkgrel="${2:-}"
      shift 2
      ;;
    --artifact-output)
      artifact_output="${2:-}"
      shift 2
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [[ -z "${project_root}" || -z "${pkgbuild}" || -z "${pkgver}" || -z "${pkgrel}" || -z "${artifact_output}" ]]; then
  usage
  exit 2
fi
project_root="$(cd -- "${project_root}" && pwd)"
pkgbuild="$(realpath -e -- "${pkgbuild}")"
package_dir="${PKGDEST:-${project_root}/package}"
build_root="$(mktemp -d "${TMPDIR:-/tmp}/tauri-packaging-makepkg.XXXXXXXX")"

cleanup() {
  rm -rf -- "${build_root}"
}
trap cleanup EXIT

if [[ ! "${pkgver}" =~ ^[0-9]+([.][0-9]+)*$ ]]; then
  echo "Unsupported Arch pkgver derived from Tauri version: ${pkgver}" >&2
  exit 1
fi
if [[ ! "${pkgrel}" =~ ^[1-9][0-9]*$ ]]; then
  echo "Arch pkgrel must be a positive integer: ${pkgrel}" >&2
  exit 1
fi

mkdir -p -- \
  "${package_dir}" \
  "${build_root}/logs" \
  "${build_root}/sources" \
  "${build_root}/work"
install -m644 "${pkgbuild}" "${build_root}/work/PKGBUILD"

makepkg_environment=(
  TAURI_PACKAGING_SOURCE_DIR="${project_root}"
  TAURI_PACKAGING_PKGVER="${pkgver}"
  TAURI_PACKAGING_PKGREL="${pkgrel}"
  PKGDEST="${package_dir}"
  BUILDDIR="${build_root}"
  LOGDEST="${build_root}/logs"
  SRCDEST="${build_root}/sources"
  SRCPKGDEST="${package_dir}"
)

(
  cd -- "${build_root}/work"
  env "${makepkg_environment[@]}" \
    makepkg --cleanbuild --force --noconfirm --config /etc/makepkg.conf
)

mapfile -t artifacts < <(
  cd -- "${build_root}/work"
  env "${makepkg_environment[@]}" makepkg --packagelist
)
if (( ${#artifacts[@]} != 1 )); then
  echo "expected one Arch package, found ${#artifacts[@]}" >&2
  printf '  %s\n' "${artifacts[@]}" >&2
  exit 1
fi

artifact="${artifacts[0]}"
test -f "${artifact}"
printf '%s\n' "${artifact}" >"${artifact_output}"
printf '%s\n' "${artifact}"
