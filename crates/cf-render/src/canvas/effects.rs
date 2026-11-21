//! Image effects on the GPU: blur and pixelate (Direct2D effects over the captured image),
//! the spotlight dim layer (one geometry with holes) and the highlighter (min blend).

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BORDER_MODE_HARD, D2D1_COMPOSITE_MODE_SOURCE_OVER, D2D1_FILL_MODE_WINDING,
};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1GaussianBlur, CLSID_D2D1Scale, D2D1_COMBINE_MODE_EXCLUDE,
    D2D1_DEFAULT_FLATTENING_TOLERANCE, D2D1_GAUSSIANBLUR_PROP_BORDER_MODE,
    D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION, D2D1_INTERPOLATION_MODE_LINEAR,
    D2D1_PRIMITIVE_BLEND_MIN, D2D1_PRIMITIVE_BLEND_SOURCE_OVER, D2D1_PROPERTY_TYPE,
    D2D1_PROPERTY_TYPE_ENUM, D2D1_PROPERTY_TYPE_FLOAT, D2D1_PROPERTY_TYPE_VECTOR2,
    D2D1_ROUNDED_RECT, D2D1_SCALE_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
    D2D1_SCALE_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_SCALE_PROP_BORDER_MODE,
    D2D1_SCALE_PROP_INTERPOLATION_MODE, D2D1_SCALE_PROP_SCALE, ID2D1Effect, ID2D1Geometry,
    ID2D1PathGeometry,
};
use windows::core::GUID;

use super::{Canvas, Color, LineDash, RectF, Stroke};
use crate::GpuImage;

// SAFETY (every method): as for the `Canvas` impl in the parent module — the context is
// between BeginDraw and EndDraw and every argument is valid for the duration of the call.
// Effects and geometries are created on the same device/factory as the context.
impl Canvas<'_> {
    /// Draws `image` with a Gaussian blur of `sigma` pixels, limited to `area`.
    /// Returns `false` if the effect could not be drawn (the caller should cover the area).
    pub fn draw_blurred(&self, image: &GpuImage, area: RectF, sigma: f32) -> bool {
        let Some(blur) = self.effect(&CLSID_D2D1GaussianBlur) else {
            return false;
        };
        // SAFETY: see above.
        unsafe { blur.SetInput(0, &image.bitmap, true) };
        let ok = set(
            &blur,
            D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0,
            D2D1_PROPERTY_TYPE_FLOAT,
            &sigma.to_ne_bytes(),
        ) && set(
            &blur,
            D2D1_GAUSSIANBLUR_PROP_BORDER_MODE.0,
            D2D1_PROPERTY_TYPE_ENUM,
            &hard_border(),
        );
        ok && self.draw_effect(&blur, area)
    }

    /// Draws `image` as `block`-pixel squares (aligned to the image origin), limited to
    /// `area`. Returns `false` if the effect could not be drawn.
    pub fn draw_pixelated(&self, image: &GpuImage, area: RectF, block: f32) -> bool {
        let block = block.max(2.0);
        let (Some(down), Some(up)) = (self.effect(&CLSID_D2D1Scale), self.effect(&CLSID_D2D1Scale))
        else {
            return false;
        };
        // SAFETY: see above.
        unsafe { down.SetInput(0, &image.bitmap, true) };
        // High-quality cubic averages each block; nearest-neighbour scales it back up as
        // hard-edged squares.
        if !scale(
            &down,
            1.0 / block,
            D2D1_SCALE_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC.0,
        ) {
            return false;
        }
        // SAFETY: see above.
        let Ok(small) = (unsafe { down.GetOutput() }) else {
            return false;
        };
        // SAFETY: see above.
        unsafe { up.SetInput(0, &small, true) };
        scale(&up, block, D2D1_SCALE_INTERPOLATION_MODE_NEAREST_NEIGHBOR.0)
            && self.draw_effect(&up, area)
    }

    /// Fills `outer` except the union of `holes` (rounded by `radius`): one layer, so
    /// overlapping holes never darken or brighten twice.
    pub fn fill_outside(&self, outer: RectF, holes: &[RectF], radius: f32, color: Color) {
        let Some(geometry) = self.outside(outer, holes, radius) else {
            return;
        };
        self.set_color(color);
        // SAFETY: see above.
        unsafe { self.dc.FillGeometry(&geometry, &self.kit.brush, None) };
    }

    /// A highlighter stroke: each channel becomes the darker of the ink and what is below,
    /// so dark text stays fully readable and overlapping strokes never stack up.
    pub fn stroke_highlight(&self, points: &[(f32, f32)], width: f32, color: Color) {
        let stroke = Stroke {
            width,
            color: Color { a: 1.0, ..color },
            dash: LineDash::Solid,
        };
        // SAFETY: see above; the blend is reset right after the stroke.
        unsafe { self.dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_MIN) };
        self.stroke_polyline(points, &stroke);
        // SAFETY: see above.
        unsafe { self.dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER) };
    }

    fn effect(&self, id: &GUID) -> Option<ID2D1Effect> {
        // SAFETY: see above; `id` is a valid effect CLSID.
        unsafe { self.dc.CreateEffect(id) }.ok()
    }

    fn draw_effect(&self, effect: &ID2D1Effect, area: RectF) -> bool {
        // SAFETY: see above.
        let Ok(output) = (unsafe { effect.GetOutput() }) else {
            return false;
        };
        self.push_clip(area);
        // SAFETY: see above; the clip is popped right after.
        unsafe {
            self.dc.DrawImage(
                &output,
                None,
                None,
                D2D1_INTERPOLATION_MODE_LINEAR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );
        }
        self.pop_clip();
        true
    }

    fn outside(&self, outer: RectF, holes: &[RectF], radius: f32) -> Option<ID2D1PathGeometry> {
        let factory = &self.gpu.factory;
        let outer = outer.d2d();
        // SAFETY: plain factory calls; the geometry sink is opened, written once and closed.
        unsafe {
            let frame = factory.CreateRectangleGeometry(&raw const outer).ok()?;
            let shapes: Vec<Option<ID2D1Geometry>> = holes
                .iter()
                .filter_map(|hole| {
                    let rounded = D2D1_ROUNDED_RECT {
                        rect: hole.d2d(),
                        radiusX: radius,
                        radiusY: radius,
                    };
                    factory
                        .CreateRoundedRectangleGeometry(&raw const rounded)
                        .ok()
                })
                .map(|g| Some(g.into()))
                .collect();
            let group = factory
                .CreateGeometryGroup(D2D1_FILL_MODE_WINDING, &shapes)
                .ok()?;
            let path = factory.CreatePathGeometry().ok()?;
            let sink = path.Open().ok()?;
            frame
                .CombineWithGeometry(
                    &group,
                    D2D1_COMBINE_MODE_EXCLUDE,
                    None,
                    D2D1_DEFAULT_FLATTENING_TOLERANCE,
                    &sink,
                )
                .ok()?;
            sink.Close().ok()?;
            Some(path.into())
        }
    }
}

