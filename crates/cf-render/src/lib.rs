//! GPU rendering for ClipFrame on Direct3D 11 + Direct2D + DirectWrite.
//!
//! Chosen over wgpu/Direct3D 12 after measuring on real hardware: device creation takes
//! ~65–110 ms instead of ~230–410 ms, costs ~7 MB instead of ~150 MB, and memory returns to
//! baseline when the device is dropped (see PLAN §2).
//!
//! All `unsafe` COM calls stay inside this crate behind safe types: [`Gpu`] (shared device),
//! [`WindowSurface`] (one swap chain per window), [`GpuImage`] (uploaded bitmap) and
//! [`Canvas`] (drawing primitives in physical pixels).

#![warn(missing_docs)]

mod canvas;
mod gpu;
mod image;
mod keep_alive;
mod surface;

pub use canvas::{Canvas, Color, RectF, TextBlock};
pub use gpu::{Gpu, GpuError};
pub use image::GpuImage;
pub use keep_alive::KeepAlive;
pub use surface::WindowSurface;
