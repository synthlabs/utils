#!/usr/bin/env bash
set -euo pipefail

template=
output_dir=
pkgver=
pkgrel=
release_tag=
asset_name=
sha256=

usage() {
  echo "usage: $0 --template PATH --output-dir PATH --pkgver VERSION --pkgrel NUMBER --release-tag TAG --asset-name NAME --sha256 HEX" >&2
}

while (( $# > 0 )); do
  case "$1" in
    --template)
      template="${2:-}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:-}"
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
    --release-tag)
      release_tag="${2:-}"
      shift 2
      ;;
    --asset-name)
      asset_name="${2:-}"
      shift 2
      ;;
    --sha256)
      sha256="${2:-}"
      shift 2
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [[ -z "${template}" || -z "${output_dir}" || -z "${pkgver}" || -z "${pkgrel}" || -z "${release_tag}" || -z "${asset_name}" || -z "${sha256}" ]]; then
  usage
  exit 2
fi
if [[ ! -f "${template}" ]]; then
  echo "AUR PKGBUILD template not found: ${template}" >&2
  exit 1
fi
if [[ ! "${pkgver}" =~ ^[0-9]+([.][0-9]+)*$ ]]; then
  echo "Invalid pkgver: ${pkgver}" >&2
  exit 1
fi
if [[ ! "${pkgrel}" =~ ^[1-9][0-9]*$ ]]; then
  echo "Invalid pkgrel: ${pkgrel}" >&2
  exit 1
fi
if [[ ! "${release_tag}" =~ ^[A-Za-z0-9][A-Za-z0-9._+-]*$ ]]; then
  echo "Invalid release tag: ${release_tag}" >&2
  exit 1
fi
if [[ "$(basename -- "${asset_name}")" != "${asset_name}" || ! "${asset_name}" =~ ^[A-Za-z0-9][A-Za-z0-9._+-]*$ ]]; then
  echo "Invalid package asset name: ${asset_name}" >&2
  exit 1
fi
if [[ ! "${sha256}" =~ ^[0-9a-fA-F]{64}$ ]]; then
  echo 'SHA-256 checksum must contain exactly 64 hexadecimal characters' >&2
  exit 1
fi

mkdir -p -- "${output_dir}"
sed \
  -e "s|@PKGVER@|${pkgver}|g" \
  -e "s|@PKGREL@|${pkgrel}|g" \
  -e "s|@RELEASE_TAG@|${release_tag}|g" \
  -e "s|@ASSET_NAME@|${asset_name}|g" \
  -e "s|@SHA256@|${sha256,,}|g" \
  "${template}" >"${output_dir}/PKGBUILD"
