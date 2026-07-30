#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/tauri-packaging-install-test.XXXXXXXX")"
project_root="${test_root}/repo with spaces"
fake_bin="${test_root}/bin"
make_log="${test_root}/make.log"
sudo_log="${test_root}/sudo.log"
build_log="${test_root}/build.log"
runtime_log="${test_root}/runtime.log"

cleanup() {
  rm -rf -- "${test_root}"
}
trap cleanup EXIT

fail() {
  echo "test failure: $*" >&2
  exit 1
}

assert_eq() {
  local expected="$1"
  local actual="$2"
  local message="$3"
  [[ "${actual}" == "${expected}" ]] || fail "${message}: expected ${expected}, got ${actual}"
}

assert_contains() {
  local file="$1"
  local expected="$2"
  grep -Fq -- "${expected}" "${file}" || fail "${file} does not contain ${expected}"
}

assert_not_contains() {
  local file="$1"
  local unexpected="$2"
  if grep -Fq -- "${unexpected}" "${file}"; then
    fail "${file} unexpectedly contains ${unexpected}"
  fi
}

assert_fails() {
  local message="$1"
  shift
  if "$@"; then
    fail "${message}"
  fi
}

mkdir -p \
  "${project_root}/src-tauri" \
  "${project_root}/packaging/arch" \
  "${fake_bin}"
touch "${project_root}/src-tauri/Cargo.toml"
cat >"${project_root}/packaging/arch/PKGBUILD" <<'EOF'
pkgname=sample
pkgver=${TAURI_PACKAGING_PKGVER:-1.0.0}
pkgrel=${TAURI_PACKAGING_PKGREL:-1}
arch=('x86_64')
depends=('glibc')
makedepends=('nodejs')
EOF

ubuntu_release="${test_root}/ubuntu-release"
debian_release="${test_root}/debian-release"
cachy_release="${test_root}/cachy-release"
arch_release="${test_root}/arch-release"
fedora_release="${test_root}/fedora-release"
printf 'ID=ubuntu\nID_LIKE=debian\n' >"${ubuntu_release}"
printf 'ID=debian\n' >"${debian_release}"
printf 'ID=cachyos\nID_LIKE=arch\n' >"${cachy_release}"
printf 'ID=arch\n' >"${arch_release}"
printf 'ID=fedora\nID_LIKE="rhel fedora"\n' >"${fedora_release}"

cat >"${fake_bin}/fake-cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '{"target_directory":"%s"}\n' "${TAURI_PACKAGING_TEST_TARGET_DIR}"
EOF

cat >"${fake_bin}/fake-make" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"${TAURI_PACKAGING_TEST_MAKE_LOG}"
printf '<ENABLE_INTERNAL=%s>\n' "${ENABLE_INTERNAL:-}" >>"${TAURI_PACKAGING_TEST_MAKE_LOG}"
printf '<APP_LOG=%s>\n' "${APP_LOG:-}" >>"${TAURI_PACKAGING_TEST_MAKE_LOG}"
case "${TAURI_PACKAGING_TEST_MAKE_MODE:-one}" in
  one)
    mkdir -p -- "${TAURI_PACKAGING_TEST_ARTIFACT_DIR}"
    touch -- "${TAURI_PACKAGING_TEST_ARTIFACT_DIR}/${TAURI_PACKAGING_TEST_ARTIFACT_NAME}"
    ;;
  multiple)
    mkdir -p -- "${TAURI_PACKAGING_TEST_ARTIFACT_DIR}"
    touch -- \
      "${TAURI_PACKAGING_TEST_ARTIFACT_DIR}/Sample_one_amd64.deb" \
      "${TAURI_PACKAGING_TEST_ARTIFACT_DIR}/Sample_two_amd64.deb"
    ;;
  missing) ;;
  fail)
    exit 31
    ;;
  *)
    exit 97
    ;;
esac
EOF

cat >"${fake_bin}/sudo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '<%s>\n' "$@" >>"${TAURI_PACKAGING_TEST_SUDO_LOG}"
exit "${TAURI_PACKAGING_TEST_SUDO_EXIT_CODE:-0}"
EOF

