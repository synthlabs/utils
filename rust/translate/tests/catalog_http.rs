#![cfg(feature = "catalog-http")]

use synth_translate::{CatalogClient, CatalogFreshness};

#[tokio::test]
#[ignore = "explicit live metadata probe; never requests model files"]
async fn public_default_catalog_verifies_with_production_https_client() {
    let directory = tempfile::tempdir().unwrap();
    let client = CatalogClient::open(directory.path()).unwrap();
    let loaded = client.initialize().await.unwrap();
    assert_eq!(loaded.freshness, CatalogFreshness::Fresh);
    assert!(loaded.refresh_error.is_none());
    println!("Verified default catalog {}", loaded.snapshot.sha256());
    for model in loaded.snapshot.models() {
        println!(
            "{} {} {}",
            model.id(),
            model.version(),
            model.architecture()
        );
    }
}
