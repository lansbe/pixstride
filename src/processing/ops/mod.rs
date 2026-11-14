//! Explicit, bounded RGBA8 geometry and compositing operations.
//!
//! Pixels use straight alpha. Operations run in this order: source crop,
//! resize, clockwise rotation, flips, result crop, padding, background, watermark.
use crate::{resize, Buffer, Error, Image, MAX_PIXELS};
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeMode {
    Fit,
    Fill,
    Force,
    /// Choose fill when source and target have the same orientation, fit otherwise.
    Auto,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Gravity {
    Center,
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
    /// Normalized focal coordinates in [0, 1] relative to the base image.
    Focus {
        x: f64,
        y: f64,
    },
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    R0,
    R90,
    R180,
    R270,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Lanczos3,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Padding {
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
    pub left: usize,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    /// Zero means the corresponding source dimension.
    pub width: usize,
    pub height: usize,
    pub gravity: Gravity,
}
pub struct Watermark {
    pub image: Image,
    pub gravity: Gravity,
    pub offset_x: i32,
    pub offset_y: i32,
    /// Overall opacity, 0 to 255, multiplied with the watermark alpha.
    pub opacity: u8,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TransformOptions {
    /// Zero leaves a dimension proportional; both zero mean no resize.
    pub width: usize,
    pub height: usize,
    pub mode: ResizeMode,
    pub gravity: Gravity,
    pub enlarge: bool,
    pub rotation: Rotation,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub padding: Padding,
    /// Optional background composited behind every pixel, including padding.
    pub background: Option<[u8; 4]>,
    #[serde(skip)]
    pub watermark: Option<Watermark>,
    pub method: Filter,
    pub crop: Option<Crop>,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy)]
pub(crate) enum Alignment {
    Start,
    Center,
    End,
    Focus(f64),
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy)]
pub(crate) struct ResizePlan {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) target_w: usize,
    pub(crate) target_h: usize,
    pub(crate) fill: bool,
}
mod filter;
pub(crate) use filter::*;
mod watermark;
pub(crate) use watermark::*;
