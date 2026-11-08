use super::*;
impl Raster {
    pub(crate) fn new(width: usize, height: usize) -> Result<Self, Error> {
        let pixels = width
            .checked_mul(height)
            .ok_or("SVG: dimensions overflow")?;
        let bytes = pixels.checked_mul(4).ok_or("SVG: dimensions overflow")?;
        if width == 0
            || height == 0
            || width > MAX_DIMENSION
            || height > MAX_DIMENSION
            || pixels > MAX_PIXELS
            || bytes > MAX_FILE
        {
            return Err("SVG: dimensions outside limit");
        }
        Ok(Self {
            image: Image {
                width,
                height,
                pixels: Buffer::new(bytes)?,
            },
            work: 0,
        })
    }
    pub(crate) fn consume(&mut self, n: u64) -> Result<(), Error> {
        self.work = self.work.checked_add(n).ok_or("SVG: work limit")?;
        if self.work > MAX_WORK {
            return Err("SVG: work limit");
        }
        Ok(())
    }
    pub(crate) fn rect(
        &self,
        minx: f64,
        miny: f64,
        maxx: f64,
        maxy: f64,
    ) -> Result<Option<Rect>, Error> {
        if [minx, miny, maxx, maxy]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1e15)
        {
            return Err("SVG: bounds outside limit");
        }
        let x0 = minx.floor().max(0.).min(self.image.width as f64) as usize;
        let y0 = miny.floor().max(0.).min(self.image.height as f64) as usize;
        let x1 = maxx.ceil().max(0.).min(self.image.width as f64) as usize;
        let y1 = maxy.ceil().max(0.).min(self.image.height as f64) as usize;
        if x0 >= x1 || y0 >= y1 {
            Ok(None)
        } else {
            Ok(Some(Rect { x0, y0, x1, y1 }))
        }
    }
    pub(crate) fn blend(&mut self, bounds: Rect, mask: &[u16], color: Color, opacity: f64) {
        let image_width = self.image.width;
        let output = self.image.pixels.as_mut_slice();
        for y in bounds.y0..bounds.y1 {
            for x in bounds.x0..bounds.x1 {
                let coverage = mask[(y - bounds.y0) * bounds.width() + x - bounds.x0].count_ones();
                if coverage == 0 {
                    continue;
                }
                let source_a = (f64::from(color.a) * opacity * f64::from(coverage) / 16.)
                    .round()
                    .clamp(0., 255.) as u32;
                if source_a == 0 {
                    continue;
                }
                let at = (y * image_width + x) * 4;
                let dst_a = u32::from(output[at + 3]);
                let remaining = 255 - source_a;
                let dest_part = (dst_a * remaining + 127) / 255;
                let out_a = source_a + dest_part;
                for (index, value) in [color.r, color.g, color.b].into_iter().enumerate() {
                    let dest = u32::from(output[at + index]);
                    let premult =
                        u32::from(value) * source_a + (dest * dst_a * remaining + 127) / 255;
                    output[at + index] = ((premult + out_a / 2) / out_a).min(255) as u8;
                }
                output[at + 3] = out_a as u8;
            }
        }
    }
}
pub(crate) fn zero_mask(size: usize) -> Result<Vec<u16>, Error> {
    let mut mask = Vec::new();
    mask.try_reserve_exact(size)
        .map_err(|_| "SVG: allocation failed")?;
    mask.resize(size, 0);
    Ok(mask)
}
pub(crate) fn mask_sample(mask: &mut [u16], bounds: Rect, sx: usize, sy: usize) {
    let x = sx / 4;
    let y = sy / 4;
    let index = (y - bounds.y0) * bounds.width() + x - bounds.x0;
    mask[index] |= 1u16 << ((sy % 4) * 4 + sx % 4);
}
pub(crate) fn fill_paths(
    raster: &mut Raster,
    bounds: Rect,
    mask: &mut [u16],
    paths: &[Path],
    matrix: Matrix,
    evenodd: bool,
) -> Result<(), Error> {
    let edges = paths
        .iter()
        .map(|p| {
            if p.points.len() > 2 {
                p.points.len()
            } else {
                0
            }
        })
        .sum::<usize>();
    if edges == 0 {
        return Ok(());
    }
    let area = bounds
        .width()
        .checked_mul(bounds.height())
        .ok_or("SVG: size overflow")? as u64;
    // Each scanline also sorts up to `edges` intersections. Account for a
    // conservative n*log2(n) sort estimate, in addition to visiting edges;
    // this is a work budget rather than a wall-clock time guarantee.
    let sort_factor = u64::from(usize::BITS - (edges - 1).leading_zeros()) + 1;
    raster.consume(
        area.saturating_mul(16).saturating_add(
            (bounds.height() as u64)
                .saturating_mul(4)
                .saturating_mul(edges as u64)
                .saturating_mul(sort_factor),
        ),
    )?;
    let mut intersections: Vec<(f64, i32)> = Vec::new();
    intersections
        .try_reserve_exact(edges)
        .map_err(|_| "SVG: allocation failed")?;
    for sy in bounds.y0 * 4..bounds.y1 * 4 {
        let y = (sy as f64 + 0.5) / 4.;
        intersections.clear();
        for path in paths {
            if path.points.len() < 3 {
                continue;
            }
            for i in 0..path.points.len() {
                let a = matrix.point(path.points[i]);
                let b = matrix.point(path.points[(i + 1) % path.points.len()]);
                let direction = if a.y <= y && y < b.y {
                    1
                } else if b.y <= y && y < a.y {
                    -1
                } else {
                    continue;
                };
                let x = a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y);
                if !x.is_finite() {
                    return Err("SVG: invalid intersection");
                }
                intersections.push((x, direction));
            }
        }
        intersections.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut winding = 0i32;
        let mut left: Option<f64> = None;
        for &(x, direction) in &intersections {
            if let Some(from) = left {
                if if evenodd {
                    winding & 1 != 0
                } else {
                    winding != 0
                } {
                    let start = ((from * 4. - 0.5).ceil() as i64)
                        .max((bounds.x0 * 4) as i64)
                        .min((bounds.x1 * 4) as i64);
                    let end = ((x * 4. - 0.5).ceil() as i64)
                        .max((bounds.x0 * 4) as i64)
                        .min((bounds.x1 * 4) as i64);
                    for sx in start..end {
                        mask_sample(mask, bounds, sx as usize, sy);
                    }
                }
            }
            winding += direction;
            left = Some(x);
        }
    }
    Ok(())
}
pub(crate) fn in_triangle(p: Point, a: Point, b: Point, c: Point) -> bool {
    let ab = b.sub(a).cross(p.sub(a));
    let bc = c.sub(b).cross(p.sub(b));
    let ca = a.sub(c).cross(p.sub(c));
    (ab >= -1e-10 && bc >= -1e-10 && ca >= -1e-10) || (ab <= 1e-10 && bc <= 1e-10 && ca <= 1e-10)
}
pub(crate) fn mark_region<F: Fn(Point) -> bool>(
    raster: &mut Raster,
    bounds: Rect,
    mask: &mut [u16],
    matrix: Matrix,
    inverse: Matrix,
    points: &[Point],
    padding: f64,
    inside: F,
) -> Result<(), Error> {
    let mut minx = f64::INFINITY;
    let mut miny = f64::INFINITY;
    let mut maxx = f64::NEG_INFINITY;
    let mut maxy = f64::NEG_INFINITY;
    for &p in points {
        let p = matrix.point(p);
        minx = minx.min(p.x);
        miny = miny.min(p.y);
        maxx = maxx.max(p.x);
        maxy = maxy.max(p.y);
    }
    let Some(mut r) = raster.rect(
        minx - padding,
        miny - padding,
        maxx + padding,
        maxy + padding,
    )?
    else {
        return Ok(());
    };
    r.x0 = r.x0.max(bounds.x0);
    r.y0 = r.y0.max(bounds.y0);
    r.x1 = r.x1.min(bounds.x1);
    r.y1 = r.y1.min(bounds.y1);
    if r.x0 >= r.x1 || r.y0 >= r.y1 {
        return Ok(());
    }
    raster.consume(
        (r.width() as u64)
            .saturating_mul(r.height() as u64)
            .saturating_mul(16),
    )?;
    for sy in r.y0 * 4..r.y1 * 4 {
        for sx in r.x0 * 4..r.x1 * 4 {
            let p = inverse.point(Point::new((sx as f64 + 0.5) / 4., (sy as f64 + 0.5) / 4.));
            if inside(p) {
                mask_sample(mask, bounds, sx, sy);
            }
        }
    }
    Ok(())
}
pub(crate) fn stroke_paths(
    raster: &mut Raster,
    bounds: Rect,
    mask: &mut [u16],
    paths: &[Path],
    matrix: Matrix,
    style: Style,
) -> Result<(), Error> {
    if style.width == 0. {
        return Ok(());
    }
    let inverse = matrix.inverse()?;
    let radius = style.width / 2.;
    let padding = radius * matrix.scale_bound() + 1.;
    let r2 = radius * radius;
    for path in paths {
        let n = path.points.len();
        if n < 2 {
            continue;
        }
        let segments = if path.closed { n } else { n - 1 };
        for i in 0..segments {
            let a = path.points[i];
            let b = path.points[(i + 1) % n];
            let vector = b.sub(a);
            let len2 = vector.x * vector.x + vector.y * vector.y;
            if len2 <= 1e-24 {
                if !path.closed && style.cap == Cap::Round {
                    mark_region(raster, bounds, mask, matrix, inverse, &[a], padding, |p| {
                        let d = p.sub(a);
                        d.x * d.x + d.y * d.y <= r2
                    })?;
                } else if !path.closed && style.cap == Cap::Square {
                    mark_region(raster, bounds, mask, matrix, inverse, &[a], padding, |p| {
                        let d = p.sub(a);
                        d.x.abs() <= radius && d.y.abs() <= radius
                    })?;
                }
                continue;
            }
            let len = len2.sqrt();
            let first = !path.closed && i == 0;
            let last = !path.closed && i + 1 == segments;
            let min_t = if first && style.cap == Cap::Square {
                -radius / len
            } else {
                0.
            };
            let max_t = if last && style.cap == Cap::Square {
                1. + radius / len
            } else {
                1.
            };
            mark_region(
                raster,
                bounds,
                mask,
                matrix,
                inverse,
                &[a, b],
                padding,
                |p| {
                    let ap = p.sub(a);
                    let t = (ap.x * vector.x + ap.y * vector.y) / len2;
                    if t < min_t || t > max_t {
                        return false;
                    }
                    let closest = a.add(vector.mul(t));
                    p.sub(closest).length() <= radius
                },
            )?;
            if style.cap == Cap::Round {
                for endpoint in [first.then_some(a), last.then_some(b)]
                    .into_iter()
                    .flatten()
                {
                    mark_region(
                        raster,
                        bounds,
                        mask,
                        matrix,
                        inverse,
                        &[endpoint],
                        padding,
                        |p| {
                            let q = p.sub(endpoint);
                            q.x * q.x + q.y * q.y <= r2
                        },
                    )?;
                }
            }
        }
        let first = if path.closed { 0 } else { 1 };
        let last = if path.closed { n } else { n - 1 };
        for i in first..last {
            let p = path.points[i];
            let prev = path.points[(i + n - 1) % n];
            let next = path.points[(i + 1) % n];
            let u = p.sub(prev);
            let v = next.sub(p);
            let lu = u.length();
            let lv = v.length();
            if lu <= 1e-12 || lv <= 1e-12 {
                continue;
            }
            let u = u.mul(1. / lu);
            let v = v.mul(1. / lv);
            let cross = u.cross(v);
            if cross.abs() < 1e-12 {
                continue;
            }
            if style.join == Join::Round {
                mark_region(raster, bounds, mask, matrix, inverse, &[p], padding, |q| {
                    let d = q.sub(p);
                    d.x * d.x + d.y * d.y <= r2
                })?;
                continue;
            }
            let side = if cross > 0. { -1. } else { 1. };
            let q1 = p.add(Point::new(-u.y, u.x).mul(side * radius));
            let q2 = p.add(Point::new(-v.y, v.x).mul(side * radius));
            let t = q2.sub(q1).cross(v) / cross;
            let miter = q1.add(u.mul(t));
            let use_miter =
                style.join == Join::Miter && miter.sub(p).length() <= style.miter * radius;
            let points = [p, q1, q2, miter];
            mark_region(
                raster,
                bounds,
                mask,
                matrix,
                inverse,
                &points[..if use_miter { 4 } else { 3 }],
                1.,
                |q| in_triangle(q, p, q1, q2) || (use_miter && in_triangle(q, q1, miter, q2)),
            )?;
        }
    }
    Ok(())
}
