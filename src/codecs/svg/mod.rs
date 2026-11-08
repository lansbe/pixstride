//! Bounded static SVG subset implemented with Rust std only.
//!
//! Supported shapes: rect, circle, ellipse, line, polygon, polyline and paths
//! with M/L/H/V/C/Q/Z. Supports nested groups, simple fills/strokes, affine
//! transforms, viewBox and 4x4 antialiasing. This is not a general SVG engine.
//! Scripts, entities, external resources, text, CSS selectors, filters, gradients,
//! animation, unsupported path commands and unknown attributes are refused.

use crate::{Buffer, Error, Image, MAX_FILE, MAX_PIXELS};
use std::f64::consts::PI;
const MAX_SVG_BYTES: usize = 2 * 1024 * 1024;
const MAX_ELEMENTS: usize = 4096;
const MAX_DEPTH: usize = 32;
const MAX_POINTS: usize = 16_384;
const MAX_WORK: u64 = 200_000_000;
const MAX_DIMENSION: usize = 100_000;
#[derive(Clone, Copy, Default)]
pub(crate) struct Point {
    x: f64,
    y: f64,
}
#[derive(Clone, Copy)]
pub(crate) struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}
#[derive(Clone, Copy)]
pub(crate) struct Color {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Cap {
    Butt,
    Round,
    Square,
}
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Join {
    Miter,
    Round,
    Bevel,
}
#[derive(Clone, Copy)]
pub(crate) struct Style {
    fill: Option<Color>,
    stroke: Option<Color>,
    width: f64,
    fill_alpha: f64,
    stroke_alpha: f64,
    opacity: f64,
    evenodd: bool,
    cap: Cap,
    join: Join,
    miter: f64,
}
#[derive(Clone)]
pub(crate) struct State {
    matrix: Matrix,
    style: Style,
}
#[derive(Default)]
pub(crate) struct Path {
    points: Vec<Point>,
    closed: bool,
}
pub(crate) struct Numbers<'a> {
    s: &'a str,
    pos: usize,
}
pub(crate) struct Tag<'a> {
    name: &'a str,
    attributes: Vec<(&'a str, &'a str)>,
    closing: bool,
    empty: bool,
}
#[derive(Clone, Copy)]
pub(crate) struct Rect {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}
pub(crate) struct Raster {
    image: Image,
    work: u64,
}
pub(crate) struct Frame<'a> {
    name: &'a str,
    state: State,
    container: bool,
}
mod point;
pub(crate) use point::*;
mod xml_name_byte;
pub(crate) use xml_name_byte::*;
mod raster;
pub(crate) use raster::*;
mod draw;
pub(crate) use draw::*;
