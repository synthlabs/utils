# Synth Tauri runtime

Dependency-free startup support shared by Synth desktop apps. Scrybe is the first
consumer; Pepo still uses its local implementation.

Add a path dependency from an app's `src-tauri/Cargo.toml`:

```toml
[dependencies.synth-tauri-runtime]
path = "../utils/rust/synth-tauri-runtime"
```

Call the helper first during single-threaded startup, before initializing Tauri,
GTK/EGL, logging plugins, or any libraries that may start threads:

```rust
// SAFETY: Called at single-threaded startup, before other initialization.
let applied = unsafe { synth_tauri_runtime::apply_nvidia_wayland_workaround() };
```

Keep the returned flag and log once during Tauri setup if it is true. The crate
does not initialize a logger or depend on Tauri.

On Linux, the helper sets `__NV_DISABLE_EXPLICIT_SYNC=1` only when
`WAYLAND_DISPLAY` is present, `/proc/driver/nvidia/version` exists, and the explicit
sync variable is absent. Detection matches Pepo's existing workaround and applies
across distributions. Presence includes empty and non-Unicode values; every
existing explicit-sync override is preserved. A missing NVIDIA driver path skips
the workaround. Other platforms return false without accessing the environment.

The return value is true only when that call sets the variable. Calling again
preserves the newly set value and returns false. Applications can explicitly opt
out with `__NV_DISABLE_EXPLICIT_SYNC=0`; this may restore the original launch issue
on affected systems.

The unsafe API exposes the startup-order requirement: changing the process
environment concurrently with other threads reading it is unsafe on Linux. See
[Rust's environment safety requirements](https://doc.rust-lang.org/std/env/fn.set_var.html#safety).

From a consuming workspace, run `cargo test --locked -p synth-tauri-runtime`.
Predicate tests do not mutate the test runner's environment. Validate the actual
application separately on an NVIDIA Wayland desktop with the override unset.
