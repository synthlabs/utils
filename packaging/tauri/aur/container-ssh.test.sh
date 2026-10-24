#!/usr/bin/env bash
set -euo pipefail

shared_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
"${CONTAINER_RUNTIME:-docker}" run --rm \
  --volume "${shared_root}:/tauri-packaging:ro" \
  archlinux:base-devel \
  bash -c 'pacman -Syu --needed --noconfirm git openssh && bash /tauri-packaging/aur/ssh-home.test.sh'
