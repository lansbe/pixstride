use super::*;
pub(crate) fn xml_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
}
pub(crate) fn next_tag<'a>(xml: &'a str, position: &mut usize) -> Result<Option<Tag<'a>>, Error> {
    let b = xml.as_bytes();
    loop {
        let start = *position;
        while *position < b.len() && b[*position] != b'<' {
            *position += 1;
        }
        if !xml[start..*position].trim().is_empty() {
            return Err("SVG: text is unsupported");
        }
        if *position == b.len() {
            return Ok(None);
        }
        if xml[*position..].starts_with("<!--") {
            let start = *position + 4;
            let end = xml[start..]
                .find("-->")
                .ok_or("SVG: unterminated comment")?
                + start;
            if xml[start..end].contains("--") {
                return Err("SVG: invalid comment");
            }
            *position = end + 3;
            continue;
        }
        *position += 1;
        if matches!(b.get(*position), Some(b'!' | b'?')) {
            return Err("SVG: declarations are unsupported");
        }
        let closing = b.get(*position) == Some(&b'/');
        if closing {
            *position += 1;
        }
        let start = *position;
        while b.get(*position).is_some_and(|&c| xml_name_byte(c)) {
            *position += 1;
        }
        if start == *position || !b[start].is_ascii_alphabetic() {
            return Err("SVG: invalid tag name");
        }
        let name = &xml[start..*position];
        let mut attributes = Vec::new();
        loop {
            while b.get(*position).is_some_and(u8::is_ascii_whitespace) {
                *position += 1;
            }
            if b.get(*position) == Some(&b'>') {
                *position += 1;
                return Ok(Some(Tag {
                    name,
                    attributes,
                    closing,
                    empty: false,
                }));
            }
            if b.get(*position..*position + 2) == Some(b"/>") {
                if closing {
                    return Err("SVG: invalid closing tag");
                }
                *position += 2;
                return Ok(Some(Tag {
                    name,
                    attributes,
                    closing,
                    empty: true,
                }));
            }
            if closing || attributes.len() == 64 {
                return Err("SVG: invalid or excessive attributes");
            }
            let start = *position;
            while b.get(*position).is_some_and(|&c| xml_name_byte(c)) {
                *position += 1;
            }
            if start == *position || !b[start].is_ascii_alphabetic() {
                return Err("SVG: invalid attribute");
            }
            let key = &xml[start..*position];
            if attributes.iter().any(|(name, _)| *name == key) {
                return Err("SVG: duplicate attribute");
            }
            while b.get(*position).is_some_and(u8::is_ascii_whitespace) {
                *position += 1;
            }
            if b.get(*position) != Some(&b'=') {
                return Err("SVG: invalid attribute assignment");
            }
            *position += 1;
            while b.get(*position).is_some_and(u8::is_ascii_whitespace) {
                *position += 1;
            }
            let quote = *b.get(*position).ok_or("SVG: truncated attribute")?;
            if quote != b'\'' && quote != b'"' {
                return Err("SVG: unquoted attribute");
            }
            *position += 1;
            let start = *position;
            while b.get(*position).is_some_and(|&c| c != quote) {
                if b[*position] == b'<' {
                    return Err("SVG: invalid attribute value");
                }
                *position += 1;
            }
            if *position == b.len() {
                return Err("SVG: truncated attribute");
            }
            let value = &xml[start..*position];
            *position += 1;
            if !b
                .get(*position)
                .is_some_and(|c| c.is_ascii_whitespace() || *c == b'/' || *c == b'>')
            {
                return Err("SVG: missing whitespace between attributes");
            }
            push(&mut attributes, (key, value))?;
        }
    }
}
pub(crate) fn attr<'a>(attributes: &[(&str, &'a str)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find_map(|(key, value)| (*key == name).then_some(*value))
}
pub(crate) fn property_name(name: &str) -> bool {
    matches!(
        name,
        "fill"
            | "stroke"
            | "stroke-width"
            | "fill-opacity"
            | "stroke-opacity"
            | "opacity"
            | "fill-rule"
            | "stroke-linecap"
            | "stroke-linejoin"
            | "stroke-miterlimit"
    )
}
pub(crate) fn state_for(parent: &State, tag: &Tag<'_>) -> Result<State, Error> {
    let mut state = parent.clone();
    state.style.opacity = 1.; // opacity is not inherited; groups with it are refused below.
    for &(name, value) in &tag.attributes {
        if property_name(name) {
            property(&mut state.style, name, value)?;
        }
    }
    if let Some(style) = attr(&tag.attributes, "style") {
        for assignment in style.split(';') {
            if assignment.trim().is_empty() {
                continue;
            }
            let (name, value) = assignment.split_once(':').ok_or("SVG: invalid style")?;
            property(&mut state.style, name, value)?;
        }
    }
    if let Some(value) = attr(&tag.attributes, "transform") {
        state.matrix = state.matrix.then(transform(value)?);
    }
    Ok(state)
}
pub(crate) fn allowed_attribute(tag: &str, name: &str) -> bool {
    if property_name(name) || matches!(name, "id" | "style" | "transform") {
        return true;
    }
    match tag {
        "svg" => matches!(
            name,
            "xmlns" | "version" | "width" | "height" | "viewBox" | "preserveAspectRatio"
        ),
        "g" => false,
        "rect" => matches!(name, "x" | "y" | "width" | "height"),
        "circle" => matches!(name, "cx" | "cy" | "r"),
        "ellipse" => matches!(name, "cx" | "cy" | "rx" | "ry"),
        "line" => matches!(name, "x1" | "y1" | "x2" | "y2"),
        "polygon" | "polyline" => name == "points",
        "path" => name == "d",
        _ => false,
    }
}
pub(crate) fn coord(tag: &Tag<'_>, name: &str, default: f64) -> Result<f64, Error> {
    attr(&tag.attributes, name).map_or(Ok(default), one_number)
}
pub(crate) fn path_data(data: &str, scale: f64) -> Result<Vec<Path>, Error> {
    let mut parser = Numbers::new(data);
    let mut paths: Vec<Path> = Vec::new();
    let mut current = Point::default();
    let mut start = current;
    let mut command = 0u8;
    let mut count = 0usize;
    while !parser.end() {
        if parser.s.as_bytes()[parser.pos].is_ascii_alphabetic() {
            command = parser.s.as_bytes()[parser.pos];
            parser.pos += 1;
            if !matches!(
                command,
                b'M' | b'm'
                    | b'L'
                    | b'l'
                    | b'H'
                    | b'h'
                    | b'V'
                    | b'v'
                    | b'C'
                    | b'c'
                    | b'Q'
                    | b'q'
                    | b'Z'
                    | b'z'
            ) {
                return Err("SVG: unsupported path command");
            }
            if matches!(command, b'Z' | b'z') {
                let path = paths.last_mut().ok_or("SVG: close without path")?;
                if path.closed {
                    return Err("SVG: duplicate path close");
                }
                path.closed = true;
                current = start;
                command = 0;
                continue;
            }
        }
        let relative = command.is_ascii_lowercase();
        match command.to_ascii_uppercase() {
            b'M' => {
                let mut p = parser.pair()?;
                if relative {
                    p = p.add(current);
                }
                let mut path = Path::default();
                point(&mut path, p, &mut count)?;
                push(&mut paths, path)?;
                current = p;
                start = p;
                command = if relative { b'l' } else { b'L' };
            }
            b'L' | b'H' | b'V' => {
                let path = paths.last_mut().ok_or("SVG: path must begin with M")?;
                if path.closed {
                    return Err("SVG: new move required after close");
                }
                let mut p = match command.to_ascii_uppercase() {
                    b'L' => parser.pair()?,
                    b'H' => Point::new(parser.number()?, if relative { 0. } else { current.y }),
                    _ => Point::new(if relative { 0. } else { current.x }, parser.number()?),
                };
                if relative {
                    p = p.add(current);
                }
                point(path, p, &mut count)?;
                current = p;
            }
            b'C' | b'Q' => {
                let path = paths.last_mut().ok_or("SVG: path must begin with M")?;
                if path.closed {
                    return Err("SVG: new move required after close");
                }
                let mut c1 = parser.pair()?;
                let mut c2 = if command.eq_ignore_ascii_case(&b'C') {
                    parser.pair()?
                } else {
                    c1
                };
                let mut end = parser.pair()?;
                if relative {
                    c1 = c1.add(current);
                    c2 = c2.add(current);
                    end = end.add(current);
                }
                let length = current.sub(c1).length() + c1.sub(c2).length() + c2.sub(end).length();
                let required = (length * scale / 4.).ceil().max(4.);
                if required > 128. {
                    return Err("SVG: curve tessellation limit");
                }
                let steps = required as usize;
                for i in 1..=steps {
                    let t = i as f64 / steps as f64;
                    let u = 1. - t;
                    let p = if command.eq_ignore_ascii_case(&b'C') {
                        current
                            .mul(u * u * u)
                            .add(c1.mul(3. * u * u * t))
                            .add(c2.mul(3. * u * t * t))
                            .add(end.mul(t * t * t))
                    } else {
                        current
                            .mul(u * u)
                            .add(c1.mul(2. * u * t))
                            .add(end.mul(t * t))
                    };
                    point(path, p, &mut count)?;
                }
                current = end;
            }
            _ => return Err("SVG: missing path command"),
        }
    }
    Ok(paths)
}
pub(crate) fn geometry(tag: &Tag<'_>, scale: f64) -> Result<Vec<Path>, Error> {
    let mut paths = Vec::new();
    let mut path = Path::default();
    let mut count = 0usize;
    match tag.name {
        "rect" => {
            let x = coord(tag, "x", 0.)?;
            let y = coord(tag, "y", 0.)?;
            let w = coord(tag, "width", 0.)?;
            let h = coord(tag, "height", 0.)?;
            if w < 0. || h < 0. {
                return Err("SVG: invalid rectangle size");
            }
            if w == 0. || h == 0. {
                return Ok(paths);
            }
            for p in [
                Point::new(x, y),
                Point::new(x + w, y),
                Point::new(x + w, y + h),
                Point::new(x, y + h),
            ] {
                point(&mut path, p, &mut count)?;
            }
            path.closed = true;
        }
        "circle" | "ellipse" => {
            let x = coord(tag, "cx", 0.)?;
            let y = coord(tag, "cy", 0.)?;
            let (rx, ry) = if tag.name == "circle" {
                let r = coord(tag, "r", 0.)?;
                (r, r)
            } else {
                (coord(tag, "rx", 0.)?, coord(tag, "ry", 0.)?)
            };
            if rx < 0. || ry < 0. {
                return Err("SVG: invalid ellipse radius");
            }
            if rx == 0. || ry == 0. {
                return Ok(paths);
            }
            let required = (PI * (2. * rx.max(ry) * scale).sqrt()).ceil().max(12.);
            if required > 512. {
                return Err("SVG: ellipse tessellation limit");
            }
            let steps = required as usize;
            for i in 0..steps {
                let a = 2. * PI * i as f64 / steps as f64;
                point(
                    &mut path,
                    Point::new(x + rx * a.cos(), y + ry * a.sin()),
                    &mut count,
                )?;
            }
            path.closed = true;
        }
        "line" => {
            for p in [
                Point::new(coord(tag, "x1", 0.)?, coord(tag, "y1", 0.)?),
                Point::new(coord(tag, "x2", 0.)?, coord(tag, "y2", 0.)?),
            ] {
                point(&mut path, p, &mut count)?;
            }
        }
        "polygon" | "polyline" => {
            let values = number_list(
                attr(&tag.attributes, "points").unwrap_or(""),
                MAX_POINTS * 2,
            )?;
            if values.len() % 2 != 0 {
                return Err("SVG: incomplete point");
            }
            for pair in values.as_chunks::<2>().0 {
                point(&mut path, Point::new(pair[0], pair[1]), &mut count)?;
            }
            path.closed = tag.name == "polygon";
        }
        "path" => return path_data(attr(&tag.attributes, "d").unwrap_or(""), scale),
        _ => return Err("SVG: unsupported element"),
    }
    if !path.points.is_empty() {
        push(&mut paths, path)?;
    }
    Ok(paths)
}
impl Rect {
    pub(crate) fn width(self) -> usize {
        self.x1 - self.x0
    }
    pub(crate) fn height(self) -> usize {
        self.y1 - self.y0
    }
}
