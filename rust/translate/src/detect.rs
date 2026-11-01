use crate::{Detection, Language};

pub(crate) trait LanguageDetector: Send + Sync {
    fn detect(&self, text: &str) -> Option<Detection>;
}

pub(crate) struct WhatlangDetector(whatlang::Detector);

impl Default for WhatlangDetector {
    fn default() -> Self {
        Self(whatlang::Detector::new())
    }
}

impl LanguageDetector for WhatlangDetector {
    fn detect(&self, text: &str) -> Option<Detection> {
        self.0.detect(text).map(|info| Detection {
            language: Language::new(info.lang().code())
                .expect("whatlang returns ISO 639-3 identifiers"),
            confidence: info.confidence(),
            reliable: info.is_reliable(),
        })
    }
}
