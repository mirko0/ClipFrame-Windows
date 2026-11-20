//! Per-context drawing resources: brush, UI font and stroke styles.

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_CUSTOM, D2D1_DASH_STYLE_SOLID, D2D1_LINE_JOIN_ROUND,
    D2D1_STROKE_STYLE_PROPERTIES, ID2D1DeviceContext, ID2D1Factory, ID2D1SolidColorBrush,
    ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, IDWriteTextFormat,
};
use windows::core::w;

use crate::Gpu;

/// Dash pattern in multiples of the stroke width (round caps extend each dash by one width).
const DASHES: [f32; 2] = [2.0, 2.5];
/// Zero-length dashes with round caps draw as dots.
const DOTS: [f32; 2] = [0.0, 2.0];

/// Resources bound to one device context.
#[derive(Debug)]
pub struct Kit {
    pub brush: ID2D1SolidColorBrush,
    pub ui_text: IDWriteTextFormat,
    pub solid: ID2D1StrokeStyle,
    pub dashed: ID2D1StrokeStyle,
    pub dotted: ID2D1StrokeStyle,
}

impl Kit {
    pub fn new(gpu: &Gpu, dc: &ID2D1DeviceContext, ui_text_px: f32) -> windows::core::Result<Self> {
        // SAFETY: plain factory calls on valid COM objects.
        let brush = unsafe { dc.CreateSolidColorBrush(&D2D1_COLOR_F::default(), None)? };
        Ok(Self {
            brush,
            ui_text: text_format(gpu, ui_text_px, DWRITE_FONT_WEIGHT_SEMI_BOLD)?,
            solid: stroke_style(gpu, None)?,
            dashed: stroke_style(gpu, Some(&DASHES))?,
            dotted: stroke_style(gpu, Some(&DOTS))?,
        })
    }

    /// A Segoe UI format at `px` pixels.
    #[allow(clippy::unused_self)] // a per-kit cache can live here later
    pub fn text_format(
        &self,
        gpu: &Gpu,
        px: f32,
        bold: bool,
    ) -> windows::core::Result<IDWriteTextFormat> {
        let weight = if bold {
            DWRITE_FONT_WEIGHT_SEMI_BOLD
        } else {
            DWRITE_FONT_WEIGHT_NORMAL
        };
        text_format(gpu, px, weight)
    }
}

fn text_format(
    gpu: &Gpu,
    px: f32,
    weight: DWRITE_FONT_WEIGHT,
) -> windows::core::Result<IDWriteTextFormat> {
    // SAFETY: plain factory call with static strings.
    unsafe {
        gpu.dwrite.CreateTextFormat(
            w!("Segoe UI"),
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            px.max(1.0),
            w!("en-us"),
        )
    }
}

fn stroke_style(gpu: &Gpu, dashes: Option<&[f32]>) -> windows::core::Result<ID2D1StrokeStyle> {
    let properties = D2D1_STROKE_STYLE_PROPERTIES {
        startCap: D2D1_CAP_STYLE_ROUND,
        endCap: D2D1_CAP_STYLE_ROUND,
        dashCap: D2D1_CAP_STYLE_ROUND,
        lineJoin: D2D1_LINE_JOIN_ROUND,
        miterLimit: 10.0,
        dashStyle: if dashes.is_some() {
            D2D1_DASH_STYLE_CUSTOM
        } else {
            D2D1_DASH_STYLE_SOLID
        },
        dashOffset: 0.0,
    };
    // SAFETY: `properties` and `dashes` outlive the call.
    unsafe { ID2D1Factory::CreateStrokeStyle(&gpu.factory, &raw const properties, dashes) }
}
