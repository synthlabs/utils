#!/usr/bin/env bash
# Shared Tauri AUR version comparison helper.
set -euo pipefail

candidate="${1:?usage: version-is-newer.sh CANDIDATE CURRENT}"
current="${2:?current version is required}"

(( $(vercmp "${candidate}" "${current}") > 0 ))
