//! Rendering into a bitmap and reading it back (exporting an edited capture).

use std::mem::ManuallyDrop;

use cf_core::image::Image;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_MAP_OPTIONS_READ, ID2D1Bitmap1, ID2D1DeviceContext,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

use crate::canvas::Canvas;
use crate::kit::Kit;
use crate::{Gpu, GpuError};

/// Paints `width × height` pixels with `paint` on the GPU and returns them as an image.
///
/// # Errors
/// [`GpuError`] when the size exceeds the GPU's bitmap limit or a call fails.
pub fn render_to_image(
    gpu: &Gpu,
    size: (u32, u32),
    paint: impl FnOnce(&Canvas<'_>),
) -> Result<Image, GpuError> {
    // SAFETY: plain factory call on a valid device.
    let dc = unsafe {
        gpu.d2d
            .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?
    };
    let kit = Kit::new(gpu, &dc, 13.0)?;
    let target = create_bitmap(
        &dc,
        size,
        D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    )?;
    // SAFETY: the target is a TARGET bitmap of this context; BeginDraw/EndDraw are paired.
    unsafe {
        dc.SetTarget(&target);
        dc.BeginDraw();
    }
    paint(&Canvas::new(&dc, &kit, gpu, size));
    // SAFETY: see above.
    unsafe { dc.EndDraw(None, None)? };
    let readback = create_bitmap(
        &dc,
        size,
        D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    )?;
    // SAFETY: both bitmaps have the same size and format and belong to this device.
    unsafe { readback.CopyFromBitmap(None, &target, None)? };
    read_pixels(&readback, size)
}

fn create_bitmap(
    dc: &ID2D1DeviceContext,
    size: (u32, u32),
    options: D2D1_BITMAP_OPTIONS,
) -> Result<ID2D1Bitmap1, GpuError> {
    let properties = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: options,
        colorContext: ManuallyDrop::new(None),
    };
    let size = D2D_SIZE_U {
        width: size.0.max(1),
        height: size.1.max(1),
    };
    // SAFETY: no source data; the properties outlive the call.
    Ok(unsafe { dc.CreateBitmap(size, None, 0, &raw const properties)? })
}

fn read_pixels(bitmap: &ID2D1Bitmap1, size: (u32, u32)) -> Result<Image, GpuError> {
    let row = size.0 as usize * 4;
    let mut pixels = Vec::with_capacity(row * size.1 as usize);
    // SAFETY: a CPU_READ bitmap may be mapped for reading; while mapped, `bits` points to
    // `pitch * height` readable bytes. It is unmapped before returning.
    unsafe {
        let mapped = bitmap.Map(D2D1_MAP_OPTIONS_READ)?;
        let pitch = mapped.pitch as usize;
        for y in 0..size.1 as usize {
            let start = mapped.bits.add(y * pitch);
            pixels.extend_from_slice(std::slice::from_raw_parts(start, row));
        }
        bitmap.Unmap()?;
    }
    Image::from_bgra(size.0, size.1, pixels).ok_or_else(|| GpuError(E_FAIL.into()))
}