/// Sets a scale effect's factor and interpolation, with hard image borders.
fn scale(effect: &ID2D1Effect, factor: f32, interpolation: i32) -> bool {
    let mut factors = [0_u8; 8];
    factors[..4].copy_from_slice(&factor.to_ne_bytes());
    factors[4..].copy_from_slice(&factor.to_ne_bytes());
    let interpolation = u32::try_from(interpolation).unwrap_or_default();
    set(
        effect,
        D2D1_SCALE_PROP_SCALE.0,
        D2D1_PROPERTY_TYPE_VECTOR2,
        &factors,
    ) && set(
        effect,
        D2D1_SCALE_PROP_INTERPOLATION_MODE.0,
        D2D1_PROPERTY_TYPE_ENUM,
        &interpolation.to_ne_bytes(),
    ) && set(
        effect,
        D2D1_SCALE_PROP_BORDER_MODE.0,
        D2D1_PROPERTY_TYPE_ENUM,
        &hard_border(),
    )
}

fn hard_border() -> [u8; 4] {
    u32::try_from(D2D1_BORDER_MODE_HARD.0)
        .unwrap_or_default()
        .to_ne_bytes()
}

/// Sets property `index`; Direct2D checks that `data` has the property's size and type.
fn set(effect: &ID2D1Effect, index: i32, kind: D2D1_PROPERTY_TYPE, data: &[u8]) -> bool {
    let index = u32::try_from(index).unwrap_or_default();
    // SAFETY: `data` is read only during the call; Direct2D validates its size.
    unsafe { effect.SetValue(index, kind, data) }.is_ok()
}
