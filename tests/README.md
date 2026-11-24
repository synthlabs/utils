# Utils tests

`pnpm test` runs the portable suites sequentially: JavaScript/Svelte unit tests (Vitest + `node:test`),
script and CI contract tests, standalone Rust crate tests, Go tests, and mocked Tauri packaging tests.
A failure stops the sequence.

`pnpm verify` runs TypeScript/Svelte checks (`pnpm check`), ESLint + Prettier checks (`pnpm lint`),
`pnpm test`, Rust formatting and clippy across all crates (`pnpm lint:rust`), and `git diff --check`.
`make test`, `make verify`, and `make test-<suite>` delegate to `pnpm` via `configs/testing.mk`.

## Setup

Portable CI targets Node 26, the pnpm version declared in `package.json` (`pnpm@12.23.0`), Go 1.25+,
and stable Rust (`rustfmt` + `clippy`) with Tauri Linux system packages on Linux hosts.

```sh
pnpm install --frozen-lockfile
```

## Commands

| Suite            | Command                     | Make alias                  | Covers                                                                                                    | Portable?                 |
| ---------------- | --------------------------- | --------------------------- | --------------------------------------------------------------------------------------------------------- | ------------------------- |
| Unit             | `pnpm test:unit`            | `make test-unit`            | Vitest 5 projects (`js`, `translate`, `i18n`) + `node --test "js/**/*.test.mjs"` (inbound/translate/i18n) | Yes                       |
| Scripts          | `pnpm test:scripts`         | `make test-scripts`         | `node --test "scripts/*.test.mjs"` (submodule updater, `testing.mk`, `cargo-each`, CI contract)           | Yes                       |
| Rust             | `pnpm test:rust`            | `make test-rust`            | `node scripts/cargo-each.mjs test --locked` across all 5 `rust/*` crates                                  | Yes                       |
| Go               | `pnpm test:go`              | `make test-go`              | `go test -C go ./...` across all `go/pkg/*` packages                                                      | Yes                       |
| Packaging        | `pnpm test:packaging`       | `make test-packaging`       | `node packaging/tauri/test.mjs` (`install.test.ps1` on Windows; `test.sh` on Linux/macOS)                 | Yes (CI skips macOS)      |
| Packaging AUR    | `pnpm test:packaging:aur`   | `make test-packaging-aur`   | `bash packaging/tauri/aur/container-ssh.test.sh` (Arch Linux Docker SSH fixtures)                         | Explicit (Linux + Docker) |
| Translate matrix | `pnpm test:translate`       | `make test-translate`       | `node scripts/check-translate.mjs` (Tauri-free feature combinations, debug + release)                     | Explicit (CI 3-OS)        |
| Translate Tauri  | `pnpm test:translate:tauri` | `make test-translate-tauri` | `node scripts/check-translate-tauri.mjs` (Tauri plugin, consumer crate, contract drift)                   | Explicit (CI Windows)     |
| Translate UI     | `pnpm test:translate:ui`    | `make test-translate-ui`    | `node scripts/check-translate-ui.mjs` (client, framework-neutral TS, Svelte check, translate Vitest)      | Explicit (CI Windows)     |

Focused reruns accept runner arguments:

```sh
pnpm exec vitest run js/testing/tauri-mock.test.ts
node --test scripts/update-utils-submodules.test.mjs
cargo test --locked --manifest-path rust/inbound/Cargo.toml
go test -C go ./pkg/util/...
```

## Suites

- **`js/**/*.test.ts` & `js/**/*.test.mjs`** — Co-located unit tests across root utilities (`cn`, `log`, `platform`, `updater`), `js/inbound/`, `js/translate/`, `js/i18n/`, and `js/testing/`. Vitest writes JSON reports to `target/test-results/vitest.json`.
- **`scripts/*.test.mjs`** — Node built-in test runner covering `update-utils-submodules.mjs`, `configs/testing.mk`, `scripts/cargo-each.mjs`, and the CI contract (`scripts/ci-contract.test.mjs`).
- **`rust/*/`** — Five standalone Cargo workspaces (`inbound`, `inbound-build`, `synth-tauri-runtime`, `translate`, `translate-build`), each with a committed `Cargo.lock`. Inline `#[cfg(test)] mod tests` by default, sibling `tests.rs` modules where suites are larger.
- **`go/pkg/*`** — Pure-Go packages tested via `go test -C go ./...`.
- **`packaging/tauri/`** — Cross-platform dispatcher `test.mjs` running `install.test.ps1` on Windows and `test.sh` on Linux/macOS; Docker-based AUR SSH tests remain explicit (`test:packaging:aur`).

## Platform limits

| Suite                  | Linux (CI)   | Windows (CI)       | macOS (CI)                       |
| ---------------------- | ------------ | ------------------ | -------------------------------- |
| `test:unit`            | Run          | Run                | Run                              |
| `test:scripts`         | Run          | Run                | Run                              |
| `test:rust`            | Run          | Run                | Run                              |
| `test:go`              | Run          | Run                | Run                              |
| `test:packaging`       | Run (`bash`) | Run (`powershell`) | Skipped (`runner.os != 'macOS'`) |
| `test:packaging:aur`   | Run (Docker) | Skipped            | Skipped                          |
| `test:translate`       | Run          | Run                | Run                              |
| `test:translate:tauri` | Skipped      | Run                | Skipped                          |
| `test:translate:ui`    | Skipped      | Run                | Skipped                          |

## Adding a test

1. **Choose the location**:
   - JS/TS/Svelte module test → co-locate as `<name>.test.ts` (or `<name>.test.mjs` for Node-only `node:test` suites) beside the source under `js/`.
   - Script/Make/CI contract test → `scripts/<name>.test.mjs`.
   - Rust unit test → inline `#[cfg(test)] mod tests` or sibling `tests.rs` inside the crate under `rust/<crate>/`.
   - Go unit test → `<name>_test.go` inside `go/pkg/<pkg>/`.
   - Packaging test → `packaging/tauri/<area>/<name>.test.{sh,ps1}` wired into `packaging/tauri/test.sh` or `test.mjs`.
2. **Naming rules**: `<name>.test.ts` / `<name>.test.mjs` / `<name>.test.sh` / `<name>.test.ps1`; Go uses `<name>_test.go`.
3. **Registering a new suite**: add `test:<suite>` to `package.json` (and chain it in `scripts.test` if portable), add a named CI step in `.github/workflows/tests.yml`, and document it in the Commands table above. `scripts/ci-contract.test.mjs` fails if `package.json`, `Makefile`, and `tests.yml` drift.

## Diagnostics

Operational scripts under `scripts/` (`install_deps.{sh,ps1}`, `patch.{sh,ps1}`, `changelog.sh`, `version.sh`, `secret-mngr.sh`, `secrets.sh`, `tauri-dev-inbound.sh`) are manual tooling — **not** automated test suites and not part of `pnpm test`.