cat >"${fake_bin}/toke" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '<%s>\n' "$@" >>"${TAURI_PACKAGING_TEST_BUILD_LOG}"
printf '<ENABLE_INTERNAL=%s>\n' "${ENABLE_INTERNAL:-}" >>"${TAURI_PACKAGING_TEST_BUILD_LOG}"
printf '<APP_LOG=%s>\n' "${APP_LOG:-}" >>"${TAURI_PACKAGING_TEST_BUILD_LOG}"
EOF

cat >"${fake_bin}/container-runtime" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '<%s>\n' "$@" >>"${TAURI_PACKAGING_TEST_RUNTIME_LOG}"
EOF

for command in apt-get pacman; do
  cat >"${fake_bin}/${command}" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
done
chmod +x "${fake_bin}"/*

# shellcheck source=platform.sh
source "${script_dir}/platform.sh"
assert_eq debian "$(TAURI_PACKAGING_OS_RELEASE_FILE="${ubuntu_release}" tauri_packaging_detect_platform)" \
  'Ubuntu family detection'
assert_eq debian "$(TAURI_PACKAGING_OS_RELEASE_FILE="${debian_release}" tauri_packaging_detect_platform)" \
  'Debian family detection'
assert_eq arch "$(TAURI_PACKAGING_OS_RELEASE_FILE="${cachy_release}" tauri_packaging_detect_platform)" \
  'CachyOS family detection'
assert_eq arch "$(TAURI_PACKAGING_OS_RELEASE_FILE="${arch_release}" tauri_packaging_detect_platform)" \
  'Arch family detection'
assert_eq linux "$(TAURI_PACKAGING_OS_RELEASE_FILE="${fedora_release}" tauri_packaging_detect_platform)" \
  'unsupported Linux family detection'

export PATH="${fake_bin}:${PATH}"
export TAURI_PACKAGING_TEST_MAKE_LOG="${make_log}"
export TAURI_PACKAGING_TEST_SUDO_LOG="${sudo_log}"
export TAURI_PACKAGING_TEST_BUILD_LOG="${build_log}"
export TAURI_PACKAGING_TEST_RUNTIME_LOG="${runtime_log}"
export TAURI_PACKAGING_TEST_TARGET_DIR="${project_root}/target"
export TAURI_PACKAGING_TEST_ARTIFACT_DIR="${TAURI_PACKAGING_TEST_TARGET_DIR}/release/bundle/deb"
export TAURI_PACKAGING_TEST_ARTIFACT_NAME=Sample_1.0.0_amd64.deb
export TAURI_PACKAGING_TEST_MAKE_MODE=one

TAURI_PACKAGING_OS_RELEASE_FILE="${ubuntu_release}" \
  "${script_dir}/install.sh" \
    --project-root "${project_root}" \
    --build-target build \
    --make-command "${fake_bin}/fake-make" \
    --cargo "${fake_bin}/fake-cargo"
assert_contains "${make_log}" "--no-print-directory -C ${project_root} build"
assert_contains "${sudo_log}" '<apt-get>'
assert_contains "${sudo_log}" '<install>'
assert_contains "${sudo_log}" '<--reinstall>'
assert_contains "${sudo_log}" "<${TAURI_PACKAGING_TEST_ARTIFACT_DIR}/Sample_1.0.0_amd64.deb>"

rm -rf -- "${TAURI_PACKAGING_TEST_TARGET_DIR}"
rm -f -- "${make_log}" "${sudo_log}"
TAURI_PACKAGING_TEST_TARGET_DIR="${project_root}/src-tauri/target" \
TAURI_PACKAGING_TEST_ARTIFACT_DIR="${project_root}/src-tauri/target/release/bundle/deb" \
TAURI_PACKAGING_OS_RELEASE_FILE="${ubuntu_release}" \
  "${script_dir}/install.sh" \
    --project-root "${project_root}" \
    --build-target build-internal \
    --make-command "${fake_bin}/fake-make" \
    --cargo "${fake_bin}/fake-cargo"
assert_contains "${make_log}" "--no-print-directory -C ${project_root} build-internal"
assert_contains "${sudo_log}" "<${project_root}/src-tauri/target/release/bundle/deb/Sample_1.0.0_amd64.deb>"

override_dir="${test_root}/override packages"
override_deb="${override_dir}/Sample local.deb"
mkdir -p -- "${override_dir}"
touch -- "${override_deb}"
rm -f -- "${make_log}" "${sudo_log}"
TAURI_PACKAGING_OS_RELEASE_FILE="${ubuntu_release}" \
  "${script_dir}/install.sh" \
    --project-root "${project_root}" \
    --build-target build \
    --artifact "${override_deb}" \
    --make-command "${fake_bin}/fake-make"
[[ ! -e "${make_log}" ]] || fail 'artifact override unexpectedly ran the build'
assert_contains "${sudo_log}" "<${override_deb}>"

rm -f -- "${sudo_log}"
(
  cd -- "${override_dir}"
  TAURI_PACKAGING_OS_RELEASE_FILE="${ubuntu_release}" \
    "${script_dir}/install.sh" \
      --project-root "${project_root}" \
      --build-target build \
      --artifact 'Sample local.deb' \
      --make-command "${fake_bin}/fake-make"
)
assert_contains "${sudo_log}" "<${override_deb}>"

bad_override="${override_dir}/Sample.exe"
touch -- "${bad_override}"
assert_fails 'Debian install accepted a non-DEB override' \
  env TAURI_PACKAGING_OS_RELEASE_FILE="${ubuntu_release}" \
  "${script_dir}/install.sh" \
    --project-root "${project_root}" \
    --build-target build \
    --artifact "${bad_override}"

rm -rf -- "${project_root}/target"
export TAURI_PACKAGING_TEST_TARGET_DIR="${project_root}/target"
export TAURI_PACKAGING_TEST_ARTIFACT_DIR="${project_root}/target/release/bundle/deb"
export TAURI_PACKAGING_TEST_MAKE_MODE=missing
assert_fails 'install accepted a missing fresh artifact' \
  env TAURI_PACKAGING_OS_RELEASE_FILE="${ubuntu_release}" \
  "${script_dir}/install.sh" \
    --project-root "${project_root}" \
    --build-target build \
    --make-command "${fake_bin}/fake-make" \
    --cargo "${fake_bin}/fake-cargo"

export TAURI_PACKAGING_TEST_MAKE_MODE=multiple
assert_fails 'install accepted multiple fresh artifacts' \
  env TAURI_PACKAGING_OS_RELEASE_FILE="${ubuntu_release}" \
  "${script_dir}/install.sh" \
    --project-root "${project_root}" \
    --build-target build \
    --make-command "${fake_bin}/fake-make" \
    --cargo "${fake_bin}/fake-cargo"

export TAURI_PACKAGING_TEST_MAKE_MODE=fail
assert_fails 'install ignored a build failure' \
  env TAURI_PACKAGING_OS_RELEASE_FILE="${ubuntu_release}" \
  "${script_dir}/install.sh" \
    --project-root "${project_root}" \
    --build-target build \
    --make-command "${fake_bin}/fake-make" \
    --cargo "${fake_bin}/fake-cargo"

export TAURI_PACKAGING_TEST_MAKE_MODE=one
export TAURI_PACKAGING_TEST_ARTIFACT_DIR="${project_root}/package"
export TAURI_PACKAGING_TEST_ARTIFACT_NAME=sample-1.0.0-1-x86_64.pkg.tar.zst
rm -rf -- "${project_root}/package"
rm -f -- "${make_log}" "${sudo_log}"
TAURI_PACKAGING_OS_RELEASE_FILE="${arch_release}" TAURI_PACKAGING_MACHINE=x86_64 \
  "${script_dir}/install.sh" \
    --project-root "${project_root}" \
    --build-target build \
    --make-command "${fake_bin}/fake-make" \
    --cargo "${fake_bin}/fake-cargo"
assert_contains "${sudo_log}" '<pacman>'
assert_contains "${sudo_log}" "<${project_root}/package/sample-1.0.0-1-x86_64.pkg.tar.zst>"

assert_fails 'Arch install accepted a non-x86_64 host' \
  env TAURI_PACKAGING_OS_RELEASE_FILE="${arch_release}" TAURI_PACKAGING_MACHINE=aarch64 \
  "${script_dir}/install.sh" \
    --project-root "${project_root}" \
    --build-target build \
    --artifact "${project_root}/package/sample-1.0.0-1-x86_64.pkg.tar.zst"
assert_fails 'unsupported Linux distribution accepted make install' \
  env TAURI_PACKAGING_OS_RELEASE_FILE="${fedora_release}" \
  "${script_dir}/install.sh" \
    --project-root "${project_root}" \
    --build-target build \
    --artifact "${override_deb}"

rm -f -- "${sudo_log}"
export TAURI_PACKAGING_TEST_SUDO_EXIT_CODE=23
assert_fails 'installer exit failure was not propagated' \
  env TAURI_PACKAGING_OS_RELEASE_FILE="${ubuntu_release}" \
  "${script_dir}/install.sh" \
    --project-root "${project_root}" \
    --build-target build \
    --artifact "${override_deb}"
unset TAURI_PACKAGING_TEST_SUDO_EXIT_CODE

rm -f -- "${build_log}"
TAURI_PACKAGING_OS_RELEASE_FILE="${ubuntu_release}" \
  "${script_dir}/build.sh" \
    --project-root "${project_root}" \
    --toke "${fake_bin}/toke" \
    --pnpm fake-pnpm
assert_contains "${build_log}" '<--bundles>'
assert_contains "${build_log}" '<deb>'

rm -f -- "${build_log}"
TAURI_PACKAGING_OS_RELEASE_FILE="${fedora_release}" \
  "${script_dir}/build.sh" \
    --project-root "${project_root}" \
    --toke "${fake_bin}/toke" \
    --pnpm fake-pnpm
assert_not_contains "${build_log}" '<--bundles>'

rm -f -- "${runtime_log}"
ENABLE_INTERNAL=1 APP_LOG=debug SECRET_VALUE=hidden \
TAURI_PACKAGING_OS_RELEASE_FILE="${cachy_release}" \
TAURI_PACKAGING_MACHINE=x86_64 \
  "${script_dir}/build.sh" \
    --project-root "${project_root}" \
    --container-runtime "${fake_bin}/container-runtime" \
    --passthrough-var ENABLE_INTERNAL \
    --passthrough-var APP_LOG
assert_contains "${runtime_log}" '<ENABLE_INTERNAL=1>'
assert_contains "${runtime_log}" '<APP_LOG=debug>'
assert_not_contains "${runtime_log}" 'SECRET_VALUE'

makefile="${project_root}/Makefile"
cat >"${makefile}" <<EOF
TAURI_PACKAGING_PROJECT_ROOT := \$(CURDIR)
TAURI_PACKAGING_TOKE := ${fake_bin}/toke
TAURI_PACKAGING_PNPM := fake-pnpm
TAURI_PACKAGING_MAKE_COMMAND := ${fake_bin}/fake-make
TAURI_PACKAGING_PASSTHROUGH_VARS := ENABLE_INTERNAL APP_LOG

build-internal: export ENABLE_INTERNAL := 1
build-internal: export APP_LOG := debug

include ${script_dir}/tauri.mk
EOF

rm -f -- "${build_log}" "${make_log}"
TAURI_PACKAGING_OS_RELEASE_FILE="${ubuntu_release}" \
  make --no-print-directory -n -C "${project_root}" build install
[[ ! -e "${build_log}" ]] || fail 'make dry-run executed the build helper'
[[ ! -e "${make_log}" ]] || fail 'make dry-run executed the install helper'

TAURI_PACKAGING_OS_RELEASE_FILE="${ubuntu_release}" \
  make --no-print-directory -C "${project_root}" build-internal
assert_contains "${build_log}" '<ENABLE_INTERNAL=1>'
assert_contains "${build_log}" '<APP_LOG=debug>'
assert_contains "${build_log}" '<deb>'

echo 'install shell tests passed'
