use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::collect::Attachment;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppId {
    Scrybe,
    Pepo,
}

impl AppId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scrybe => "scrybe",
            Self::Pepo => "pepo",
        }
    }

    pub fn webhook_url(self) -> Option<&'static str> {
        match self {
            Self::Scrybe => option_env!("INBOUND_WEBHOOK_SCRYBE"),
            Self::Pepo => option_env!("INBOUND_WEBHOOK_PEPO"),
        }
    }
}

impl core::fmt::Display for AppId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BuildInfo {
    pub app_version: String,
    pub app_commit: String,
    pub build_time: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct IncludeFlags {
    pub system: bool,
    pub build: bool,
    pub log: bool,
    pub config: bool,
    pub error: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReportInput {
    pub discord_user: Option<String>,
    pub message: String,
    pub include: IncludeFlags,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub locale: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LogPreview {
    pub exists: bool,
    pub byte_count: usize,
    pub preview_text: String,
    pub gzipped_size: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ErrorContext {
    pub kind: String,
    pub message: String,
    pub target: Option<String>,
    pub timestamp: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub app: AppId,
    pub build: Option<BuildInfo>,
    pub submitted_at: String,
    pub discord_user: Option<String>,
    pub message: String,
    pub include: IncludeFlags,
    pub system: Option<SystemInfo>,
    pub config: Option<Value>,
    pub error: Option<ErrorContext>,
}

impl Report {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        app: AppId,
        build: BuildInfo,
        submitted_at: String,
        discord_user: Option<String>,
        message: String,
        include: IncludeFlags,
        system: Option<SystemInfo>,
        config: Option<Value>,
        error: Option<ErrorContext>,
    ) -> Self {
        Self {
            schema_version: 1,
            app,
            build: include.build.then_some(build),
            submitted_at,
            discord_user: discord_user.and_then(|value| {
                let trimmed = value.trim().to_owned();
                (!trimmed.is_empty()).then_some(trimmed)
            }),
            message,
            include,
            system,
            config,
            error,
        }
    }

    pub(crate) fn json_attachment_filename(&self) -> String {
        format!("{}-report.json", self.app.as_str())
    }

    pub(crate) fn json_attachment(&self) -> Result<Attachment, String> {
        Ok(Attachment {
            filename: self.json_attachment_filename(),
            bytes: serde_json::to_vec_pretty(self)
                .map_err(|err| format!("failed to serialize report attachment: {err}"))?,
            mime: "application/json".to_owned(),
        })
    }

    pub(crate) fn included_sections(&self) -> String {
        let sections = [
            ("system", self.include.system),
            ("build", self.include.build),
            ("log", self.include.log),
            ("config", self.include.config),
            ("error", self.include.error),
        ]
        .into_iter()
        .filter_map(|(name, included)| included.then_some(name))
        .collect::<Vec<_>>();

        if sections.is_empty() {
            "none".to_owned()
        } else {
            sections.join(", ")
        }
    }

    pub(crate) fn attachments_summary(attachments: &[Attachment]) -> String {
        if attachments.is_empty() {
            return "none".to_owned();
        }

        attachments
            .iter()
            .map(|attachment| format!("{} ({} bytes)", attachment.filename, attachment.bytes.len()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_report() -> Report {
        Report::new(
            AppId::Pepo,
            BuildInfo {
                app_version: "0.2.23".to_owned(),
                app_commit: "ece1528".to_owned(),
                build_time: "1778451489".to_owned(),
            },
            "1778451510".to_owned(),
            Some(" jerod ".to_owned()),
            "message".to_owned(),
            IncludeFlags {
                system: true,
                build: true,
                log: true,
                config: true,
                error: true,
            },
            Some(SystemInfo {
                os: "macos".to_owned(),
                arch: "aarch64".to_owned(),
                locale: Some("en_US.UTF-8".to_owned()),
            }),
            Some(json!({ "theme": "dark" })),
            Some(ErrorContext {
                kind: "error".to_owned(),
                message: "inbound submit failed".to_owned(),
                target: Some("webview".to_owned()),
                timestamp: "1778451510".to_owned(),
            }),
        )
    }

    #[test]
    fn json_attachment_contains_full_report() {
        let attachment = sample_report().json_attachment().unwrap();
        assert_eq!(attachment.filename, "pepo-report.json");
        assert_eq!(attachment.mime, "application/json");

        let value: Value = serde_json::from_slice(&attachment.bytes).unwrap();
        assert_eq!(value["app"], "pepo");
        assert_eq!(value["discord_user"], "jerod");
        assert_eq!(value["system"]["os"], "macos");
        assert_eq!(value["config"]["theme"], "dark");
        assert_eq!(value["error"]["message"], "inbound submit failed");
    }

    #[test]
    fn included_sections_describes_enabled_flags() {
        assert_eq!(
            sample_report().included_sections(),
            "system, build, log, config, error"
        );

        let report = Report::new(
            AppId::Scrybe,
            BuildInfo {
                app_version: "1.0.0".to_owned(),
                app_commit: "unknown".to_owned(),
                build_time: "unknown".to_owned(),
            },
            "1778451510".to_owned(),
            None,
            String::new(),
            IncludeFlags::default(),
            None,
            None,
            None,
        );
        assert_eq!(report.included_sections(), "none");
    }
}
