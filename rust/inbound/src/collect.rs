use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::{write::GzEncoder, Compression};
use tauri::{AppHandle, Manager, Runtime};

use crate::report::{LogPreview, SystemInfo};

#[derive(Clone, Debug)]
pub struct Attachment {
    pub filename: String,
    pub bytes: Vec<u8>,
    pub mime: String,
}

pub fn submitted_at() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "unknown".to_owned())
}

pub fn system() -> SystemInfo {
    SystemInfo {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        locale: std::env::var("LC_ALL")
            .ok()
            .or_else(|| std::env::var("LC_MESSAGES").ok())
            .or_else(|| std::env::var("LANG").ok())
            .filter(|value| !value.is_empty()),
    }
}

pub fn preview_log_tail<R: Runtime>(
    app: &AppHandle<R>,
    max_bytes: usize,
) -> Result<LogPreview, String> {
    let bytes = read_log_tail(app, max_bytes)?;
    let Some(bytes) = bytes else {
        return Ok(LogPreview {
            exists: false,
            byte_count: 0,
            preview_text: String::new(),
            gzipped_size: 0,
        });
    };

    let gzipped = gzip(&bytes)?;
    let preview_start = bytes.len().saturating_sub(4 * 1024);

    Ok(LogPreview {
        exists: true,
        byte_count: bytes.len(),
        preview_text: String::from_utf8_lossy(&bytes[preview_start..]).into_owned(),
        gzipped_size: gzipped.len(),
    })
}

pub fn log_tail_attachment<R: Runtime>(
    app: &AppHandle<R>,
    max_bytes: usize,
) -> Result<Option<Attachment>, String> {
    let Some(bytes) = read_log_tail(app, max_bytes)? else {
        return Ok(None);
    };

    let filename = format!("{}.log.gz", app.package_info().name);
    Ok(Some(Attachment {
        filename,
        bytes: gzip(&bytes)?,
        mime: "application/gzip".to_owned(),
    }))
}

fn read_log_tail<R: Runtime>(
    app: &AppHandle<R>,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>, String> {
    let path = app
        .path()
        .app_log_dir()
        .map_err(|err| err.to_string())?
        .join(format!("{}.log", app.package_info().name));

    if !path.exists() {
        return Ok(None);
    }

    let bytes = std::fs::read(&path).map_err(|err| format!("failed to read log file: {err}"))?;
    if bytes.len() <= max_bytes {
        return Ok(Some(bytes));
    }

    Ok(Some(bytes[bytes.len() - max_bytes..].to_vec()))
}

fn gzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(bytes)
        .map_err(|err| format!("failed to gzip log tail: {err}"))?;
    encoder
        .finish()
        .map_err(|err| format!("failed to finish gzip log tail: {err}"))
}
