#!/usr/bin/env bash
# install_deps.sh — verify (default) or install (--ci) system deps for a
# Tauri 2 + pnpm project. Shared across projects via the utils submodule.
# Per-project extras live in <repo-root>/.install_deps.sh (sourced if present).

set -euo pipefail

MODE=verify
if [[ "${1:-}" == "--ci" ]]; then
    MODE=install
elif [[ -n "${1:-}" ]]; then
    echo "usage: $0 [--ci]" >&2
    exit 2
fi

ROOT=$(git rev-parse --show-toplevel)
cd "${ROOT}"

if [[ -t 1 ]] && [[ -z "${NO_COLOR:-}" ]]; then
    C_RED=$'\033[31m'; C_GREEN=$'\033[32m'; C_YELLOW=$'\033[33m'
    C_BOLD=$'\033[1m'; C_DIM=$'\033[2m'; C_RESET=$'\033[0m'
else
    C_RED=''; C_GREEN=''; C_YELLOW=''; C_BOLD=''; C_DIM=''; C_RESET=''
fi

ok()   { printf "  ${C_GREEN}✓${C_RESET} %s\n" "$1"; }
miss() { printf "  ${C_RED}✗${C_RESET} %s ${C_DIM}— %s${C_RESET}\n" "$1" "$2"; }
warn() { printf "  ${C_YELLOW}!${C_RESET} %s ${C_DIM}— %s${C_RESET}\n" "$1" "$2"; }
info() { printf "${C_BOLD}==>${C_RESET} %s\n" "$1"; }

MISSING=()
APT_PKGS=()
APT_TRACKED=()

# check_tool <name> <check-cmd> <install-hint>
# Runs <check-cmd>. In install mode, executes <install-hint> on failure and re-checks.
check_tool() {
    local name="$1" check_cmd="$2" hint="$3"
    if eval "$check_cmd" >/dev/null 2>&1; then
        ok "$name"
        return 0
    fi
    if [[ "$MODE" == "install" ]]; then
        info "installing: $name"
        if eval "$hint" && eval "$check_cmd" >/dev/null 2>&1; then
            ok "$name (installed)"
            return 0
        fi
        miss "$name" "install failed"
    else
        miss "$name" "$hint"
    fi
    MISSING+=("$name")
}

# check_apt <name> <check-cmd> <apt-pkg>
# Linux-only. In install mode queues the package for a single batched apt-get call.
check_apt() {
    local name="$1" check_cmd="$2" pkg="$3"
    if eval "$check_cmd" >/dev/null 2>&1; then
        ok "$name"
        return 0
    fi
    if [[ "$MODE" == "install" ]]; then
        APT_PKGS+=("$pkg")
        APT_TRACKED+=("$name"$'\t'"$check_cmd")
        warn "$name" "queued for apt: $pkg"
    else
        miss "$name" "sudo apt-get install -y $pkg"
        MISSING+=("$name")
    fi
}

