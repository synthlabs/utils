#!/bin/bash
# patch.sh — bump version, regenerate specta bindings, commit.
#
# Assumes the repo has a root-level pnpm workspace (package.json) and Cargo
# workspace (Cargo.toml) so `pnpm exec cargo run --example gen_bindings` works
# from the repo root. Also assumes `vergo`, `sumry`, and `jq` are on PATH.
#
# Overridable via env vars:
#   PATCH_BINDINGS_CMD  Command to regenerate typescript bindings.
#                       Default: pnpm exec cargo run --example gen_bindings --features=gen_bindings
#                       Set to empty string ("") to skip bindings regen.
#   PATCH_FILES         Space-separated paths (relative to repo root) to git add.
#                       Default: package.json Cargo.lock src-tauri/Cargo.toml
#                                src-tauri/tauri.conf.json SUMRY.md archive/

set -e

ROOT=$(git rev-parse --show-toplevel)
cd "${ROOT}"

vergo -project-root "${ROOT}" -debug -update
VERSION=$(jq -r '.version' "${ROOT}/package.json")
sumry -project-root "${ROOT}" -debug -update

# `pnpm exec` puts the project's prettier on PATH so specta's formatter picks it up
BINDINGS_CMD="${PATCH_BINDINGS_CMD-pnpm exec cargo run --example gen_bindings --features=gen_bindings}"
if [[ -n "${BINDINGS_CMD}" ]]; then
    eval "${BINDINGS_CMD}"
fi

FILES="${PATCH_FILES:-package.json Cargo.lock src-tauri/Cargo.toml src-tauri/tauri.conf.json SUMRY.md archive/}"
# shellcheck disable=SC2086
git add ${FILES}

git commit -m "chore(updater): version bump ${VERSION}"
