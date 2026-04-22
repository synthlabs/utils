#!/bin/bash
# install_deps.sh — install dev tools used by scripts in this submodule.

set -e

if ! command -v go >/dev/null 2>&1; then
    echo "error: go required — https://go.dev/dl/" >&2
    exit 1
fi

go install github.com/xjerod/sumry/cmd/vergo@latest
go install github.com/xjerod/sumry/cmd/sumry@latest

if ! command -v vergo >/dev/null 2>&1; then
    GO_BIN=$(go env GOBIN)
    [[ -z "${GO_BIN}" ]] && GO_BIN="$(go env GOPATH)/bin"
    echo "note: add ${GO_BIN} to PATH to use these tools" >&2
fi
