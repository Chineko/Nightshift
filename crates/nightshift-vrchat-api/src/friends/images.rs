use std::{collections::HashMap, sync::{Arc, Mutex, OnceLock}, time::{Duration, Instant}};

use anyhow::{Context, Result, bail};
use reqwest::Url;

use super::super::VrchatApiClient;

pub const FRIEND_IMAGE_CACHE_TTL: Duration = Duration::from_secs(60 * 60 * 6);
const MAX_FRIEND_IMAGE_BYTES: usize = 8 * 1024 * 1024;

struct CachedImage {
    loaded_at: Instant,
    bytes: Arc<[u8]>,
}

fn cache() -> &'static Mutex<HashMap<String, CachedImage>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedImage>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

impl VrchatApiClient {
    pub async fn friend_image_bytes(&self, value: &str) -> Result<Arc<[u8]>> {
        let url = validate_friend_image_url(value)?;
        let key = url.as_str().to_owned();
        let now = Instant::now();

        if let Ok(cache) = cache().lock()
            && let Some(entry) = cache.get(&key)
            && now.saturating_duration_since(entry.loaded_at) < FRIEND_IMAGE_CACHE_TTL
        {
            return Ok(entry.bytes.clone());
        }

        let response = self.http.get(url).send().await.context("requesting VRChat friend image")?;
        let response = response.error_for_status().context("VRChat friend image request failed")?;
        if response.content_length().is_some_and(|len| len > MAX_FRIEND_IMAGE_BYTES as u64) {
            bail!("VRChat friend image exceeds the 8 MiB safety limit");
        }
        let bytes = response.bytes().await.context("reading VRChat friend image")?;
        if bytes.len() > MAX_FRIEND_IMAGE_BYTES {
            bail!("VRChat friend image exceeds the 8 MiB safety limit");
        }
        let bytes: Arc<[u8]> = Arc::from(bytes.as_ref());
        if let Ok(mut cache) = cache().lock() {
            cache.insert(key, CachedImage { loaded_at: now, bytes: bytes.clone() });
        }
        Ok(bytes)
    }

    pub fn invalidate_friend_image_cache(&self) {
        if let Ok(mut cache) = cache().lock() {
            cache.clear();
        }
    }
}

fn validate_friend_image_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).context("parsing VRChat friend image URL")?;
    if url.scheme() != "https" || url.host_str().is_none() {
        bail!("VRChat friend image URL must be an absolute HTTPS URL");
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_friend_images_are_allowed() {
        assert!(validate_friend_image_url("https://api.vrchat.cloud/image.png").is_ok());
        assert!(validate_friend_image_url("http://example.com/image.png").is_err());
        assert!(validate_friend_image_url("file:///tmp/image.png").is_err());
    }

    #[test]
    fn cache_ttl_is_six_hours() {
        assert_eq!(FRIEND_IMAGE_CACHE_TTL, Duration::from_secs(21_600));
    }
}
