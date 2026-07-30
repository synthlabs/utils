#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
test_dir="$(mktemp -d "${TMPDIR:-/tmp}/tauri-packaging-aur-render.XXXXXXXX")"
template="${test_dir}/PKGBUILD.in"

cleanup() {
  rm -rf -- "${test_dir}"
}
trap cleanup EXIT

cat >"${template}" <<'EOF'
pkgname=sample-bin
pkgver=@PKGVER@
pkgrel=@PKGREL@
pkgdesc='Shared packaging fixture'
arch=('x86_64')
url='https://github.com/example/sample'
license=('MIT')
depends=('glibc')
provides=("sample=${pkgver}")
conflicts=('sample')
source=("@ASSET_NAME@::https://github.com/example/sample/releases/download/@RELEASE_TAG@/@ASSET_NAME@")
noextract=('@ASSET_NAME@')
sha256sums=('@SHA256@')

package() {
  bsdtar -xf "${srcdir}/@ASSET_NAME@" -C "${pkgdir}"
}
EOF

"${script_dir}/render.sh" \
  --template "${template}" \
  --output-dir "${test_dir}/output" \
  --pkgver 1.2.3 \
  --pkgrel 17 \
  --release-tag app-1.2.3-abcdef0 \
  --asset-name sample-1.2.3-17-x86_64.pkg.tar.zst \
  --sha256 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef

(
  cd -- "${test_dir}/output"
  BUILDDIR="${test_dir}" \
    LOGDEST="${test_dir}" \
    PKGDEST="${test_dir}" \
    SRCDEST="${test_dir}" \
    SRCPKGDEST="${test_dir}" \
    makepkg --printsrcinfo >.SRCINFO
)

grep -Fqx $'\tpkgver = 1.2.3' "${test_dir}/output/.SRCINFO"
grep -Fqx $'\tpkgrel = 17' "${test_dir}/output/.SRCINFO"
grep -Fqx $'\tprovides = sample=1.2.3' "${test_dir}/output/.SRCINFO"
grep -Fqx $'\tconflicts = sample' "${test_dir}/output/.SRCINFO"
grep -Fqx \
  $'\tsource = sample-1.2.3-17-x86_64.pkg.tar.zst::https://github.com/example/sample/releases/download/app-1.2.3-abcdef0/sample-1.2.3-17-x86_64.pkg.tar.zst' \
  "${test_dir}/output/.SRCINFO"

if "${script_dir}/render.sh" \
  --template "${template}" \
  --output-dir "${test_dir}/invalid" \
  --pkgver 1.2.3 \
  --pkgrel 17 \
  --release-tag app-1.2.3-abcdef0 \
  --asset-name ../sample.pkg.tar.zst \
  --sha256 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef; then
  echo 'render accepted an unsafe asset name' >&2
  exit 1
fi
