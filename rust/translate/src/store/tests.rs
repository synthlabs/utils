use std::{
    io::Write,
    process::{Command, Stdio},
};

use super::*;
use crate::{
    catalog::tests::{snapshot, wire, RU_EN},
    InstallationState, TranslationCore,
};

pub(crate) fn install(store: &ModelStore, catalog: &CatalogSnapshot, pair: LanguagePair) {
    store.retain_catalog(catalog).unwrap();
    let guard = store.exclusive(pair).unwrap();
    fs::create_dir_all(guard.directory()).unwrap();
    let model = catalog.model(pair).unwrap();
    fs::write(
        guard.directory().join(INSTALL_MANIFEST_FILENAME),
        serde_json::to_vec(&InstallationManifest::for_model(model)).unwrap(),
    )
    .unwrap();
    for file in model.required_files() {
        let contents: &[u8] = if file.filename == "weights.bin" {
            b"weights"
        } else {
            b"vocabulary"
        };
        fs::write(guard.directory().join(&file.filename), contents).unwrap();
    }
}

fn fixture() -> (tempfile::TempDir, ModelStore, CatalogSnapshot) {
    let directory = tempfile::tempdir().unwrap();
    let store = ModelStore::open(directory.path()).unwrap();
    let catalog = snapshot(&wire());
    install(&store, &catalog, RU_EN);
    (directory, store, catalog)
}

fn state(store: &ModelStore) -> InstallationState {
    store
        .inventory(None)
        .unwrap()
        .into_iter()
        .find(|entry| entry.pair == Some(RU_EN))
        .unwrap()
        .state
}

#[test]
fn inventory_is_quick_acquisition_hashes_and_lexicons_are_optional() {
    let (_directory, store, catalog) = fixture();
    assert_eq!(state(&store), InstallationState::PresentUnverified);
    assert!(store.verify(RU_EN).is_ok());
    let path = store.directory(RU_EN).unwrap().join("weights.bin");
    fs::write(&path, b"WRONG!!").unwrap();
    assert_eq!(state(&store), InstallationState::PresentUnverified);
    assert!(matches!(
        store.acquire(RU_EN),
        Err(StoreError::Verification(
            TranslationError::ChecksumMismatch { .. }
        ))
    ));
    fs::write(&path, b"short").unwrap();
    assert_eq!(state(&store), InstallationState::Invalid);
    fs::remove_file(&path).unwrap();
    assert_eq!(state(&store), InstallationState::Incomplete);
    assert!(store.remove(RU_EN).unwrap());
    assert!(!store.remove(RU_EN).unwrap());
    assert_eq!(state(&store), InstallationState::NotInstalled);
    install(&store, &catalog, RU_EN);
    assert!(store.verify(RU_EN).is_ok());
}

#[test]
fn unknown_and_invalid_manifests_are_reported_and_never_removed() {
    let (_directory, store, _) = fixture();
    fs::create_dir(store.root().join("models/old-model")).unwrap();
    assert!(store
        .inventory(None)
        .unwrap()
        .iter()
        .any(|e| e.pair.is_none() && e.state == InstallationState::Unmanaged));
    let path = store
        .directory(RU_EN)
        .unwrap()
        .join(INSTALL_MANIFEST_FILENAME);
    let valid = fs::read(&path).unwrap();
    for bytes in [
        b"invalid".to_vec(),
        valid
            .iter()
            .copied()
            .map(|b| if b == b'1' { b'9' } else { b })
            .collect(),
    ] {
        fs::write(&path, bytes).unwrap();
        assert_eq!(state(&store), InstallationState::Unmanaged);
        assert!(matches!(store.remove(RU_EN), Err(StoreError::Unmanaged(_))));
        assert!(store.directory(RU_EN).unwrap().exists());
    }
    fs::write(&path, valid).unwrap();
    assert!(store.remove(RU_EN).unwrap());
    assert!(store.root().join("models/old-model").exists());
}

#[test]
fn older_and_unlisted_installations_keep_pinned_metadata() {
    let (_directory, store, original) = fixture();
    let mut value = wire();
    value["models"][0]["version"] = serde_json::json!("4.0");
    let updated = snapshot(&value);
    CatalogCache::open(store.root())
        .unwrap()
        .commit(&updated)
        .unwrap();
    assert!(!store
        .inventory(Some(&updated))
        .unwrap()
        .into_iter()
        .find(|e| e.pair == Some(RU_EN))
        .unwrap()
        .matches_current_catalog
        .unwrap());
    assert_eq!(
        store.acquire(RU_EN).unwrap().model().id(),
        original.model(RU_EN).unwrap().id()
    );
    value["models"].as_array_mut().unwrap().remove(0);
    let unlisted = snapshot(&value);
    CatalogCache::open(store.root())
        .unwrap()
        .commit(&unlisted)
        .unwrap();
    assert!(store.verify(RU_EN).is_ok());
    assert!(store.remove(RU_EN).unwrap());
}

