//! Axis-aligned rectangles (crops, effect regions).

use serde::{Deserialize, Serialize};

use super::shape::Vec2;

/// An axis-aligned rectangle given by its corners.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Area {
    /// Top-left.
    pub min: Vec2,
    /// Bottom-right.
    pub max: Vec2,
}

impl Area {
    /// The rectangle spanned by two opposite corners, in any order.
    pub fn from_corners(a: Vec2, b: Vec2) -> Self {
        Self {
            min: Vec2::new(a.x.min(b.x), a.y.min(b.y)),
            max: Vec2::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    /// Width.
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    /// Height.
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    /// Whether `p` lies inside (edges included).
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    /// Clipped to `0..size`.
    #[must_use]
    pub fn clamp_to(self, size: Vec2) -> Self {
        let clamp = |p: Vec2| Vec2::new(p.x.clamp(0.0, size.x), p.y.clamp(0.0, size.y));
        Self {
            min: clamp(self.min),
            max: clamp(self.max),
        }
    }

    /// Corners rounded to whole pixels.
    #[must_use]
    pub fn round(self) -> Self {
        let round = |p: Vec2| Vec2::new(p.x.round(), p.y.round());
        Self {
            min: round(self.min),
            max: round(self.max),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_normalises_clamps_and_rounds() {
        let area = Area::from_corners(Vec2::new(50.4, -3.0), Vec2::new(-10.0, 20.6));
        let fitted = area.clamp_to(Vec2::new(40.0, 40.0)).round();
        assert_eq!(fitted.min, Vec2::new(0.0, 0.0));
        assert_eq!(fitted.max, Vec2::new(40.0, 21.0));
        assert!(fitted.contains(Vec2::new(40.0, 0.0)));
        assert!(!fitted.contains(Vec2::new(41.0, 0.0)));
    }
}
