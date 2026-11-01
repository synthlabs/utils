use serde_json::{json, Value};

use super::*;

pub(crate) const RU_EN: LanguagePair = LanguagePair {
    source: Language::RUSSIAN,
    target: Language::ENGLISH,
};
pub(crate) const EN_RU: LanguagePair = LanguagePair {
    source: Language::ENGLISH,
    target: Language::RUSSIAN,
};

pub(crate) fn wire() -> Value {
    let artifact = |role: &str, name: &str, contents: &[u8]| {
        let sha = hash(contents).to_string();
        json!({"role": role, "filename": name, "url": format!("{ORIGIN}/objects/sha256/{sha}.zst"), "sha256": sha,
            "size": contents.len(), "decompressed_sha256": sha, "decompressed_size": contents.len()})
    };
    let model = |source, target, version, architecture| {
        json!({
            "source": source, "target": target, "version": version, "architecture": architecture, "filter_expression": "",
            "files": [artifact("model", "weights.bin", b"weights"), artifact("vocab", "vocab.spm", b"vocabulary"), artifact("lex", "lex.bin", b"lexicon")]
        })
    };
    json!({
        "schema_version": 1,
        "source": {"license_url": "https://www.mozilla.org/media/MPL/2.0/index.txt", "license_sha256": hash(b"license").to_string(),
            "records_url": "https://example.test/records", "records_sha256": hash(b"records").to_string(), "retrieved_at": "2026-11-01T00:00:00Z"},
        "models": [model("ru", "en", "3.1", "base-memory"), model("en", "ru", "3.0", "base"), {"source": "fr", "target": "en", "unrelated": true}]
    })
}

pub(crate) fn snapshot(value: &Value) -> CatalogSnapshot {
    let bytes = serde_json::to_vec(value).unwrap();
    CatalogSnapshot::from_bytes(&bytes, &hash(&bytes).to_string()).unwrap()
}

#[test]
fn resolves_only_supported_pairs_and_counts_shared_vocab_once() {
    let catalog = snapshot(&wire());
    assert_eq!(catalog.models().count(), 2);
    let model = catalog.model(RU_EN).unwrap();
    assert_eq!(model.version(), "3.1");
    assert_eq!(model.architecture(), "base-memory");
    assert!(model.id().starts_with("mozilla-rus-eng-"));
    assert_eq!(model.required_files().len(), 2);
    assert_eq!(model.files().len(), 3);
    assert_eq!(model.installed_bytes(), 17);
    assert_eq!(model.download_bytes(), 17);
    assert_eq!(
        model.source_tokenizer().filename,
        model.target_tokenizer().filename
    );
    assert_eq!(catalog.model(EN_RU).unwrap().architecture(), "base");
}

#[test]
fn hashes_exact_bytes_before_parsing_and_checks_pointer_origin() {
    assert!(matches!(
        CatalogSnapshot::from_bytes(b"not json", &hash(b"other").to_string()),
        Err(CatalogError::ChecksumMismatch { .. })
    ));
    let catalog = snapshot(&wire());
    let pointer = serde_json::to_vec(&catalog.pointer()).unwrap();
    assert_eq!(
        CatalogPointer::from_bytes(&pointer).unwrap().sha256(),
        catalog.sha256().to_string()
    );
    let mut pointer: Value = serde_json::from_slice(&pointer).unwrap();
    pointer["catalog_url"] = json!("https://evil.test/catalog.json");
    assert!(CatalogPointer::from_bytes(&serde_json::to_vec(&pointer).unwrap()).is_err());
    pointer["sha256"] = json!("../escape");
    assert!(CatalogPointer::from_bytes(&serde_json::to_vec(&pointer).unwrap()).is_err());
    assert!(CatalogPointer::from_bytes(&vec![b' '; POINTER_LIMIT + 1]).is_err());
    assert!(CatalogSnapshot::from_bytes(
        &vec![b' '; CATALOG_LIMIT + 1],
        &catalog.sha256().to_string()
    )
    .is_err());
}