apply_apt_batch() {
    [[ ${#APT_PKGS[@]} -eq 0 ]] && return 0
    info "sudo apt-get install -y ${APT_PKGS[*]}"
    sudo apt-get update
    sudo apt-get install -y "${APT_PKGS[@]}"

    local entry name check
    for entry in "${APT_TRACKED[@]}"; do
        name="${entry%%$'\t'*}"
        check="${entry#*$'\t'}"
        if eval "$check" >/dev/null 2>&1; then
            ok "$name (installed)"
        else
            miss "$name" "apt install did not resolve — check package name"
            MISSING+=("$name")
        fi
    done
}

pnpm_pinned_version() {
    [[ -f "${ROOT}/package.json" ]] || return 0
    sed -nE 's/.*"packageManager"[[:space:]]*:[[:space:]]*"pnpm@([0-9.]+)".*/\1/p' "${ROOT}/package.json"
}

init_submodules() {
    [[ -f "${ROOT}/.gitmodules" ]] || return 0
    # Only init uninitialized submodules (status prefixed with `-`). Don't
    # force-reset already-initialized ones — that would silently revert any
    # in-progress pointer bumps the user hasn't committed yet.
    if git -C "${ROOT}" submodule status | grep -q '^-'; then
        info "git submodule update --init --recursive"
        git -C "${ROOT}" submodule update --init --recursive
    fi
}

check_macos() {
    info "macOS dependencies"

    check_tool "Xcode Command Line Tools" \
        "xcode-select -p" \
        "xcode-select --install  # opens a GUI dialog"

    check_tool "Homebrew" \
        "command -v brew" \
        '/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"'

    check_tool "cmake" \
        "command -v cmake" \
        "brew install cmake"

    check_tool "rustup" \
        "command -v rustup" \
        "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
}

check_linux() {
    info "Linux (Debian/Ubuntu) dependencies"

    check_apt "C/C++ toolchain"     "command -v cc && command -v c++"        "build-essential"
    check_apt "cmake"               "command -v cmake"                       "cmake"
    check_apt "pkg-config"          "command -v pkg-config"                  "pkg-config"
    check_apt "webkit2gtk-4.1"      "pkg-config --exists webkit2gtk-4.1"     "libwebkit2gtk-4.1-dev"
    check_apt "appindicator3"       "pkg-config --exists appindicator3-0.1"  "libappindicator3-dev"
    check_apt "librsvg"             "pkg-config --exists librsvg-2.0"        "librsvg2-dev"
    check_apt "patchelf"            "command -v patchelf"                    "patchelf"
    check_apt "xdg-utils"           "command -v xdg-open"                    "xdg-utils"

    check_tool "rustup" \
        "command -v rustup" \
        "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
}

check_shared() {
    info "shared dependencies"

    check_tool "Node.js" \
        "command -v node" \
        "install Node (brew install node / apt install nodejs / nvm)"

    local pinned
    pinned=$(pnpm_pinned_version || true)
    check_tool "pnpm${pinned:+ $pinned}" \
        "command -v pnpm" \
        "corepack enable && corepack prepare pnpm@${pinned:-latest} --activate"
}

check_secrets_tools() {
    info "secrets management tools"

    check_tool "gcloud" \
        "command -v gcloud" \
        "install gcloud SDK — https://cloud.google.com/sdk/docs/install"

    check_tool "gh" \
        "command -v gh" \
        "brew install gh  # or see https://cli.github.com"
}

check_release_tools() {
    info "release scripting tools"

    if ! command -v go >/dev/null 2>&1; then
        warn "vergo/sumry" "install Go (https://go.dev/dl/) to enable release scripts"
        return 0
    fi

    local go_bin
    go_bin=$(go env GOBIN)
    [[ -z "$go_bin" ]] && go_bin="$(go env GOPATH)/bin"

    if [[ ":${PATH}:" != *":${go_bin}:"* ]]; then
        warn "PATH" "${go_bin} not on PATH — add to your shell rc"
        export PATH="${go_bin}:${PATH}"
    fi

    local tool
    for tool in vergo sumry; do
        check_tool "$tool" \
            "command -v $tool" \
            "go install github.com/xjerod/sumry/cmd/${tool}@latest"
    done
}

main() {
    local os
    os=$(uname -s)
    info "mode: $MODE"
    info "os:   $os"
    info "root: $ROOT"

    case "$os" in
        Darwin) check_macos ;;
        Linux)  check_linux ;;
        *)
            echo "unsupported OS: $os — use install_deps.ps1 on Windows" >&2
            exit 2
            ;;
    esac

    check_shared
    check_secrets_tools
    check_release_tools

    if [[ -f "${ROOT}/.install_deps.sh" ]]; then
        info "per-project extras: ${ROOT}/.install_deps.sh"
        # shellcheck source=/dev/null
        source "${ROOT}/.install_deps.sh"
    fi

    [[ "$os" == "Linux" ]] && apply_apt_batch

    init_submodules

    echo
    if [[ ${#MISSING[@]} -eq 0 ]]; then
        printf "${C_GREEN}${C_BOLD}all good.${C_RESET}"
        [[ "$MODE" == "verify" ]] && printf " next: ${C_BOLD}pnpm install && pnpm tauri dev${C_RESET}"
        echo
        exit 0
    fi

    printf "${C_RED}${C_BOLD}%d missing:${C_RESET} %s\n" "${#MISSING[@]}" "${MISSING[*]}"
    [[ "$MODE" == "verify" ]] && printf "re-run with ${C_BOLD}--ci${C_RESET} to install automatically\n"
    exit 1
}

main "$@"
