use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use reqwest::Url;

use super::super::VrchatApiClient;

pub const FRIEND_IMAGE_CACHE_TTL: Duration = Duration::from_secs(60 * 60 * 6);
const MAX_FRIEND_IMAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_FRIEND_IMAGE_CACHE_ENTRIES: usize = 128;
const MAX_FRIEND_IMAGE_CACHE_BYTES: usize = 64 * 1024 * 1024;

struct CachedImage {
    loaded_at: Instant,
    bytes: Arc<[u8]>,
}

fn cache() -> &'static Mutex<HashMap<String, CachedImage>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedImage>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn prune_cache(cache: &mut HashMap<String, CachedImage>, now: Instant) {
    cache.retain(|_, entry| {
        now.saturating_duration_since(entry.loaded_at) < FRIEND_IMAGE_CACHE_TTL
    });

    while cache.len() > MAX_FRIEND_IMAGE_CACHE_ENTRIES
        || cache.values().map(|entry| entry.bytes.len()).sum::<usize>()
            > MAX_FRIEND_IMAGE_CACHE_BYTES
    {
        let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, entry)| entry.loaded_at)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        cache.remove(&oldest_key);
    }
}

impl VrchatApiClient {
    pub async fn friend_image_bytes(&self, value: &str) -> Result<Arc<[u8]>> {
        let url = validate_friend_image_url(value)?;
        let key = url.as_str().to_owned();
        let now = Instant::now();

        if let Ok(mut cache) = cache().lock() {
            prune_cache(&mut cache, now);
            if let Some(entry) = cache.get(&key) {
                return Ok(entry.bytes.clone());
            }
        }

        let response = self
            .http
            .get(url)
            .send()
            .await
            .context("requesting VRChat friend image")?;
        let response = response
            .error_for_status()
            .context("VRChat friend image request failed")?;
        if response
            .content_length()
            .is_some_and(|len| len > MAX_FRIEND_IMAGE_BYTES as u64)
        {
            bail!("VRChat friend image exceeds the 2 MiB safety limit");
        }
        let bytes = response
            .bytes()
            .await
            .context("reading VRChat friend image")?;
        if bytes.len() > MAX_FRIEND_IMAGE_BYTES {
            bail!("VRChat friend image exceeds the 2 MiB safety limit");
        }
        let bytes: Arc<[u8]> = Arc::from(bytes.as_ref());
        if let Ok(mut cache) = cache().lock() {
            cache.insert(
                key,
                CachedImage {
                    loaded_at: now,
                    bytes: bytes.clone(),
                },
            );
            // Avatars may be cute, but they do not get infinite RAM.
            prune_cache(&mut cache, now);
        }
        Ok(bytes)
    }

    pub fn friend_image_cache_metrics(&self) -> (usize, usize, Option<Duration>) {
        let now = Instant::now();
        let Ok(mut cache) = cache().lock() else {
            return (0, 0, None);
        };
        prune_cache(&mut cache, now);
        (
            cache.len(),
            cache.values().map(|entry| entry.bytes.len()).sum(),
            cache
                .values()
                .map(|entry| now.saturating_duration_since(entry.loaded_at))
                .max(),
        )
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

    fn image(loaded_at: Instant, bytes: usize) -> CachedImage {
        CachedImage {
            loaded_at,
            bytes: Arc::from(vec![0u8; bytes]),
        }
    }

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

    #[test]
    fn per_image_limit_is_two_mib() {
        assert_eq!(MAX_FRIEND_IMAGE_BYTES, 2 * 1024 * 1024);
    }

    #[test]
    fn cache_pruning_removes_expired_and_oldest_entries() {
        let now = Instant::now();
        let mut entries = HashMap::new();
        entries.insert(
            "expired".into(),
            image(now - FRIEND_IMAGE_CACHE_TTL, 1),
        );
        for index in 0..=MAX_FRIEND_IMAGE_CACHE_ENTRIES {
            entries.insert(
                format!("image-{index}"),
                image(now - Duration::from_secs(index as u64), 1),
            );
        }

        prune_cache(&mut entries, now);

        assert!(!entries.contains_key("expired"));
        assert_eq!(entries.len(), MAX_FRIEND_IMAGE_CACHE_ENTRIES);
        assert!(!entries.contains_key(&format!("image-{}", MAX_FRIEND_IMAGE_CACHE_ENTRIES)));
    }

    #[test]
    fn cache_pruning_enforces_total_byte_budget() {
        let now = Instant::now();
        let mut entries = HashMap::new();
        for index in 0..33 {
            entries.insert(
                format!("image-{index}"),
                image(now - Duration::from_secs(index), MAX_FRIEND_IMAGE_BYTES),
            );
        }

        prune_cache(&mut entries, now);

        let total_bytes = entries.values().map(|entry| entry.bytes.len()).sum::<usize>();
        assert!(total_bytes <= MAX_FRIEND_IMAGE_CACHE_BYTES);
        assert_eq!(entries.len(), 32);
        assert!(!entries.contains_key("image-32"));
    }
}
