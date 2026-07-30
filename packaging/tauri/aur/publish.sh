#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
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

project_root="$(cd -- "${project_root}" && pwd)"
package_path="$(realpath -e -- "${package_path}")"
template="${project_root}/packaging/aur/PKGBUILD.in"
aur_dir="$(mktemp -d "${TMPDIR:-/tmp}/tauri-packaging-aur.XXXXXXXX")"
push_output="$(mktemp "${TMPDIR:-/tmp}/tauri-packaging-aur-push.XXXXXXXX.log")"

cleanup() {
  rm -rf -- "${aur_dir}"
  rm -f -- "${push_output}"
}
trap cleanup EXIT

asset_name="$(basename -- "${package_path}")"
sha256="$(sha256sum "${package_path}" | cut -d' ' -f1)"
candidate_version="${pkgver}-${pkgrel}"
aur_package=

current_version() {
  local srcinfo_path="$1"
  local current_pkgver
  local current_pkgrel

  [[ -f "${srcinfo_path}" ]] || return 1
  current_pkgver="$(awk '$1 == "pkgver" { print $3; exit }' "${srcinfo_path}")"
  current_pkgrel="$(awk '$1 == "pkgrel" { print $3; exit }' "${srcinfo_path}")"
  [[ -n "${current_pkgver}" && -n "${current_pkgrel}" ]] || return 1
  printf '%s-%s\n' "${current_pkgver}" "${current_pkgrel}"
}

candidate_is_newer() {
  local remote_version

  if ! remote_version="$(current_version "${aur_dir}/.SRCINFO")"; then
    return 0
  fi

  "${script_dir}/version-is-newer.sh" "${candidate_version}" "${remote_version}"
}

render_and_commit() {
  "${script_dir}/render.sh" \
    --template "${template}" \
    --output-dir "${aur_dir}" \
    --pkgver "${pkgver}" \
    --pkgrel "${pkgrel}" \
    --release-tag "${release_tag}" \
    --asset-name "${asset_name}" \
    --sha256 "${sha256}"

  (
    cd -- "${aur_dir}"
    makepkg --printsrcinfo >.SRCINFO
    aur_package="$(awk '$1 == "pkgbase" { print $3; exit }' .SRCINFO)"
    if [[ -z "${aur_package}" ]]; then
      echo 'Rendered AUR metadata has no pkgbase' >&2
      exit 1
    fi
    makepkg --nodeps --cleanbuild --force --noconfirm
    git add PKGBUILD .SRCINFO
    git commit -m "${aur_package} ${candidate_version}"
  )
}

git clone "${aur_repository}" "${aur_dir}"

if ! candidate_is_newer; then
  echo "AUR already has ${candidate_version} or a newer version; nothing to publish"
  exit 0
fi

git -C "${aur_dir}" config user.name "${git_user_name}"
git -C "${aur_dir}" config user.email "${git_user_email}"
render_and_commit

if git -C "${aur_dir}" push origin HEAD:master >"${push_output}" 2>&1; then
  cat "${push_output}"
  exit 0
fi

cat "${push_output}" >&2
if ! grep -Eq 'non-fast-forward|fetch first|rejected' "${push_output}"; then
  exit 1
fi

git -C "${aur_dir}" fetch origin master
if ! git -C "${aur_dir}" rebase origin/master; then
  git -C "${aur_dir}" rebase --abort
  git -C "${aur_dir}" reset --hard origin/master
fi

git -C "${aur_dir}" show origin/master:.SRCINFO >"${aur_dir}/.SRCINFO.remote"
mv -- "${aur_dir}/.SRCINFO.remote" "${aur_dir}/.SRCINFO"

if ! candidate_is_newer; then
  echo "AUR advanced to ${candidate_version} or a newer version; nothing to publish"
  exit 0
fi

if ! git -C "${aur_dir}" diff --quiet origin/master -- PKGBUILD .SRCINFO; then
  git -C "${aur_dir}" reset --hard origin/master
  render_and_commit
fi

git -C "${aur_dir}" push origin HEAD:master
