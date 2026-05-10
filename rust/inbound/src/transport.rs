use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use serde_json::json;

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

#[async_trait]
impl Transport for WebhookTransport {
    async fn submit(&self, report: &Report, attachments: Vec<Attachment>) -> Result<(), String> {
        let url = self
            .app
            .webhook_url()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("missing inbound webhook URL for {}", self.app))?;

        let thread_name = format!(
            "{} {}",
            report.app.as_str(),
            report
                .build
                .as_ref()
                .map(|build| build.app_commit.as_str())
                .unwrap_or("unknown")
        );

        let payload = json!({
            "username": format!("{} inbound", report.app.as_str()),
            "thread_name": thread_name,
            "content": report.message,
            "embeds": [{
                "title": format!("{} bug report", report.app.as_str()),
                "color": match report.app {
                    AppId::Scrybe => 0x4f9cff,
                    AppId::Pepo => 0x8fdaff,
                },
                "fields": [
                    {
                        "name": "Discord user",
                        "value": report.discord_user.as_deref().unwrap_or("(not provided)"),
                        "inline": true
                    },
                    {
                        "name": "Submitted",
                        "value": report.submitted_at,
                        "inline": true
                    },
                    {
                        "name": "Build",
                        "value": report.build.as_ref().map(|build| {
                            format!("{} / {} / {}", build.app_version, build.app_commit, build.build_time)
                        }).unwrap_or_else(|| "(not included)".to_owned()),
                        "inline": false
                    },
                    {
                        "name": "Attachments",
                        "value": Report::attachments_summary(&attachments),
                        "inline": false
                    }
                ]
            }],
            "allowed_mentions": {
                "parse": []
            }
        });

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
