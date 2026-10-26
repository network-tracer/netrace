use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{GroupId, ShapeStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AnnotationId(pub u64);

impl fmt::Display for AnnotationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Canvas annotations: they document a topology but take no part in the simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationKind {
    /// A text note.
    Note,
    /// A labelled shape outlining a zone, VLAN or site. Drawn behind devices.
    Area,
}

/// Axis-aligned rectangle on the canvas, in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const MIN_SIDE: f64 = 24.0;

    /// Sanitises the rectangle: non-finite values become 0 and sides are
    /// at least [`Rect::MIN_SIDE`], so the project always serialises cleanly.
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        let finite = |v: f64| if v.is_finite() { v } else { 0.0 };
        Rect {
            x: finite(x),
            y: finite(y),
            width: finite(width).max(Self::MIN_SIDE),
            height: finite(height).max(Self::MIN_SIDE),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: AnnotationId,
    pub kind: AnnotationKind,
    pub rect: Rect,
    pub text: String,
    /// Added in format 2; older files get the original look.
    #[serde(default)]
    pub style: ShapeStyle,
    /// Locked annotations can be selected but not moved or resized.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupId>,
}

impl Annotation {
    /// Longest text kept; longer input is truncated at a character boundary.
    pub const MAX_TEXT: usize = 2000;

    pub(crate) fn clamp_text(text: &str) -> String {
        text.chars().take(Self::MAX_TEXT).collect()
    }
}

/// Everything needed to put a removed annotation back in its stacking slot.
#[derive(Debug, Clone, PartialEq)]
pub struct RemovedAnnotation {
    pub annotation: Annotation,
    pub index: usize,
}