#[test]
fn rejects_unsupported_schema_filters_ambiguous_entries_and_missing_roles() {
    for mutation in 0..7 {
        let mut value = wire();
        match mutation {
            0 => value["schema_version"] = json!(2),
            1 => value["models"][0]["filter_expression"] = json!("arbitrary javascript"),
            2 => {
                let duplicate = value["models"][0].clone();
                value["models"].as_array_mut().unwrap().push(duplicate);
            }
            3 => {
                value["models"][0]["files"]
                    .as_array_mut()
                    .unwrap()
                    .remove(0);
            }
            4 => value["models"][0]["files"][1]["role"] = json!("unknown"),
            5 => value["models"][0]["files"][0]["url"] = json!("http://models.jerod.me/file"),
            _ => value["models"][0]["files"][0]["decompressed_size"] = json!(0),
        }
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(
            CatalogSnapshot::from_bytes(&bytes, &hash(&bytes).to_string()).is_err(),
            "mutation {mutation}"
        );
    }
    let mut value = wire();
    let mut android = value["models"][0].clone();
    android["filter_expression"] = json!("env.appinfo.OS == 'Android'");
    value["models"].as_array_mut().unwrap().push(android);
    value["models"][1]["filter_expression"] = json!("env.appinfo.OS != 'Android'");
    assert_eq!(snapshot(&value).models().count(), 2);
}

#[test]
fn rejects_unsafe_and_case_colliding_filenames() {
    for filename in [
        "../x",
        "C:\\x",
        "x/y",
        "x:y",
        ".",
        "..",
        "CON",
        "nul.txt",
        "com1.bin",
        "x.",
        "install.json",
        "VOCAB.SPM",
    ] {
        let mut value = wire();
        value["models"][0]["files"][0]["filename"] = json!(filename);
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(
            CatalogSnapshot::from_bytes(&bytes, &hash(&bytes).to_string()).is_err(),
            "{filename}"
        );
    }
}

#[test]
fn separate_tokenizers_work_and_revision_identity_ignores_snapshot_only_changes() {
    let initial = snapshot(&wire());
    let mut value = wire();
    value["source"]["retrieved_at"] = json!("later");
    let later = snapshot(&value);
    assert_ne!(initial.sha256(), later.sha256());
    assert_eq!(
        initial.model(RU_EN).unwrap().id(),
        later.model(RU_EN).unwrap().id()
    );
    let mut target = value["models"][0]["files"][1].clone();
    value["models"][0]["files"][1]["role"] = json!("srcvocab");
    target["role"] = json!("trgvocab");
    target["filename"] = json!("target.spm");
    value["models"][0]["files"]
        .as_array_mut()
        .unwrap()
        .push(target);
    assert_eq!(
        snapshot(&value)
            .model(RU_EN)
            .unwrap()
            .required_files()
            .len(),
        3
    );
}

#[test]
fn cache_publication_preserves_old_snapshot_and_failed_pointer_update() {
    let directory = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(directory.path()).unwrap();
    let first = snapshot(&wire());
    assert!(cache.current().is_err());
    cache.commit(&first).unwrap();
    let mut value = wire();
    value["models"][0]["version"] = json!("4.0");
    let second = snapshot(&value);
    cache.retain(&second).unwrap();
    assert_eq!(cache.current().unwrap().sha256(), first.sha256());
    let lock = FileLock::try_lock(cache.root.lock_file("catalog.lock").unwrap(), true).unwrap();
    assert!(cache.commit(&second).is_err());
    assert_eq!(cache.current().unwrap().sha256(), first.sha256());
    drop(lock);
    cache.commit(&second).unwrap();
    assert_eq!(cache.current().unwrap().sha256(), second.sha256());
    assert_eq!(
        cache
            .load_snapshot(&first.sha256().to_string())
            .unwrap()
            .sha256(),
        first.sha256()
    );
    std::fs::write(
        directory
            .path()
            .join("catalog")
            .join(format!("{}.json", second.sha256())),
        b"corrupt",
    )
    .unwrap();
    assert!(cache.current().is_err());
    assert!(cache.load_snapshot("../../escape").is_err());
}
