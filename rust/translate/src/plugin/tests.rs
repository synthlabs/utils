use super::*;
use crate::*;

#[test]
fn dto_contract_keeps_language_validation_skips_and_full_width_bytes() {
    let request: dto::RequestDto = serde_json::from_value(serde_json::json!({
        "text": "hello", "source": "RUS", "target": "ENG"
    }))
    .unwrap();
    let request = TranslationRequest::try_from(request).unwrap();
    assert_eq!(request.source, SourceLanguage::Explicit(Language::RUSSIAN));
    let invalid: dto::RequestDto =
        serde_json::from_value(serde_json::json!({"text": "x", "target": "en"})).unwrap();
    assert_eq!(
        TranslationRequest::try_from(invalid).unwrap_err().code,
        ErrorCode::InvalidLanguage
    );
    let outcome: dto::OutcomeDto = TranslationOutcome::Skipped(SkipReason::BlankInput).into();
    assert_eq!(
        serde_json::to_value(outcome).unwrap(),
        serde_json::json!({"kind":"skipped", "reason":{"kind":"blank_input"}})
    );
    let progress: dto::InstallProgressDto = InstallProgress {
        network_bytes: u64::MAX,
        ..InstallProgress::default()
    }
    .into();
    assert_eq!(
        serde_json::to_value(progress).unwrap()["networkBytes"],
        u64::MAX.to_string()
    );
}

#[test]
fn errors_preserve_nested_categories() {
    assert_eq!(
        ServiceError::from(WorkloadError::Store(std::sync::Arc::new(StoreError::Busy(
            LanguagePair {
                source: Language::RUSSIAN,
                target: Language::ENGLISH,
            }
        ))))
        .code,
        ErrorCode::Busy
    );
    assert_eq!(
        ServiceError::from(InstallError::Store(StoreError::RecoveryRequired {
            pair: LanguagePair {
                source: Language::RUSSIAN,
                target: Language::ENGLISH
            },
            detail: "restore backup".into(),
        }))
        .code,
        ErrorCode::RecoveryRequired
    );
}
