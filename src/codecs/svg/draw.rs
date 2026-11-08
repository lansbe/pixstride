use super::*;
pub(crate) fn draw(raster: &mut Raster, paths: &[Path], state: &State) -> Result<(), Error> {
    if paths.is_empty() || (state.style.fill.is_none() && state.style.stroke.is_none()) {
        return Ok(());
    }
    if state.style.opacity < 1.
        && state.style.fill.is_some()
        && state.style.stroke.is_some()
        && state.style.width > 0.
    {
        return Err("SVG: combined fill/stroke opacity unsupported");
    }
    let mut minx = f64::INFINITY;
    let mut miny = f64::INFINITY;
    let mut maxx = f64::NEG_INFINITY;
    let mut maxy = f64::NEG_INFINITY;
    for p in paths.iter().flat_map(|path| path.points.iter()) {
        let p = state.matrix.point(*p);
        if !p.x.is_finite() || !p.y.is_finite() {
            return Err("SVG: invalid transformed point");
        }
        minx = minx.min(p.x);
        miny = miny.min(p.y);
        maxx = maxx.max(p.x);
        maxy = maxy.max(p.y);
    }
    let pad = if state.style.stroke.is_some() {
        state.style.width * 0.5 * state.matrix.scale_bound() * state.style.miter.max(1.) + 1.
    } else {
        1.
    };
    let Some(bounds) = raster.rect(minx - pad, miny - pad, maxx + pad, maxy + pad)? else {
        return Ok(());
    };
    let area = bounds
        .width()
        .checked_mul(bounds.height())
        .ok_or("SVG: dimensions overflow")?;
    // Bound both the temporary mask and total work before allocation.
    if area > (MAX_WORK / 16) as usize {
        return Err("SVG: work limit");
    }
    let mut mask = zero_mask(area)?;
    if let Some(fill) = state.style.fill {
        fill_paths(
            raster,
            bounds,
            &mut mask,
            paths,
            state.matrix,
            state.style.evenodd,
        )?;
        raster.blend(
            bounds,
            &mask,
            fill,
            state.style.fill_alpha * state.style.opacity,
        );
        mask.fill(0);
    }
    if let Some(stroke) = state.style.stroke {
        stroke_paths(raster, bounds, &mut mask, paths, state.matrix, state.style)?;
        raster.blend(
            bounds,
            &mask,
            stroke,
            state.style.stroke_alpha * state.style.opacity,
        );
    }
    Ok(())
}
pub(crate) fn validate_declaration(declaration: &str) -> Result<(), Error> {
    let bytes = declaration.as_bytes();
    let mut pos = 0usize;
    let mut stage = 0u8;
    while pos < bytes.len() {
        let start_space = pos;
        while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
            pos += 1;
        }
        if pos == bytes.len() {
            break;
        }
        if start_space == pos {
            return Err("SVG: invalid XML declaration spacing");
        }
        let start = pos;
        while bytes.get(pos).is_some_and(u8::is_ascii_alphabetic) {
            pos += 1;
        }
        if start == pos {
            return Err("SVG: invalid XML declaration");
        }
        let name = &declaration[start..pos];
        while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
            pos += 1;
        }
        if bytes.get(pos) != Some(&b'=') {
            return Err("SVG: invalid XML declaration");
        }
        pos += 1;
        while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
            pos += 1;
        }
        let quote = *bytes.get(pos).ok_or("SVG: invalid XML declaration")?;
        if quote != b'\'' && quote != b'"' {
            return Err("SVG: invalid XML declaration");
        }
        pos += 1;
        let start = pos;
        while bytes.get(pos).is_some_and(|&c| c != quote) {
            pos += 1;
        }
        if pos == bytes.len() {
            return Err("SVG: invalid XML declaration");
        }
        let value = &declaration[start..pos];
        pos += 1;
        match name {
            "version" if stage == 0 && (value == "1.0" || value == "1.1") => stage = 1,
            "encoding" if stage == 1 && value.eq_ignore_ascii_case("utf-8") => stage = 2,
            "standalone" if (stage == 1 || stage == 2) && (value == "yes" || value == "no") => {
                stage = 3
            }
            _ => return Err("SVG: unsupported XML declaration"),
        }
    }
    if stage == 0 {
        return Err("SVG: XML version required");
    }
    Ok(())
}
pub(crate) fn content_start(xml: &str) -> Result<usize, Error> {
    let mut at = 0usize;
    if xml.starts_with('\u{feff}') {
        at = '\u{feff}'.len_utf8();
    }
    while xml.as_bytes().get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    if xml[at..].starts_with("<?xml") {
        if !xml
            .as_bytes()
            .get(at + 5)
            .is_some_and(u8::is_ascii_whitespace)
        {
            return Err("SVG: invalid XML declaration");
        }
        let end = xml[at..]
            .find("?>")
            .ok_or("SVG: unterminated XML declaration")?
            + at;
        if end - at > 512 {
            return Err("SVG: XML declaration too long");
        }
        validate_declaration(&xml[at + 5..end])?;
        at = end + 2;
    }
    Ok(at)
}
/// Recognize the supported standalone SVG root without accepting generic XML.
pub(crate) fn looks_like(data: &[u8]) -> bool {
    if data.len() > MAX_SVG_BYTES {
        return false;
    }
    let Ok(xml) = std::str::from_utf8(data) else {
        return false;
    };
    let Ok(mut at) = content_start(xml) else {
        return false;
    };
    while let Some(next) = xml.get(at..) {
        at += next.len()
            - next
                .trim_start_matches(|c: char| c.is_ascii_whitespace())
                .len();
        if !xml[at..].starts_with("<!--") {
            break;
        }
        let Some(end) = xml[at + 4..].find("-->") else {
            return false;
        };
        at += 4 + end + 3;
    }
    xml[at..].starts_with("<svg")
        && matches!(
            xml.as_bytes().get(at + 4),
            Some(b'>' | b'/' | b' ' | b'\t' | b'\r' | b'\n')
        )
}
/// Decode a supported static SVG into an RGBA8 image.
pub(crate) fn decode(data: &[u8]) -> Result<Image, Error> {
    if data.len() > MAX_SVG_BYTES {
        return Err("SVG: source exceeds limit");
    }
    if data.contains(&b'&') {
        return Err("SVG: entities are unsupported");
    }
    let xml = std::str::from_utf8(data).map_err(|_| "SVG: UTF-8 required")?;
    let mut position = content_start(xml)?;
    let mut stack: Vec<Frame<'_>> = Vec::new();
    let mut raster: Option<Raster> = None;
    let mut seen_root = false;
    let mut closed_root = false;
    let mut elements = 0usize;
    while let Some(tag) = next_tag(xml, &mut position)? {
        if tag.closing {
            let frame = stack.pop().ok_or("SVG: unexpected closing tag")?;
            if frame.name != tag.name {
                return Err("SVG: mismatched closing tag");
            }
            if stack.is_empty() {
                closed_root = true;
            }
            continue;
        }
        if closed_root {
            return Err("SVG: multiple roots");
        }
        elements += 1;
        if elements > MAX_ELEMENTS {
            return Err("SVG: element limit");
        }
        if tag
            .attributes
            .iter()
            .any(|(name, _)| !allowed_attribute(tag.name, name))
        {
            return Err("SVG: unsupported attribute or element");
        }
        if !seen_root {
            if tag.name != "svg" {
                return Err("SVG: root must be svg");
            }
            seen_root = true;
            if attr(&tag.attributes, "xmlns").is_some_and(|v| v != "http://www.w3.org/2000/svg") {
                return Err("SVG: unsupported namespace");
            }
            if attr(&tag.attributes, "version")
                .is_some_and(|v| v != "1.1" && v != "2.0" && v != "2")
            {
                return Err("SVG: unsupported version");
            }
            if attr(&tag.attributes, "transform").is_some() {
                return Err("SVG: root transform unsupported");
            }
            let dimension = |name: &str, default: usize| -> Result<usize, Error> {
                let Some(value) = attr(&tag.attributes, name) else {
                    return Ok(default);
                };
                let n = one_number(value)?;
                if n <= 0. || n.fract() != 0. || n > MAX_DIMENSION as f64 {
                    return Err("SVG: invalid viewport dimension");
                }
                Ok(n as usize)
            };
            let width = dimension("width", 300)?;
            let height = dimension("height", 150)?;
            let mut matrix = Matrix::identity();
            let aspect = attr(&tag.attributes, "preserveAspectRatio")
                .unwrap_or("xMidYMid meet")
                .trim();
            if !matches!(aspect, "none" | "xMidYMid" | "xMidYMid meet") {
                return Err("SVG: unsupported preserveAspectRatio");
            }
            if let Some(box_value) = attr(&tag.attributes, "viewBox") {
                let n = number_list(box_value, 4)?;
                if n.len() != 4 || n[2] <= 0. || n[3] <= 0. {
                    return Err("SVG: invalid viewBox");
                }
                let sx = width as f64 / n[2];
                let sy = height as f64 / n[3];
                if aspect == "none" {
                    matrix.a = sx;
                    matrix.d = sy;
                    matrix.e = -n[0] * sx;
                    matrix.f = -n[1] * sy;
                } else {
                    let scale = sx.min(sy);
                    matrix.a = scale;
                    matrix.d = scale;
                    matrix.e = (width as f64 - n[2] * scale) / 2. - n[0] * scale;
                    matrix.f = (height as f64 - n[3] * scale) / 2. - n[1] * scale;
                }
            }
            let state = state_for(
                &State {
                    matrix,
                    style: Style::default(),
                },
                &tag,
            )?;
            if state.style.opacity != 1. {
                return Err("SVG: group opacity unsupported");
            }
            raster = Some(Raster::new(width, height)?);
            if tag.empty {
                closed_root = true;
            } else {
                push(
                    &mut stack,
                    Frame {
                        name: tag.name,
                        state,
                        container: true,
                    },
                )?;
            }
            continue;
        }
        let parent = stack.last().ok_or("SVG: missing root")?;
        if !parent.container {
            return Err("SVG: shape children unsupported");
        }
        let state = state_for(&parent.state, &tag)?;
        let container = tag.name == "g";
        if container {
            if state.style.opacity != 1. {
                return Err("SVG: group opacity unsupported");
            }
        } else {
            let paths = geometry(&tag, state.matrix.scale_bound())?;
            draw(raster.as_mut().ok_or("SVG: missing root")?, &paths, &state)?;
        }
        if !tag.empty {
            if stack.len() >= MAX_DEPTH {
                return Err("SVG: nesting limit");
            }
            push(
                &mut stack,
                Frame {
                    name: tag.name,
                    state,
                    container,
                },
            )?;
        }
    }
    if !seen_root || !closed_root || !stack.is_empty() {
        return Err("SVG: incomplete document");
    }
    Ok(raster.ok_or("SVG: missing root")?.image)
}
