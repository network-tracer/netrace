//! Canvas styling across the bridge: colours, shapes, line patterns, groups.

use netrace_doc as doc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum LinePattern {
    Solid,
    Dashed,
    Dotted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ShapeKind {
    Rectangle,
    RoundedRectangle,
    Ellipse,
    Polygon { sides: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct ShapeStyle {
    pub shape: ShapeKind,
    pub stroke: Option<Swatch>,
    pub fill: Option<Swatch>,
    pub fill_opacity: f64,
    pub line: LinePattern,
}

/// Defaults for a new area: the original neutral, dashed rounded rectangle.
#[uniffi::export]
pub fn default_shape_style() -> ShapeStyle {
    doc::ShapeStyle::default().into()
}

/// A device or annotation, for grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum CanvasItem {
    Device { id: u64 },
    Annotation { id: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct GroupMembership {
    pub item: CanvasItem,
    pub group: Option<u64>,
}

/// A cable's colour and line pattern, as returned for undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct LinkStyle {
    pub color: Option<Swatch>,
    pub line: Option<LinePattern>,
}

macro_rules! mirror_enum {
    ($ffi:ident <=> $doc:path { $($variant:ident),* $(,)? }) => {
        impl From<$ffi> for $doc {
            fn from(value: $ffi) -> Self {
                match value { $($ffi::$variant => <$doc>::$variant),* }
            }
        }
        impl From<$doc> for $ffi {
            fn from(value: $doc) -> Self {
                match value { $(<$doc>::$variant => $ffi::$variant),* }
            }
        }
    };
}

mirror_enum!(Swatch <=> doc::Swatch { Gray, Blue, Teal, Green, Yellow, Orange, Red, Pink, Purple });
mirror_enum!(LinePattern <=> doc::LinePattern { Solid, Dashed, Dotted });

impl From<ShapeKind> for doc::ShapeKind {
    fn from(kind: ShapeKind) -> Self {
        match kind {
            ShapeKind::Rectangle => doc::ShapeKind::Rectangle,
            ShapeKind::RoundedRectangle => doc::ShapeKind::RoundedRectangle,
            ShapeKind::Ellipse => doc::ShapeKind::Ellipse,
            ShapeKind::Polygon { sides } => doc::ShapeKind::Polygon { sides },
        }
    }
}

impl From<doc::ShapeKind> for ShapeKind {
    fn from(kind: doc::ShapeKind) -> Self {
        match kind {
            doc::ShapeKind::Rectangle => ShapeKind::Rectangle,
            doc::ShapeKind::RoundedRectangle => ShapeKind::RoundedRectangle,
            doc::ShapeKind::Ellipse => ShapeKind::Ellipse,
            doc::ShapeKind::Polygon { sides } => ShapeKind::Polygon { sides },
        }
    }
}

impl From<ShapeStyle> for doc::ShapeStyle {
    fn from(s: ShapeStyle) -> Self {
        doc::ShapeStyle {
            shape: s.shape.into(),
            stroke: s.stroke.map(Into::into),
            fill: s.fill.map(Into::into),
            fill_opacity: s.fill_opacity,
            line: s.line.into(),
        }
    }
}

impl From<doc::ShapeStyle> for ShapeStyle {
    fn from(s: doc::ShapeStyle) -> Self {
        ShapeStyle {
            shape: s.shape.into(),
            stroke: s.stroke.map(Into::into),
            fill: s.fill.map(Into::into),
            fill_opacity: s.fill_opacity,
            line: s.line.into(),
        }
    }
}

impl From<CanvasItem> for doc::CanvasItem {
    fn from(item: CanvasItem) -> Self {
        match item {
            CanvasItem::Device { id } => doc::CanvasItem::Device(doc::DeviceId(id)),
            CanvasItem::Annotation { id } => doc::CanvasItem::Annotation(doc::AnnotationId(id)),
        }
    }
}

impl From<doc::CanvasItem> for CanvasItem {
    fn from(item: doc::CanvasItem) -> Self {
        match item {
            doc::CanvasItem::Device(id) => CanvasItem::Device { id: id.0 },
            doc::CanvasItem::Annotation(id) => CanvasItem::Annotation { id: id.0 },
        }
    }
}
