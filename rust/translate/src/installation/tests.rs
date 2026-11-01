use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use super::*;
use crate::catalog::tests::{snapshot, wire, EN_RU, RU_EN};

fn fixture(
    previous: bool,
) -> (
    tempfile::TempDir,
    ModelStore,
    crate::CatalogSnapshot,
    crate::CatalogSnapshot,
) {
    let directory = tempfile::tempdir().unwrap();
    let store = ModelStore::open(directory.path()).unwrap();
    let original = snapshot(&wire());
    if previous {
        crate::store::tests::install(&store, &original, RU_EN);
    }
    let mut value = wire();
    value["models"][0]["version"] = serde_json::json!("4.0");
    let next = snapshot(&value);
    store.retain_catalog(&next).unwrap();
    (directory, store, original, next)
}

fn stage(store: &ModelStore, catalog: &crate::CatalogSnapshot) {
    let previous = store.installed(RU_EN).ok();
    let model = catalog.model(RU_EN).unwrap();
    let relative = begin(store, model, previous.as_ref()).unwrap();
    for file in model.required_files() {
        fs::write(
            store.root().join(&relative).join(&file.filename),
            if file.filename == "weights.bin" {
                b"weights".as_slice()
            } else {
                b"vocabulary"
            },
        )
        .unwrap();
    }
    fs::write(
        store.root().join(relative).join(INSTALL_MANIFEST_FILENAME),
        serde_json::to_vec(&InstallationManifest::for_model(model)).unwrap(),
    )
    .unwrap();
}

#[test]
fn publication_failures_restore_previous_or_absent_state() {
    for previous in [false, true] {
        for phase in ["prepared", "backup", "published"] {
            let (_directory, store, original, next) = fixture(previous);
            let _global = global_lock(&store, RU_EN).unwrap();
            let _guard = store.exclusive(RU_EN).unwrap();
            stage(&store, &next);
            FAIL_PHASE.with(|value| *value.borrow_mut() = Some(phase.into()));
            let result = publish(&store, RU_EN);
            FAIL_PHASE.with(|value| *value.borrow_mut() = None);
            assert!(result.is_err(), "{phase}");
            if previous {
                assert_eq!(
                    store.installed(RU_EN).unwrap().id(),
                    original.model(RU_EN).unwrap().id()
                );
            } else {
                assert!(matches!(
                    store.installed(RU_EN),
                    Err(StoreError::NotInstalled(_))
                ));
            }
            assert!(!store.root().join(area(".install", RU_EN)).exists());
            assert!(!store
                .root()
                .join(area(".install", RU_EN).with_extension("json"))
                .exists());
        }
    }
}

#[test]
fn interrupted_preparation_rolls_back_locally_and_removes_partial_downloads() {
    let (_directory, store, original, next) = fixture(true);
    stage(&store, &next);
    let downloads = downloads(&store, RU_EN).unwrap();
    fs::write(
        store.root().join(&downloads).join(".partial-crashed"),
        b"incomplete",
    )
    .unwrap();
    fs::write(
        store.root().join(&downloads).join("retained.zst"),
        b"complete",
    )
    .unwrap();
    let reopened = ModelStore::open(store.root()).unwrap();
    assert_eq!(reopened.recovery_report().rolled_back, vec![RU_EN]);
    assert_eq!(
        reopened.verify(RU_EN).unwrap().id(),
        original.model(RU_EN).unwrap().id()
    );
    assert!(!store
        .root()
        .join(&downloads)
        .join(".partial-crashed")
        .exists());
    assert!(store.root().join(downloads).join("retained.zst").exists());
}

#[test]
fn committed_cleanup_failure_reports_warning_and_reopens_without_rollback() {
    let (_directory, store, _original, next) = fixture(true);
    stage(&store, &next);
    FAIL_PHASE.with(|value| *value.borrow_mut() = Some("committed".into()));
    let warnings = publish(&store, RU_EN).unwrap();
    FAIL_PHASE.with(|value| *value.borrow_mut() = None);
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        store.installed(RU_EN).unwrap().id(),
        next.model(RU_EN).unwrap().id()
    );
    let reopened = ModelStore::open(store.root()).unwrap();
    assert_eq!(reopened.recovery_report().cleaned_up, vec![RU_EN]);
    assert_eq!(
        reopened.verify(RU_EN).unwrap().id(),
        next.model(RU_EN).unwrap().id()
    );
}

