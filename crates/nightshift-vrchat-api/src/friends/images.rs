use std::{sync::{Arc, OnceLock}, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::{Client as HttpClient, Url, redirect::Policy};

use super::super::VrchatApiClient;

const MAX_FRIEND_IMAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_FRIEND_IMAGE_REDIRECTS: usize = 3;
const IMAGE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const IMAGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const ALLOWED_IMAGE_DOMAINS: [&str; 2] = ["vrchat.cloud", "vrchat.com"];

static IMAGE_HTTP: OnceLock<Result<HttpClient, String>> = OnceLock::new();

fn image_client() -> Result<&'static HttpClient> {
    match IMAGE_HTTP.get_or_init(|| build_image_client().map_err(|error| error.to_string())) {
        Ok(client) => Ok(client),
        Err(error) => bail!("building VRChat image HTTP client: {error}"),
    }
}

fn build_image_client() -> Result<HttpClient> {
    HttpClient::builder()
        .user_agent(format!(
            "Nightshift/{} (https://github.com/Chineko/Nightshift)",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(IMAGE_CONNECT_TIMEOUT)
        .timeout(IMAGE_REQUEST_TIMEOUT)
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_FRIEND_IMAGE_REDIRECTS {
                attempt.error("too many VRChat friend image redirects")
            } else if is_allowed_image_url(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("VRChat friend image redirected to a disallowed location")
            }
        }))
        .build()
        .context("building VRChat image HTTP client")
}

impl VrchatApiClient {
    pub async fn friend_image_bytes(&self, value: &str) -> Result<Arc<[u8]>> {
        let url = validate_friend_image_url(value)?;
        let mut response = image_client()?
            .get(url)
            .send()
            .await
            .context("requesting VRChat friend image")?
            .error_for_status()
            .context("VRChat friend image request failed")?;

        if response
            .content_length()
            .is_some_and(|len| len > MAX_FRIEND_IMAGE_BYTES as u64)
        {
            bail!("VRChat friend image exceeds the 2 MiB safety limit");
        }

        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .context("reading VRChat friend image")?
        {
            if !within_image_limit(bytes.len(), chunk.len()) {
                bail!("VRChat friend image exceeds the 2 MiB safety limit");
            }
            bytes.extend_from_slice(&chunk);
        }

        // The transport only hands the parcel upstairs. The UI keeps the tiny thumbnail, not this box.
        Ok(Arc::from(bytes))
    }
}

fn within_image_limit(current: usize, incoming: usize) -> bool {
    current
        .checked_add(incoming)
        .is_some_and(|total| total <= MAX_FRIEND_IMAGE_BYTES)
}

fn is_allowed_image_host(host: &str) -> bool {
    ALLOWED_IMAGE_DOMAINS.iter().any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.') && prefix.len() > 1)
    })
}

fn is_allowed_image_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.host_str().is_some_and(is_allowed_image_host)
}

fn validate_friend_image_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).context("parsing VRChat friend image URL")?;
    if !is_allowed_image_url(&url) {
        bail!("VRChat friend image URL is not on an allowed VRChat HTTPS host");
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_friend_images_are_allowed() {
        assert!(validate_friend_image_url("https://api.vrchat.cloud/image.png").is_ok());
        assert!(validate_friend_image_url("http://api.vrchat.cloud/image.png").is_err());
        assert!(validate_friend_image_url("file:///tmp/image.png").is_err());
    }

    #[test]
    fn friend_images_are_limited_to_vrchat_hosts() {
        assert!(validate_friend_image_url("https://assets.vrchat.com/a.png").is_ok());
        assert!(validate_friend_image_url("https://files.vrchat.cloud/a.png").is_ok());
        assert!(validate_friend_image_url("https://vrchat.com/a.png").is_ok());
        assert!(validate_friend_image_url("https://example.com/a.png").is_err());
        assert!(validate_friend_image_url("https://evilvrchat.com/a.png").is_err());
        assert!(validate_friend_image_url("https://vrchat.com.evil.example/a.png").is_err());
        assert!(validate_friend_image_url("https://api.vrchat.cloud@evil.example/a.png").is_err());
        assert!(validate_friend_image_url("https://user:pw@api.vrchat.cloud/a.png").is_err());
        assert!(validate_friend_image_url("https://api.vrchat.cloud:8443/a.png").is_err());
        assert!(validate_friend_image_url("https://127.0.0.1/a.png").is_err());
        assert!(validate_friend_image_url("https://[::1]/a.png").is_err());
    }

    #[test]
    fn host_matching_requires_a_dot_boundary() {
        assert!(is_allowed_image_host("vrchat.com"));
        assert!(is_allowed_image_host("a.b.vrchat.cloud"));
        assert!(!is_allowed_image_host(".vrchat.com"));
        assert!(!is_allowed_image_host("notvrchat.com"));
        assert!(!is_allowed_image_host(""));
    }

    #[test]
    fn per_image_limit_is_two_mib_and_checked_while_streaming() {
        assert_eq!(MAX_FRIEND_IMAGE_BYTES, 2 * 1024 * 1024);
        assert!(within_image_limit(0, MAX_FRIEND_IMAGE_BYTES));
        assert!(within_image_limit(MAX_FRIEND_IMAGE_BYTES - 1, 1));
        assert!(!within_image_limit(MAX_FRIEND_IMAGE_BYTES, 1));
        assert!(!within_image_limit(usize::MAX, 1));
    }

    #[test]
    fn image_client_builds() {
        build_image_client().unwrap();
    }
}
