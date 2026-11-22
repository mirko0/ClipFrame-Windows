//! A flip-model swap chain for one window, with a Direct2D context drawing into it.

use std::mem::ManuallyDrop;

use crate::canvas::Canvas;
use crate::kit::Kit;
use crate::{Gpu, GpuError};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{E_INVALIDARG, HWND};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, ID2D1DeviceContext,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_MWA_NO_ALT_ENTER, DXGI_PRESENT, DXGI_SCALING_NONE, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT,
    IDXGISurface, IDXGISwapChain1,
};

/// Draws into one window. Everything is in physical pixels (96 DPI context).
#[derive(Debug)]
pub struct WindowSurface {
    swap_chain: IDXGISwapChain1,
    dc: ID2D1DeviceContext,
    kit: Kit,
    size: (u32, u32),
}

impl WindowSurface {
    /// Creates a swap chain for `window`, with UI text of `text_px` pixels.
    ///
    /// # Errors
    /// [`GpuError`] when `window` is not a Win32 window or a graphics call fails.
    pub fn new(
        gpu: &Gpu,
        window: &impl HasWindowHandle,
        size: (u32, u32),
        text_px: f32,
    ) -> Result<Self, GpuError> {
        let hwnd = hwnd_of(window)?;
        let swap_chain = create_swap_chain(gpu, hwnd, size)?;
        // SAFETY: plain factory call on a valid device.
        let dc = unsafe {
            gpu.d2d
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?
        };
        let kit = Kit::new(gpu, &dc, text_px)?;
        let surface = Self {
            swap_chain,
            dc,
            kit,
            size,
        };
        surface.bind_back_buffer()?;
        Ok(surface)
    }

    /// Current size in pixels.
    pub const fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Resizes the swap chain buffers.
    ///
    /// # Errors
    /// [`GpuError`] when the buffers cannot be resized.
    pub fn resize(&mut self, size: (u32, u32)) -> Result<(), GpuError> {
        if size == self.size || size.0 == 0 || size.1 == 0 {
            return Ok(());
        }
        // SAFETY: the back buffer must be released (target unset) before ResizeBuffers.
        unsafe {
            self.dc.SetTarget(None);
            self.swap_chain.ResizeBuffers(
                0,
                size.0,
                size.1,
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
        }
        self.size = size;
        self.bind_back_buffer()
    }

    /// Draws one frame with `paint` and presents it without waiting for the display's
    /// refresh: Windows composes the newest frame at the next refresh and drops older ones.
    /// (Waiting for vsync here blocked the thread once per window, so with an overlay window
    /// on each monitor the pointer fell behind on the second one.)
    ///
    /// # Errors
    /// [`GpuError`] when drawing or presenting fails (e.g. the GPU was reset).
    pub fn draw(&self, gpu: &Gpu, paint: impl FnOnce(&Canvas<'_>)) -> Result<(), GpuError> {
        let canvas = Canvas::new(&self.dc, &self.kit, gpu, self.size);
        // SAFETY: BeginDraw/EndDraw are paired on this thread's context with a bound target.
        unsafe { self.dc.BeginDraw() };
        paint(&canvas);
        // SAFETY: see above; Present on a valid flip-model swap chain.
        unsafe {
            self.dc.EndDraw(None, None)?;
            self.swap_chain.Present(0, DXGI_PRESENT(0)).ok()?;
        }
        Ok(())
    }

    fn bind_back_buffer(&self) -> Result<(), GpuError> {
        let properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: ManuallyDrop::new(None),
        };
        // SAFETY: buffer 0 of a D3D11 flip-model swap chain is always the current back
        // buffer; the bitmap wrapping it is set as this context's target.
        unsafe {
            let buffer: IDXGISurface = self.swap_chain.GetBuffer(0)?;
            let target = self
                .dc
                .CreateBitmapFromDxgiSurface(&buffer, Some(&raw const properties))?;
            self.dc.SetTarget(&target);
        }
        Ok(())
    }
}

fn hwnd_of(window: &impl HasWindowHandle) -> Result<HWND, GpuError> {
    let handle = window
        .window_handle()
        .map_err(|_| GpuError(E_INVALIDARG.into()))?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Ok(HWND(h.hwnd.get() as *mut _)),
        _ => Err(GpuError(E_INVALIDARG.into())),
    }
}

fn create_swap_chain(gpu: &Gpu, hwnd: HWND, size: (u32, u32)) -> Result<IDXGISwapChain1, GpuError> {
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: size.0,
        Height: size.1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        Scaling: DXGI_SCALING_NONE,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        ..Default::default()
    };
    // SAFETY: `hwnd` is a live window owned by the caller for at least as long as this
    // surface; the device and descriptor are valid.
    unsafe {
        let swap_chain =
            gpu.dxgi_factory
                .CreateSwapChainForHwnd(&gpu.d3d, hwnd, &raw const desc, None, None)?;
        gpu.dxgi_factory
            .MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER)?;
        Ok(swap_chain)
    }
}
