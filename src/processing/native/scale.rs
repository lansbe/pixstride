//! Calculate shrink, effective DPR and target size before allocating pixels.
use crate::{ops::ResizeMode, request::Request, Error};

pub(super) struct Plan {
    pub scale: [f64; 2],
    pub target: [i32; 2],
    pub dpr: f64,
}
fn rounded(n: f64) -> Result<i32, Error> {
    if !n.is_finite() || !(0.0..=100_000.0).contains(&n) {
        return Err("native scale exceeds dimension limit");
    }
    Ok(n.round() as i32)
}
impl Plan {
    pub fn new(width: i32, height: i32, r: &Request, vector: bool) -> Result<Self, Error> {
        let o = &r.options;
        let e = &r.extra;
        let src = [f64::from(width), f64::from(height)];
        let dst = [
            if o.width == 0 { src[0] } else { o.width as f64 },
            if o.height == 0 {
                src[1]
            } else {
                o.height as f64
            },
        ];
        let mut shrink = [src[0] / dst[0], src[1] / dst[1]];
        let fill = o.mode == ResizeMode::Fill
            || (o.mode == ResizeMode::Auto && (src[0] >= src[1]) == (dst[0] >= dst[1]));
        if o.mode != ResizeMode::Force {
            let uniform = if o.width == 0 {
                shrink[1]
            } else if o.height == 0 {
                shrink[0]
            } else if fill {
                shrink[0].min(shrink[1])
            } else {
                shrink[0].max(shrink[1])
            };
            shrink = [uniform; 2];
        }
        for (s, z) in shrink.iter_mut().zip(e.zoom) {
            *s /= z;
        }
        let mut dpr = e.dpr;
        if !o.enlarge && !vector {
            let min = shrink[0].min(shrink[1]);
            if min < 1.0 {
                shrink = shrink.map(|n| n / min);
                if !e.extend {
                    dpr /= min;
                }
            }
            dpr = dpr.min(shrink[0].min(shrink[1]));
        }
        for axis in 0..2 {
            if e.min[axis] > 0 {
                let limit = src[axis] / e.min[axis] as f64;
                if limit < shrink[axis] {
                    let factor = limit / shrink[axis];
                    shrink = shrink.map(|n| n * factor);
                }
            }
        }
        let scale = [
            1.0 / (shrink[0] / dpr).min(src[0]),
            1.0 / (shrink[1] / dpr).min(src[1]),
        ];
        let target = [
            rounded(o.width as f64 * dpr * e.zoom[0])?,
            rounded(o.height as f64 * dpr * e.zoom[1])?,
        ];
        let mut plan = Self { scale, target, dpr };
        let mut out = [
            rounded(src[0] * plan.scale[0])?.max(1),
            rounded(src[1] * plan.scale[1])?.max(1),
        ];
        for (axis, value) in out.iter_mut().enumerate() {
            if target[axis] > 0 {
                *value = (*value).min(target[axis]);
            }
        }
        if e.extend {
            for (value, target) in out.iter_mut().zip(target) {
                *value = (*value).max(target);
            }
        } else if e.extend_aspect && target[0] > 0 && target[1] > 0 {
            let ratio = f64::from(target[0]) / f64::from(target[1]);
            if f64::from(out[0]) / f64::from(out[1]) < ratio {
                out[0] = rounded(f64::from(out[1]) * ratio)?;
            } else {
                out[1] = rounded(f64::from(out[0]) / ratio)?;
            }
        }
        out[0] += rounded(o.padding.left as f64 * dpr)? + rounded(o.padding.right as f64 * dpr)?;
        out[1] += rounded(o.padding.top as f64 * dpr)? + rounded(o.padding.bottom as f64 * dpr)?;
        let largest = out[0].max(out[1]);
        if largest as usize > e.max_result_dimension {
            let factor = e.max_result_dimension as f64 / f64::from(largest);
            plan.dpr *= factor;
            for (s, n) in plan.scale.iter_mut().zip(src) {
                *s = (*s * factor).max(1.0 / n);
            }
            plan.target = [
                rounded(o.width as f64 * plan.dpr * e.zoom[0])?,
                rounded(o.height as f64 * plan.dpr * e.zoom[1])?,
            ];
        }
        Ok(plan)
    }
}
