#!/usr/bin/env bash
set -euo pipefail

if (( $# != 4 )); then
  echo "usage: $0 UID GID HOME SSH_DIRECTORY" >&2
  exit 2
fi

build_uid="$1"
build_gid="$2"
build_home="$3"
ssh_directory="$4"

build_group="$(getent group "${build_gid}" | cut -d: -f1 || true)"
if [[ -z "${build_group}" ]]; then
  build_group="builder-${build_gid}"
  groupadd --gid "${build_gid}" "${build_group}"
fi

build_user="$(getent passwd "${build_uid}" | cut -d: -f1 || true)"
if [[ -z "${build_user}" ]]; then
  build_user="builder-${build_uid}"
  useradd --uid "${build_uid}" --gid "${build_group}" \
    --home-dir "${build_home}" --create-home "${build_user}"
elif [[ "$(getent passwd "${build_uid}" | cut -d: -f6)" != "${build_home}" ]]; then
  usermod --home "${build_home}" "${build_user}"
fi

install -d -m755 -o "${build_user}" -g "${build_group}" "${build_home}"
install -d -m700 -o "${build_user}" -g "${build_group}" "${build_home}/.ssh"
cp -a "${ssh_directory}/." "${build_home}/.ssh/"
chmod 700 "${build_home}/.ssh"
find "${build_home}/.ssh" -type f -exec chmod 600 {} +
chown -R "${build_user}:${build_group}" "${build_home}/.ssh"
printf '%s\n' "${build_user}"
