use std::sync::Arc;

use anyhow::{Context, Result, bail};
use reqwest::Url;

use super::super::VrchatApiClient;

const MAX_FRIEND_IMAGE_BYTES: usize = 2 * 1024 * 1024;

impl VrchatApiClient {
    pub async fn friend_image_bytes(&self, value: &str) -> Result<Arc<[u8]>> {
        let url = validate_friend_image_url(value)?;
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

        // The transport only hands the parcel upstairs. The UI keeps the tiny thumbnail, not this box.
        Ok(Arc::from(bytes.as_ref()))
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
    fn per_image_limit_is_two_mib() {
        assert_eq!(MAX_FRIEND_IMAGE_BYTES, 2 * 1024 * 1024);
    }
}
