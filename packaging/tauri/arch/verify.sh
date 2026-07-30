#!/usr/bin/env bash
set -euo pipefail

project_root=
pkgbuild=
artifact=
pkgver=
pkgrel=

usage() {
  echo "usage: $0 --project-root PATH --pkgbuild PATH --artifact PATH --pkgver VERSION --pkgrel NUMBER" >&2
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
    --artifact)
      artifact="${2:-}"
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

if [[ -z "${project_root}" || -z "${pkgbuild}" || -z "${artifact}" || -z "${pkgver}" || -z "${pkgrel}" ]]; then
  usage
  exit 2
fi

extract_dir="$(mktemp -d "${TMPDIR:-/tmp}/tauri-packaging-package.XXXXXXXX")"
cleanup() {
  rm -rf -- "${extract_dir}"
}
trap cleanup EXIT

export TAURI_PACKAGING_SOURCE_DIR="${project_root}"
export TAURI_PACKAGING_PKGVER="${pkgver}"
export TAURI_PACKAGING_PKGREL="${pkgrel}"
# shellcheck source=/dev/null
source "${pkgbuild}"

binary_name="${_tauri_binary:-${pkgname}}"
expected_files=("usr/bin/${binary_name}")
if declare -p _tauri_expected_files >/dev/null 2>&1; then
  expected_files=("${_tauri_expected_files[@]}")
fi

assert_line() {
  local text="$1"
  local expected="$2"

  if ! grep -Fqx -- "${expected}" <<<"${text}"; then
    echo "Missing package metadata or file: ${expected}" >&2
    exit 1
  fi
}

namcap_output="$(namcap "${pkgbuild}" "${artifact}")"
printf '%s\n' "${namcap_output}"
if grep -Eq ' E: ' <<<"${namcap_output}"; then
  echo 'namcap reported an error' >&2
  exit 1
fi

pkginfo="$(bsdtar -xOf "${artifact}" .PKGINFO)"
assert_line "${pkginfo}" "pkgname = ${pkgname}"
assert_line "${pkginfo}" "pkgver = ${pkgver}-${pkgrel}"
assert_line "${pkginfo}" 'arch = x86_64'
for dependency in "${depends[@]}"; do
  assert_line "${pkginfo}" "depend = ${dependency}"
done

archive_files="$(bsdtar -tf "${artifact}")"
for installed_file in "${expected_files[@]}"; do
  assert_line "${archive_files}" "${installed_file}"
done

bsdtar -xf "${artifact}" -C "${extract_dir}"
binary="${extract_dir}/usr/bin/${binary_name}"

if ldd "${binary}" | grep -F 'not found'; then
  echo 'The packaged binary has unresolved shared libraries' >&2
  exit 1
fi

if strings "${binary}" | grep -Eq '__TAURI_BUNDLE_TYPE_VAR_(DEB|RPM|APPIMAGE)'; then
  echo 'The packaged binary is marked as a different Linux bundle type' >&2
  exit 1
fi
