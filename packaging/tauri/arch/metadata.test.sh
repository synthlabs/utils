#!/usr/bin/env bash
set -euo pipefail

project_root=
pkgbuild=
pkgver=
pkgrel=

usage() {
  echo "usage: $0 --project-root PATH --pkgbuild PATH --pkgver VERSION --pkgrel NUMBER" >&2
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
    *)
      usage
      exit 2
      ;;
  esac
done

if [[ -z "${project_root}" || -z "${pkgbuild}" || -z "${pkgver}" || -z "${pkgrel}" ]]; then
  usage
  exit 2
fi

test_dir="$(mktemp -d "${TMPDIR:-/tmp}/tauri-packaging-arch-metadata.XXXXXXXX")"
cleanup() {
  rm -rf -- "${test_dir}"
}
trap cleanup EXIT

export TAURI_PACKAGING_SOURCE_DIR="${project_root}"
export TAURI_PACKAGING_PKGVER="${pkgver}"
export TAURI_PACKAGING_PKGREL="${pkgrel}"
# shellcheck source=/dev/null
source "${pkgbuild}"
install -m644 "${pkgbuild}" "${test_dir}/PKGBUILD"

(
  cd -- "${test_dir}"
  BUILDDIR="${test_dir}" \
    LOGDEST="${test_dir}" \
    PKGDEST="${test_dir}" \
    SRCDEST="${test_dir}" \
    SRCPKGDEST="${test_dir}" \
    makepkg --printsrcinfo >.SRCINFO
)

grep -Fqx 'pkgbase = '"${pkgname}" "${test_dir}/.SRCINFO"
grep -Fqx $'\tpkgver = '"${pkgver}" "${test_dir}/.SRCINFO"
grep -Fqx $'\tpkgrel = '"${pkgrel}" "${test_dir}/.SRCINFO"
grep -Fqx $'\tarch = x86_64' "${test_dir}/.SRCINFO"

for dependency in "${depends[@]}"; do
  grep -Fqx $'\tdepends = '"${dependency}" "${test_dir}/.SRCINFO"
done
for dependency in "${makedepends[@]}"; do
  grep -Fqx $'\tmakedepends = '"${dependency}" "${test_dir}/.SRCINFO"
done
