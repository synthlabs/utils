#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root=
build_target=
artifact_override=
make_command=make
cargo_command=cargo
node_command=node
marker=

usage() {
  echo "usage: $0 --project-root PATH --build-target {build|build-internal} [--artifact PATH] [--make-command COMMAND] [--cargo COMMAND] [--node COMMAND]" >&2
}

while (( $# > 0 )); do
  case "$1" in
    --project-root)
      project_root="${2:-}"
      shift 2
      ;;
    --build-target)
      build_target="${2:-}"
      shift 2
      ;;
    --artifact)
      artifact_override="${2:-}"
      shift 2
      ;;
    --make-command)
      make_command="${2:-}"
      shift 2
      ;;
    --cargo)
      cargo_command="${2:-}"
      shift 2
      ;;
    --node)
      node_command="${2:-}"
      shift 2
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [[ -z "${project_root}" ]]; then
  usage
  exit 2
fi
project_root="$(cd -- "${project_root}" && pwd)"

# shellcheck source=platform.sh
source "${script_dir}/platform.sh"

cleanup() {
  if [[ -n "${marker}" ]]; then
    rm -f -- "${marker}"
  fi
}
trap cleanup EXIT

fail_artifact_count() {
  local expected="$1"
  shift

  echo "expected exactly one fresh ${expected} artifact, found $#" >&2
  if (( $# > 0 )); then
    printf '  %s\n' "$@" >&2
  fi
  exit 1
}

resolve_override() {
  local path="$1"

  if [[ "${path}" != /* ]]; then
    path="${PWD}/${path}"
  fi
  if [[ ! -f "${path}" ]]; then
    echo "install artifact does not exist: ${path}" >&2
    exit 1
  fi
  realpath -e -- "${path}"
}

find_fresh_artifact() {
  local directory="$1"
  local pattern="$2"
  local description="$3"
  local matches=()

  if [[ -d "${directory}" ]]; then
    while IFS= read -r -d '' match; do
      matches+=("${match}")
    done < <(
      find "${directory}" -maxdepth 1 -type f -name "${pattern}" -newer "${marker}" -print0
    )
  fi

  if (( ${#matches[@]} != 1 )); then
    fail_artifact_count "${description}" "${matches[@]}"
  fi
  realpath -e -- "${matches[0]}"
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "required install command not found: $1" >&2
    exit 1
  fi
}

case "${build_target}" in
  build | build-internal) ;;
  *)
    usage
    exit 2
    ;;
esac

platform="$(tauri_packaging_detect_platform)"
case "${platform}" in
  debian) ;;
  arch)
    tauri_packaging_require_arch_x86_64
    ;;
  *)
    echo "make install supports Windows, Debian-family Linux, and x86_64 Arch-family Linux; detected ${platform}" >&2
    exit 2
    ;;
esac

if [[ -n "${artifact_override}" ]]; then
  artifact="$(resolve_override "${artifact_override}")"
else
  cargo_metadata="$(
    "${cargo_command}" metadata \
      --manifest-path "${project_root}/src-tauri/Cargo.toml" \
      --format-version 1 \
      --no-deps
  )"
  target_directory="$(
    printf '%s' "${cargo_metadata}" |
      "${node_command}" -e '
        let input = "";
        process.stdin.setEncoding("utf8");
        process.stdin.on("data", chunk => input += chunk);
        process.stdin.on("end", () => {
          const value = JSON.parse(input).target_directory;
          if (typeof value !== "string" || value.length === 0) process.exit(1);
          process.stdout.write(value);
        });
      '
  )"

  marker="$(mktemp "${TMPDIR:-/tmp}/tauri-packaging-install.XXXXXXXX")"
  "${make_command}" --no-print-directory -C "${project_root}" "${build_target}"

  case "${platform}" in
    debian)
      artifact="$(
        find_fresh_artifact \
          "${target_directory}/release/bundle/deb" \
          '*.deb' \
          'DEB'
      )"
      ;;
    arch)
      artifact="$(
        find_fresh_artifact \
          "${project_root}/package" \
          '*.pkg.tar.zst' \
          'Arch package'
      )"
      ;;
  esac
fi

case "${platform}" in
  debian)
    if [[ "${artifact}" != *.deb ]]; then
      echo "Debian-family installs require a .deb artifact: ${artifact}" >&2
      exit 1
    fi
    require_command sudo
    require_command apt-get
    sudo env DEBIAN_FRONTEND=noninteractive \
      apt-get install --yes --reinstall --allow-downgrades "${artifact}"
    ;;
  arch)
    if [[ "${artifact}" != *.pkg.tar.zst ]]; then
      echo "Arch-family installs require a .pkg.tar.zst artifact: ${artifact}" >&2
      exit 1
    fi
    require_command sudo
    require_command pacman
    sudo pacman -U --noconfirm "${artifact}"
    ;;
esac
