use std::{path::Path, time::Duration};

use super::*;

fn http_error(error: reqwest::Error) -> CatalogError {
    use std::error::Error;
    let mut detail = error.to_string();
    let mut source = error.source();
    while let Some(error) = source {
        detail.push_str(": ");
        detail.push_str(&error.to_string());
        source = error.source();
    }
    CatalogError::Http(detail)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogFreshness {
    Fresh,
    Cached,
}

#[derive(Clone, Debug)]
pub struct CatalogLoad {
    pub snapshot: CatalogSnapshot,
    pub freshness: CatalogFreshness,
    pub refresh_error: Option<CatalogError>,
}

/// Fetches catalog JSON only; this client has no model-download operation.
pub struct CatalogClient {
    cache: CatalogCache,
    client: reqwest::Client,
    #[cfg(test)]
    test_origin: Option<String>,
}

impl CatalogClient {
    pub fn open(store_root: impl AsRef<Path>) -> Result<Self, CatalogError> {
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(http_error)?;
        Ok(Self {
            cache: CatalogCache::open(store_root)?,
            client,
            #[cfg(test)]
            test_origin: None,
        })
    }

    pub fn cached(&self) -> Result<CatalogSnapshot, CatalogError> {
        self.cache.current()
    }

    /// Call once when the catalog service starts; this spawns no background work.
    pub async fn initialize(&self) -> Result<CatalogLoad, CatalogError> {
        self.refresh().await
    }

    pub async fn refresh(&self) -> Result<CatalogLoad, CatalogError> {
        match self.fetch_snapshot().await {
            Ok(snapshot) => match self.cache.commit(&snapshot) {
                Ok(()) => Ok(CatalogLoad {
                    snapshot,
                    freshness: CatalogFreshness::Fresh,
                    refresh_error: None,
                }),
                Err(error) => self.fallback(error),
            },
            Err(error) => self.fallback(error),
        }
    }

    fn fallback(&self, error: CatalogError) -> Result<CatalogLoad, CatalogError> {
        match self.cache.current() {
            Ok(snapshot) => Ok(CatalogLoad {
                snapshot,
                freshness: CatalogFreshness::Cached,
                refresh_error: Some(error),
            }),
            Err(_) => Err(error),
        }
    }

    async fn fetch_snapshot(&self) -> Result<CatalogSnapshot, CatalogError> {
        let pointer =
            CatalogPointer::from_bytes(&self.fetch(DEFAULT_CATALOG_URL, POINTER_LIMIT).await?)?;
        if let Ok(snapshot) = self.cache.load_snapshot(pointer.sha256()) {
            return Ok(snapshot);
        }
        let bytes = self.fetch(pointer.catalog_url(), CATALOG_LIMIT).await?;
        CatalogSnapshot::from_bytes(&bytes, pointer.sha256())
    }

    async fn fetch(&self, url: &str, limit: usize) -> Result<Vec<u8>, CatalogError> {
        #[cfg(test)]
        let rewritten = self
            .test_origin
            .as_ref()
            .map(|origin| url.replacen(ORIGIN, origin, 1));
        #[cfg(test)]
        let url = rewritten.as_deref().unwrap_or(url);
        let mut response = self
            .client
            .get(url)
            .header(reqwest::header::CACHE_CONTROL, "no-cache")
            .send()
            .await
            .map_err(http_error)?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(CatalogError::Http(format!(
                "unexpected HTTP status {}",
                response.status()
            )));
        }
        if response
            .content_length()
            .is_some_and(|size| size > limit as u64)
        {
            return Err(invalid("catalog response exceeds its size limit"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(http_error)? {
            if chunk.len() > limit - bytes.len() {
                return Err(invalid("catalog response exceeds its size limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests;
