use anyhow::{Result, anyhow, bail};
use openxr as xr;

const WAIT_SLICE: xr::Duration = xr::Duration::from_nanos(250_000_000);
const WAIT_ATTEMPTS: usize = 8;

pub(crate) fn wait_image_bounded(
    swapchain: &xr::Swapchain<xr::Vulkan>,
    label: &str,
) -> Result<()> {
    let info = xr::sys::SwapchainImageWaitInfo {
        ty: xr::sys::SwapchainImageWaitInfo::TYPE,
        next: std::ptr::null_mut(),
        timeout: WAIT_SLICE,
    };

    for _ in 0..WAIT_ATTEMPTS {
        let result = unsafe {
            (swapchain.instance().fp().wait_swapchain_image)(swapchain.as_raw(), &info)
        };
        if result == xr::sys::Result::TIMEOUT_EXPIRED {
            continue;
        }
        if result == xr::sys::Result::SUCCESS || result == xr::sys::Result::SESSION_LOSS_PENDING {
            return Ok(());
        }
        return Err(anyhow!("{label}: xrWaitSwapchainImage failed: {result:?}"));
    }

    bail!("{label}: timed out waiting for OpenXR swapchain image")
}

pub(crate) fn release_image(
    swapchain: &xr::Swapchain<xr::Vulkan>,
    label: &str,
) -> Result<()> {
    let result = unsafe {
        (swapchain.instance().fp().release_swapchain_image)(swapchain.as_raw(), std::ptr::null())
    };
    if result == xr::sys::Result::SUCCESS || result == xr::sys::Result::SESSION_LOSS_PENDING {
        Ok(())
    } else {
        Err(anyhow!("{label}: xrReleaseSwapchainImage failed: {result:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_budget_is_bounded() {
        assert_eq!(WAIT_ATTEMPTS, 8);
        assert_eq!(WAIT_SLICE, xr::Duration::from_nanos(250_000_000));
    }
}