#[test]
fn recovery_skips_live_writer_and_rejects_unknown_journals() {
    let (_directory, store, _original, next) = fixture(true);
    stage(&store, &next);
    let global = global_lock(&store, RU_EN).unwrap();
    let reopened = ModelStore::open(store.root()).unwrap();
    assert!(reopened.recovery_report().installer_busy);
    assert!(store.root().join(area(".install", RU_EN)).exists());
    drop(global);
    fs::write(
        store
            .root()
            .join(area(".install", RU_EN).with_extension("json")),
        b"invalid",
    )
    .unwrap();
    assert!(matches!(
        ModelStore::open(store.root()),
        Err(StoreError::RecoveryRequired { .. })
    ));
    assert!(matches!(
        store.remove(RU_EN),
        Err(StoreError::RecoveryRequired { .. })
    ));
    assert!(store.root().join("models/rus-eng/install.json").exists());
    assert!(store.root().join(area(".install", RU_EN)).exists());
}

#[test]
fn external_journal_survives_partial_transaction_directory_cleanup() {
    let (_directory, store, _original, next) = fixture(true);
    stage(&store, &next);
    FAIL_PHASE.with(|value| *value.borrow_mut() = Some("committed".into()));
    publish(&store, RU_EN).unwrap();
    FAIL_PHASE.with(|value| *value.borrow_mut() = None);
    // Reproduce interruption after directory cleanup but before journal removal.
    fs::remove_dir_all(store.root().join(area(".install", RU_EN))).unwrap();
    let reopened = ModelStore::open(store.root()).unwrap();
    assert_eq!(reopened.recovery_report().cleaned_up, vec![RU_EN]);
    assert_eq!(
        reopened.verify(RU_EN).unwrap().id(),
        next.model(RU_EN).unwrap().id()
    );
}

#[test]
fn crash_child() {
    let Some(root) = std::env::var_os("SYNTH_TRANSLATE_CRASH_ROOT") else {
        return;
    };
    let store = ModelStore::open(root).unwrap();
    let _global = global_lock(&store, RU_EN).unwrap();
    let _guard = store.exclusive(RU_EN).unwrap();
    let mut value = wire();
    value["models"][0]["version"] = serde_json::json!("4.0");
    let next = snapshot(&value);
    store.retain_catalog(&next).unwrap();
    stage(&store, &next);
    publish(&store, RU_EN).unwrap();
    panic!("child did not reach crash checkpoint");
}

#[test]
fn process_exit_at_each_publication_boundary_recovers_old_or_committed_revision() {
    for previous in [false, true] {
        for phase in ["prepared", "backup", "published", "committed"] {
            let (_directory, store, original, next) = fixture(previous);
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "installation::tests::crash_child", "--nocapture"])
                .env("SYNTH_TRANSLATE_CRASH_ROOT", store.root())
                .env("SYNTH_TRANSLATE_CRASH_PHASE", phase)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(77),
                "{phase}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let reopened = ModelStore::open(store.root()).unwrap();
            if phase == "committed" {
                assert_eq!(
                    reopened.verify(RU_EN).unwrap().id(),
                    next.model(RU_EN).unwrap().id()
                );
            } else if previous {
                assert_eq!(
                    reopened.verify(RU_EN).unwrap().id(),
                    original.model(RU_EN).unwrap().id()
                );
            } else {
                assert!(matches!(
                    reopened.verify(RU_EN),
                    Err(StoreError::NotInstalled(_))
                ));
            }
            assert!(!store.root().join(area(".install", RU_EN)).exists());
            assert!(!store
                .root()
                .join(area(".install", RU_EN).with_extension("json"))
                .exists());
        }
    }
}

#[test]
fn global_lock_child() {
    let Some(root) = std::env::var_os("SYNTH_TRANSLATE_INSTALL_LOCK_ROOT") else {
        return;
    };
    let store = ModelStore::open(root).unwrap();
    let _lock = global_lock(&store, RU_EN).unwrap();
    fs::write(store.root().join("installer-ready"), b"ready").unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
}

