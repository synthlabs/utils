# Shared Tauri packaging

This directory provides platform-aware build and install targets for Tauri 2 projects that include
`synthlabs/utils`.

Projects set the shared variables and include the Make fragment:

```make
TAURI_PACKAGING_PROJECT_ROOT := $(CURDIR)
TAURI_PACKAGING_PASSTHROUGH_VARS := ENABLE_INTERNAL APP_LOG
include utils/packaging/tauri/tauri.mk
```

The include defines `build`, `build-internal`, `install`, and `install-internal`. An existing local
package can be installed with `TAURI_PACKAGING_INSTALL_ARTIFACT=/path/to/package`.

Consumers may also set `TAURI_PACKAGING_TOKE`, `TAURI_PACKAGING_PNPM`,
`TAURI_PACKAGING_POWERSHELL`, `TAURI_PACKAGING_MAKE_COMMAND`, and
`TAURI_PACKAGING_CONTAINER_RUNTIME` to override tool commands. `TAURI_PACKAGING_PASSTHROUGH_VARS`
is a space-separated allowlist of existing environment variable names that may cross the Arch
container boundary; their values are never accepted as Make-fragment configuration.

Arch projects keep their descriptors under `packaging/arch/` and their AUR template under
`packaging/aur/PKGBUILD.in`. The local PKGBUILD may define:

```bash
_tauri_binary=example
_tauri_expected_files=(
  usr/bin/example
  usr/share/applications/example.desktop
)
_tauri_smoke_enabled=1
_tauri_smoke_timeout=15
_tauri_smoke_env=('WEBKIT_DISABLE_COMPOSITING_MODE=1')
```

The shared Arch helpers set `TAURI_PACKAGING_SOURCE_DIR`, `TAURI_PACKAGING_PKGVER`, and
`TAURI_PACKAGING_PKGREL` while evaluating and building the local PKGBUILD.
