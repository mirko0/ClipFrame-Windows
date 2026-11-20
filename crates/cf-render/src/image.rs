//! Captured images uploaded to the GPU.

use std::mem::ManuallyDrop;

use cf_core::image::Image;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1, ID2D1Bitmap1,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

use crate::{Gpu, GpuError};

/// A BGRA image living in GPU memory, drawable on any window of the same [`Gpu`].
#[derive(Debug, Clone)]
pub struct GpuImage {
    pub(crate) bitmap: ID2D1Bitmap1,
    width: u32,
    height: u32,
}

impl GpuImage {
    /// Uploads `image` — its BGRA layout is Direct2D's native format, so no conversion.
    ///
    /// Captures are opaque (alpha 255) or fully transparent in gaps (all zero), so the bytes
    /// are already valid premultiplied alpha.
    ///
    /// # Errors
    /// [`GpuError`] when the bitmap cannot be created (e.g. larger than the GPU allows).
    pub fn upload(gpu: &Gpu, image: &Image) -> Result<Self, GpuError> {
        let properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: ManuallyDrop::new(None),
        };
        let size = D2D_SIZE_U {
            width: image.width(),
            height: image.height(),
        };
        let pitch = u32::try_from(image.stride()).unwrap_or(u32::MAX);
        // SAFETY: the source pointer covers `height * pitch` readable bytes (guaranteed by
        // `Image`'s invariant) and is only read during the call.
        let bitmap = unsafe {
            gpu.resources.CreateBitmap(
                size,
                Some(image.bgra().as_ptr().cast()),
                pitch,
                &raw const properties,
            )?
        };
        Ok(Self {
            bitmap,
            width: image.width(),
            height: image.height(),
        })
    }

    /// Width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }
}