#[test]
fn global_install_lock_excludes_other_direction_across_processes() {
    let (_directory, store, _original, _next) = fixture(false);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "installation::tests::global_lock_child"])
        .env("SYNTH_TRANSLATE_INSTALL_LOCK_ROOT", store.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !store.root().join("installer-ready").exists() {
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("child lock timeout");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(matches!(
        global_lock(&store, EN_RU),
        Err(StoreError::Busy(_))
    ));
    child.stdin.take().unwrap().write_all(b"release\n").unwrap();
    assert!(child.wait().unwrap().success());
    global_lock(&store, EN_RU).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_locked_installation_survives_failed_replacement_then_retries() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_directory, store, original, next) = fixture(true);
    stage(&store, &next);
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(store.root().join("models/rus-eng/weights.bin"))
        .unwrap();
    assert!(publish(&store, RU_EN).is_err());
    drop(handle);
    let reopened = ModelStore::open(store.root()).unwrap();
    assert_eq!(
        reopened.verify(RU_EN).unwrap().id(),
        original.model(RU_EN).unwrap().id()
    );
    stage(&store, &next);
    publish(&store, RU_EN).unwrap();
    assert_eq!(
        reopened.verify(RU_EN).unwrap().id(),
        next.model(RU_EN).unwrap().id()
    );
}

#[cfg(windows)]
#[test]
fn windows_locked_download_cleanup_keeps_commit_and_recovery_evidence() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_directory, store, _original, next) = fixture(true);
    let downloads = downloads(&store, RU_EN).unwrap();
    let object = store.root().join(&downloads).join("retained.zst");
    fs::write(&object, b"retained").unwrap();
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&object)
        .unwrap();
    stage(&store, &next);
    assert_eq!(publish(&store, RU_EN).unwrap().len(), 1);
    assert_eq!(
        store.installed(RU_EN).unwrap().id(),
        next.model(RU_EN).unwrap().id()
    );
    let reopened = ModelStore::open(store.root()).unwrap();
    assert_eq!(reopened.recovery_report().warnings.len(), 1);
    assert!(matches!(
        store.remove(RU_EN),
        Err(StoreError::CleanupPending { .. })
    ));
    assert_eq!(
        store.verify(RU_EN).unwrap().id(),
        next.model(RU_EN).unwrap().id()
    );
    assert!(store
        .root()
        .join(area(".install", RU_EN).with_extension("json"))
        .exists());
    drop(handle);
    let reopened = ModelStore::open(store.root()).unwrap();
    assert_eq!(reopened.recovery_report().cleaned_up, vec![RU_EN]);
    assert!(!object.exists());
    assert!(!store.root().join(downloads.with_extension("json")).exists());
    assert!(store.remove(RU_EN).unwrap());
    ModelStore::open(store.root()).unwrap();
    stage(&store, &next);
    publish(&store, RU_EN).unwrap();
    store.verify(RU_EN).unwrap();
}

#[test]
fn removal_recovers_pending_transactions_before_deleting_the_model() {
    for phase in ["prepared", "backup", "published", "committed"] {
        let (_directory, store, _original, next) = fixture(true);
        stage(&store, &next);
        let relative = area(".install", RU_EN);
        if phase == "committed" {
            FAIL_PHASE.with(|value| *value.borrow_mut() = Some("committed".into()));
            assert_eq!(publish(&store, RU_EN).unwrap().len(), 1);
            FAIL_PHASE.with(|value| *value.borrow_mut() = None);
        } else {
            if phase != "prepared" {
                fs::rename(
                    store.root().join("models/rus-eng"),
                    store.root().join(&relative).join("backup"),
                )
                .unwrap();
            }
            if phase == "published" {
                fs::rename(
                    store.root().join(&relative).join("staged"),
                    store.root().join("models/rus-eng"),
                )
                .unwrap();
            }
        }
        assert!(store.remove(RU_EN).unwrap(), "{phase}");
        assert!(!store.root().join(&relative).exists());
        assert!(!store.root().join(relative.with_extension("json")).exists());
        let reopened = ModelStore::open(store.root()).unwrap();
        assert!(matches!(
            reopened.installed(RU_EN),
            Err(StoreError::NotInstalled(_))
        ));
        stage(&reopened, &next);
        publish(&reopened, RU_EN).unwrap();
        reopened.verify(RU_EN).unwrap();
    }
}

