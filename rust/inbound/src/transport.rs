use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use serde_json::{json, Value};

use crate::collect::Attachment;
use crate::report::{AppId, Report};

#[async_trait]
pub trait Transport: Send + Sync {
    async fn submit(&self, report: &Report, attachments: Vec<Attachment>) -> Result<(), String>;
}

pub struct WebhookTransport {
    app: AppId,
    client: reqwest::Client,
}

impl WebhookTransport {
    pub fn new(app: AppId) -> Self {
        Self {
            app,
            client: reqwest::Client::new(),
        }
    }
}

const CONTENT_LIMIT: usize = 2000;
const FIELD_VALUE_LIMIT: usize = 1024;

fn webhook_payload(report: &Report, attachments: &[Attachment]) -> Value {
    json!({
        "username": format!("{} inbound", report.app.as_str()),
        "thread_name": format!(
            "{} {}",
            report.app.as_str(),
            report
                .build
                .as_ref()
                .map(|build| build.app_commit.as_str())
                .unwrap_or("unknown")
        ),
        "content": truncate_chars(&report.message, CONTENT_LIMIT),
        "embeds": [{
            "title": format!("{} bug report", report.app.as_str()),
            "color": match report.app {
                AppId::Scrybe => 0x4f9cff,
                AppId::Pepo => 0x8fdaff,
            },
            "fields": embed_fields(report, attachments)
        }],
        "allowed_mentions": {
            "parse": []
        }
    })
}

fn embed_fields(report: &Report, attachments: &[Attachment]) -> Vec<Value> {
    let mut fields = vec![
        json!({
            "name": "Discord user",
            "value": truncate_chars(
                report.discord_user.as_deref().unwrap_or("(not provided)"),
                FIELD_VALUE_LIMIT
            ),
            "inline": true
        }),
        json!({
            "name": "Submitted",
            "value": report.submitted_at,
            "inline": true
        }),
        json!({
            "name": "Build",
            "value": truncate_chars(
                &report.build.as_ref().map(|build| {
                    format!("{} / {} / {}", build.app_version, build.app_commit, build.build_time)
                }).unwrap_or_else(|| "(not included)".to_owned()),
                FIELD_VALUE_LIMIT
            ),
            "inline": false
        }),
        json!({
            "name": "Included",
            "value": report.included_sections(),
            "inline": false
        }),
    ];

    if let Some(system) = report.system.as_ref() {
        fields.push(json!({
            "name": "System",
            "value": truncate_chars(&system_summary(system), FIELD_VALUE_LIMIT),
            "inline": false
        }));
    }

    if report.include.error {
        fields.push(json!({
            "name": "Error",
            "value": truncate_chars(&error_summary(report), FIELD_VALUE_LIMIT),
            "inline": false
        }));
    }

    fields.push(json!({
        "name": "Report",
        "value": format!("Full report attached as `{}`.", report.json_attachment_filename()),
        "inline": false
    }));
    fields.push(json!({
        "name": "Attachments",
        "value": truncate_chars(&Report::attachments_summary(attachments), FIELD_VALUE_LIMIT),
        "inline": false
    }));

    fields
}

fn system_summary(system: &crate::report::SystemInfo) -> String {
    match system.locale.as_deref() {
        Some(locale) => format!("{} / {} / {}", system.os, system.arch, locale),
        None => format!("{} / {}", system.os, system.arch),
    }
}

fn error_summary(report: &Report) -> String {
    let Some(error) = report.error.as_ref() else {
        return "(not captured)".to_owned();
    };

    let mut summary = format!("{} at {}", error.kind, error.timestamp);
    if let Some(target) = error.target.as_deref().filter(|target| !target.is_empty()) {
        summary.push_str(" / ");
        summary.push_str(target);
    }
    if !error.message.is_empty() {
        summary.push('\n');
        summary.push_str(&error.message);
    }
    summary
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }

    let suffix = "...";
    let take = max_chars.saturating_sub(suffix.len());
    let mut truncated = value.chars().take(take).collect::<String>();
    truncated.push_str(suffix);
    truncated
}

#[async_trait]
impl Transport for WebhookTransport {
    async fn submit(&self, report: &Report, attachments: Vec<Attachment>) -> Result<(), String> {
        let url = self
            .app
            .webhook_url()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("missing inbound webhook URL for {}", self.app))?;

        let payload = webhook_payload(report, &attachments);

        let mut form = Form::new().text("payload_json", payload.to_string());
        for (index, attachment) in attachments.into_iter().enumerate() {
            let part = Part::bytes(attachment.bytes)
                .file_name(attachment.filename)
                .mime_str(&attachment.mime)
                .map_err(|err| err.to_string())?;
            form = form.part(format!("files[{index}]"), part);
        }

        let response = self
            .client
            .post(url)
            .multipart(form)
            .send()
            .await
            .map_err(|err| format!("failed to submit report: {err}"))?;

        if response.status().is_success() {
            return Ok(());
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(format!("Discord webhook returned {status}: {body}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{BuildInfo, ErrorContext, IncludeFlags, SystemInfo};

    fn sample_report() -> Report {
        Report::new(
            AppId::Pepo,
            BuildInfo {
                app_version: "0.2.23".to_owned(),
                app_commit: "ece1528".to_owned(),
                build_time: "1778451489".to_owned(),
            },
            "1778451510".to_owned(),
            None,
            "full message".to_owned(),
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
            None,
            Some(ErrorContext {
                kind: "error".to_owned(),
                message: "inbound submit failed".to_owned(),
                target: Some("webview".to_owned()),
                timestamp: "1778451510".to_owned(),
            }),
        )
    }

    #[test]
    fn webhook_payload_summarizes_and_points_to_full_report() {
        let report = sample_report();
        let attachments = vec![
            report.json_attachment().unwrap(),
            Attachment {
                filename: "pepo.log.gz".to_owned(),
                bytes: vec![1, 2, 3],
                mime: "application/gzip".to_owned(),
            },
        ];
        let payload = webhook_payload(&report, &attachments);

        assert_eq!(payload["thread_name"], "pepo ece1528");
        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        assert!(fields.iter().any(|field| field["name"] == "Included"
            && field["value"] == "system, build, log, config, error"));
        assert!(fields
            .iter()
            .any(|field| field["name"] == "System"
                && field["value"] == "macos / aarch64 / en_US.UTF-8"));
        assert!(fields.iter().any(|field| field["name"] == "Error"
            && field["value"]
                .as_str()
                .unwrap()
                .contains("inbound submit failed")));
        assert!(fields.iter().any(|field| field["name"] == "Report"
            && field["value"] == "Full report attached as `pepo-report.json`."));
        assert!(fields.iter().any(|field| field["name"] == "Attachments"
            && field["value"]
                .as_str()
                .unwrap()
                .contains("pepo-report.json")));
    }

    #[test]
    fn truncate_chars_preserves_char_boundaries() {
        assert_eq!(truncate_chars("abcdef", 6), "abcdef");
        assert_eq!(truncate_chars("abc😀def", 6), "abc...");
    }
}
