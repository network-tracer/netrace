//! How things look on the canvas: colours, shapes and line patterns.
//! None of this affects the simulation.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A colour from the fixed canvas palette. Named rather than stored as RGB
/// so the app can pick shades that read well in both light and dark mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Swatch {
    Gray,
    Blue,
    Teal,
    Green,
    Yellow,
    Orange,
    Red,
    Pink,
    Purple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinePattern {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

/// Outline of an area. Every shape is fitted to the annotation's rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShapeKind {
    Rectangle,
    #[default]
    RoundedRectangle,
    Ellipse,
    /// Regular polygon with its first corner at the top.
    Polygon {
        sides: u8,
    },
}

impl ShapeKind {
    pub const MIN_SIDES: u8 = 3;
    pub const MAX_SIDES: u8 = 12;

    /// Clamps polygon sides into range.
    pub fn sanitised(self) -> Self {
        match self {
            ShapeKind::Polygon { sides } => ShapeKind::Polygon {
                sides: sides.clamp(Self::MIN_SIDES, Self::MAX_SIDES),
            },
            other => other,
        }
    }
}

/// Look of an annotation. Notes use the colours only; areas use everything.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShapeStyle {
    pub shape: ShapeKind,
    /// Border colour; `None` is the neutral default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Swatch>,
    /// Fill colour; `None` is the neutral default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<Swatch>,
    /// 0 (no fill) to 1 (opaque). Low values let overlapping shapes blend.
    pub fill_opacity: f64,
    pub line: LinePattern,
    /// Label colour; `None` picks a neutral one that contrasts with the fill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<Swatch>,
}

impl ShapeStyle {
    pub const DEFAULT_FILL_OPACITY: f64 = 0.06;

    /// Clamps values into range so the project always serialises cleanly.
    pub fn sanitised(self) -> Self {
        let opacity = if self.fill_opacity.is_finite() {
            self.fill_opacity.clamp(0.0, 1.0)
        } else {
            Self::DEFAULT_FILL_OPACITY
        };
        ShapeStyle {
            shape: self.shape.sanitised(),
            fill_opacity: opacity,
            ..self
        }
    }
}

impl Default for ShapeStyle {
    /// The original area look: neutral rounded rectangle, dashed border.
    fn default() -> Self {
        ShapeStyle {
            shape: ShapeKind::RoundedRectangle,
            stroke: None,
            fill: None,
            fill_opacity: Self::DEFAULT_FILL_OPACITY,
            line: LinePattern::Dashed,
            label: None,
        }
    }
}

/// Devices and annotations that select and move together. Ids come from the
/// project's shared counter, so a group id never collides with an item id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GroupId(pub u64);

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "group #{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygon_sides_and_opacity_are_clamped() {
        let style = ShapeStyle {
            shape: ShapeKind::Polygon { sides: 40 },
            fill_opacity: f64::NAN,
            ..ShapeStyle::default()
        }
        .sanitised();
        assert_eq!(style.shape, ShapeKind::Polygon { sides: 12 });
        assert_eq!(style.fill_opacity, ShapeStyle::DEFAULT_FILL_OPACITY);
        assert_eq!(
            ShapeKind::Polygon { sides: 1 }.sanitised(),
            ShapeKind::Polygon { sides: 3 }
        );
    }

    #[test]
    fn style_json_is_compact_and_tolerant() {
        let json = serde_json::to_value(ShapeStyle {
            shape: ShapeKind::Polygon { sides: 6 },
            fill: Some(Swatch::Teal),
            ..ShapeStyle::default()
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "shape": { "kind": "polygon", "sides": 6 },
                "fill": "teal",
                "fill_opacity": 0.06,
                "line": "dashed"
            })
        );
        // Missing fields fall back to defaults.
        let partial: ShapeStyle = serde_json::from_str(r#"{ "stroke": "red" }"#).unwrap();
        assert_eq!(partial.stroke, Some(Swatch::Red));
        assert_eq!(partial.shape, ShapeKind::RoundedRectangle);
    }
}