#[test]
fn removal_recovery_is_independent_of_the_other_directions_installer() {
    let (_directory, store, original, next) = fixture(true);
    crate::store::tests::install(&store, &original, EN_RU);
    stage(&store, &next);
    let _global = global_lock(&store, EN_RU).unwrap();
    let other_direction = store.exclusive(EN_RU).unwrap();
    assert!(store.remove(RU_EN).unwrap());
    assert!(!store.root().join(area(".install", RU_EN)).exists());
    drop(other_direction);
    store.verify(EN_RU).unwrap();
}

#[test]
fn post_restore_cleanup_failures_warn_and_defer_removal() {
    for previous in [false, true] {
        let (_directory, store, original, next) = fixture(previous);
        stage(&store, &next);
        FAIL_PHASE.with(|value| *value.borrow_mut() = Some("cleanup".into()));
        let reopened = ModelStore::open(store.root()).unwrap();
        assert_eq!(reopened.recovery_report().rolled_back, vec![RU_EN]);
        assert_eq!(reopened.recovery_report().warnings.len(), 1);
        if previous {
            assert_eq!(
                reopened.verify(RU_EN).unwrap().id(),
                original.model(RU_EN).unwrap().id()
            );
        } else {
            assert!(matches!(
                reopened.installed(RU_EN),
                Err(StoreError::NotInstalled(_))
            ));
        }
        assert!(matches!(
            store.remove(RU_EN),
            Err(StoreError::CleanupPending { .. })
        ));
        assert!(store
            .root()
            .join(area(".install", RU_EN).with_extension("json"))
            .exists());
        FAIL_PHASE.with(|value| *value.borrow_mut() = None);
        assert_eq!(store.remove(RU_EN).unwrap(), previous);
        assert!(!store.root().join(area(".install", RU_EN)).exists());
        ModelStore::open(store.root()).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn locked_staged_files_leave_the_restored_store_usable() {
    use std::os::windows::fs::OpenOptionsExt;
    for previous in [false, true] {
        let (_directory, store, original, next) = fixture(previous);
        stage(&store, &next);
        let path = store
            .root()
            .join(area(".install", RU_EN))
            .join("staged/weights.bin");
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let reopened = ModelStore::open(store.root()).unwrap();
        assert_eq!(reopened.recovery_report().rolled_back, vec![RU_EN]);
        assert!(!reopened.recovery_report().warnings.is_empty());
        if previous {
            assert_eq!(
                reopened.verify(RU_EN).unwrap().id(),
                original.model(RU_EN).unwrap().id()
            );
        } else {
            assert!(matches!(
                reopened.installed(RU_EN),
                Err(StoreError::NotInstalled(_))
            ));
        }
        assert!(matches!(
            store.remove(RU_EN),
            Err(StoreError::CleanupPending { .. })
        ));
        drop(held);
        let reopened = ModelStore::open(store.root()).unwrap();
        assert!(reopened.recovery_report().warnings.is_empty());
        assert!(!store.root().join(area(".install", RU_EN)).exists());
    }
}

#[test]
fn failed_rollback_preserves_evidence_until_the_problem_is_resolved() {
    let (_directory, store, original, next) = fixture(true);
    stage(&store, &next);
    let relative = area(".install", RU_EN);
    fs::rename(
        store.root().join("models/rus-eng"),
        store.root().join(&relative).join("backup"),
    )
    .unwrap();
    fs::rename(
        store.root().join(&relative).join("staged"),
        store.root().join("models/rus-eng"),
    )
    .unwrap();
    let manifest = store.root().join(&relative).join("backup/install.json");
    let bytes = fs::read(&manifest).unwrap();
    fs::write(&manifest, b"invalid").unwrap();
    assert!(matches!(
        ModelStore::open(store.root()),
        Err(StoreError::RecoveryRequired { .. })
    ));
    assert!(store.root().join(relative.with_extension("json")).exists());
    assert!(store.root().join("models/rus-eng/install.json").exists());
    fs::write(manifest, bytes).unwrap();
    let reopened = ModelStore::open(store.root()).unwrap();
    assert_eq!(
        reopened.verify(RU_EN).unwrap().id(),
        original.model(RU_EN).unwrap().id()
    );
}