#[test]
fn guards_exclude_writers_and_survive_lazy_registration_and_drop() {
    let (_directory, store, _) = fixture();
    let lease = store.acquire(RU_EN).unwrap();
    let second = store.acquire(RU_EN).unwrap();
    assert!(matches!(store.remove(RU_EN), Err(StoreError::Busy(_))));
    drop(second);
    let mut core = TranslationCore::default();
    let id = lease.model().id().to_owned();
    core.register_stored_model(lease, LoadMode::Lazy).unwrap();
    assert!(matches!(store.exclusive(RU_EN), Err(StoreError::Busy(_))));
    let duplicate = store.acquire(RU_EN).unwrap();
    assert!(core
        .register_stored_model(duplicate, LoadMode::Lazy)
        .is_err());
    assert!(core.unregister_model(&id));
    let exclusive = store.exclusive(RU_EN).unwrap();
    assert!(matches!(store.acquire(RU_EN), Err(StoreError::Busy(_))));
    assert_eq!(state(&store), InstallationState::Busy);
    drop(exclusive);
    core.register_stored_model(store.acquire(RU_EN).unwrap(), LoadMode::Lazy)
        .unwrap();
    drop(core);
    assert!(store.remove(RU_EN).unwrap());
}

#[test]
fn pinned_snapshot_corruption_does_not_make_installation_trusted() {
    let (_directory, store, catalog) = fixture();
    fs::write(
        store
            .root()
            .join("catalog")
            .join(format!("{}.json", catalog.sha256())),
        b"{}",
    )
    .unwrap();
    assert_eq!(state(&store), InstallationState::Unmanaged);
    assert!(store.acquire(RU_EN).is_err());
    assert!(store.remove(RU_EN).is_err());
}

#[cfg(windows)]
#[test]
fn removal_failure_preserves_metadata_and_is_retryable() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_directory, store, _) = fixture();
    let path = store.directory(RU_EN).unwrap().join("weights.bin");
    let open = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .unwrap();
    assert!(store.remove(RU_EN).is_err());
    assert!(store
        .directory(RU_EN)
        .unwrap()
        .join(INSTALL_MANIFEST_FILENAME)
        .exists());
    drop(open);
    assert!(store.remove(RU_EN).unwrap());
}

#[test]
fn cross_process_lock_child() {
    let Some(root) = std::env::var_os("SYNTH_TRANSLATE_LOCK_TEST_ROOT") else {
        return;
    };
    let store = ModelStore::open(root).unwrap();
    let _lease = store.acquire(RU_EN).unwrap();
    fs::write(store.root().join("child-ready"), b"ready").unwrap();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}

#[test]
fn leases_exclude_removal_across_processes_and_release_on_process_exit() {
    let (_directory, store, _) = fixture();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::tests::cross_process_lock_child",
            "--nocapture",
        ])
        .env("SYNTH_TRANSLATE_LOCK_TEST_ROOT", store.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !store.root().join("child-ready").exists() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("lock child exited: {status}");
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("child did not acquire lease");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(matches!(store.remove(RU_EN), Err(StoreError::Busy(_))));
    child.stdin.take().unwrap().write_all(b"release\n").unwrap();
    assert!(child.wait().unwrap().success());
    assert!(store.remove(RU_EN).unwrap());
}

#[test]
fn symlinked_artifacts_and_descendants_never_escape_store() {
    let (_directory, store, _) = fixture();
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("weights.bin");
    fs::write(&outside_file, b"weights").unwrap();
    let artifact = store.directory(RU_EN).unwrap().join("weights.bin");
    fs::remove_file(&artifact).unwrap();
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&outside_file, &artifact);
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(&outside_file, &artifact);
    if let Err(error) = linked {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            return;
        }
        panic!("cannot create test symlink: {error}");
    }
    assert!(store.acquire(RU_EN).is_err());
    assert!(store.remove(RU_EN).is_err());
    assert_eq!(fs::read(&outside_file).unwrap(), b"weights");
}
