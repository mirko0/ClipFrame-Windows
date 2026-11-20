//! The shared Direct3D 11 / Direct2D / DirectWrite device.

use std::time::Instant;

use windows::Win32::Foundation::{E_POINTER, HMODULE};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory,
    ID2D1Device, ID2D1DeviceContext, ID2D1Factory1,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory, IDWriteFactory,
};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGIFactory2};
use windows::core::Interface as _;

/// A Direct3D / Direct2D / DirectWrite call failed.
#[derive(Debug, thiserror::Error)]
#[error("graphics call failed")]
pub struct GpuError(#[from] pub windows::core::Error);

/// The shared device. Create once per capture session; cheap to drop and recreate.
#[derive(Debug)]
pub struct Gpu {
    pub(crate) d3d: ID3D11Device,
    pub(crate) dxgi_factory: IDXGIFactory2,
    pub(crate) d2d: ID2D1Device,
    pub(crate) dwrite: IDWriteFactory,
    /// Device context used only to create shared resources (bitmaps) — resources created on
    /// one context of a Direct2D device are usable by every context of that device.
    pub(crate) resources: ID2D1DeviceContext,
}

impl Gpu {
    /// Creates the device on the hardware adapter, falling back to WARP (software).
    ///
    /// # Errors
    /// [`GpuError`] when neither adapter works.
    pub fn new() -> Result<Self, GpuError> {
        let started = Instant::now();
        let d3d = create_d3d(D3D_DRIVER_TYPE_HARDWARE).or_else(|err| {
            tracing::warn!("hardware Direct3D 11 unavailable ({err}); using WARP");
            create_d3d(D3D_DRIVER_TYPE_WARP)
        })?;
        let dxgi_device: IDXGIDevice = d3d.cast()?;
        // SAFETY: plain COM getters on valid interfaces; results are owned smart pointers.
        let dxgi_factory: IDXGIFactory2 = unsafe { dxgi_device.GetAdapter()?.GetParent()? };
        // SAFETY: creating a single-threaded factory with default options has no
        // preconditions; every object made from it is used on this thread only.
        let factory: ID2D1Factory1 =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        // SAFETY: `dxgi_device` is a valid DXGI device of a BGRA-capable D3D11 device.
        let d2d = unsafe { factory.CreateDevice(&dxgi_device)? };
        // SAFETY: as above, a plain factory call on a valid device.
        let resources = unsafe { d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)? };
        // SAFETY: creating the shared DirectWrite factory has no preconditions.
        let dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis(),
            "GPU device ready"
        );
        Ok(Self {
            d3d,
            dxgi_factory,
            d2d,
            dwrite,
            resources,
        })
    }
}

fn create_d3d(driver: D3D_DRIVER_TYPE) -> windows::core::Result<ID3D11Device> {
    let mut device = None;
    // SAFETY: `device` is a valid out-pointer; no adapter, software module or feature-level
    // list is passed, so the defaults apply. BGRA support is required by Direct2D.
    unsafe {
        D3D11CreateDevice(
            None,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&raw mut device),
            None,
            None,
        )?;
    }
    device.ok_or_else(|| E_POINTER.into())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    #[ignore = "needs a GPU or WARP"]
    fn creates_device() {
        assert!(Gpu::new().is_ok());
    }
}
