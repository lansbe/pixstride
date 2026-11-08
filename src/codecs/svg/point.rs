use super::*;
impl Point {
    pub(crate) fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub(crate) fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
    pub(crate) fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
    pub(crate) fn mul(self, value: f64) -> Self {
        Self::new(self.x * value, self.y * value)
    }
    pub(crate) fn cross(self, other: Self) -> f64 {
        self.x * other.y - self.y * other.x
    }
    pub(crate) fn length(self) -> f64 {
        self.x.hypot(self.y)
    }
}
impl Matrix {
    pub(crate) fn identity() -> Self {
        Self {
            a: 1.,
            b: 0.,
            c: 0.,
            d: 1.,
            e: 0.,
            f: 0.,
        }
    }
    pub(crate) fn point(self, p: Point) -> Point {
        Point::new(
            self.a * p.x + self.c * p.y + self.e,
            self.b * p.x + self.d * p.y + self.f,
        )
    }
    pub(crate) fn then(self, right: Self) -> Self {
        Self {
            a: self.a * right.a + self.c * right.b,
            b: self.b * right.a + self.d * right.b,
            c: self.a * right.c + self.c * right.d,
            d: self.b * right.c + self.d * right.d,
            e: self.a * right.e + self.c * right.f + self.e,
            f: self.b * right.e + self.d * right.f + self.f,
        }
    }
    pub(crate) fn inverse(self) -> Result<Self, Error> {
        let det = self.a * self.d - self.b * self.c;
        if !det.is_finite() || det.abs() < 1e-14 {
            return Err("SVG: singular transform unsupported");
        }
        let inv = Self {
            a: self.d / det,
            b: -self.b / det,
            c: -self.c / det,
            d: self.a / det,
            e: (self.c * self.f - self.d * self.e) / det,
            f: (self.b * self.e - self.a * self.f) / det,
        };
        if [inv.a, inv.b, inv.c, inv.d, inv.e, inv.f]
            .iter()
            .any(|x| !x.is_finite() || x.abs() > 1e12)
        {
            return Err("SVG: transform outside limit");
        }
        Ok(inv)
    }
    pub(crate) fn scale_bound(self) -> f64 {
        (self.a * self.a + self.b * self.b + self.c * self.c + self.d * self.d).sqrt()
    }
}
impl Default for Style {
    fn default() -> Self {
        Self {
            fill: Some(Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            }),
            stroke: None,
            width: 1.,
            fill_alpha: 1.,
            stroke_alpha: 1.,
            opacity: 1.,
            evenodd: false,
            cap: Cap::Butt,
            join: Join::Miter,
            miter: 4.,
        }
    }
}
pub(crate) fn push<T>(values: &mut Vec<T>, value: T) -> Result<(), Error> {
    values
        .try_reserve(1)
        .map_err(|_| "SVG: allocation failed")?;
    values.push(value);
    Ok(())
}
pub(crate) fn point(path: &mut Path, p: Point, count: &mut usize) -> Result<(), Error> {
    if !p.x.is_finite()
        || !p.y.is_finite()
        || p.x.abs() > 1e9
        || p.y.abs() > 1e9
        || *count == MAX_POINTS
    {
        return Err("SVG: point outside limit");
    }
    push(&mut path.points, p)?;
    *count += 1;
    Ok(())
}
impl<'a> Numbers<'a> {
    pub(crate) fn new(s: &'a str) -> Self {
        Self { s, pos: 0 }
    }
    pub(crate) fn skip(&mut self) {
        while self
            .s
            .as_bytes()
            .get(self.pos)
            .is_some_and(|c| c.is_ascii_whitespace() || *c == b',')
        {
            self.pos += 1;
        }
    }
    pub(crate) fn end(&mut self) -> bool {
        self.skip();
        self.pos == self.s.len()
    }
    pub(crate) fn number(&mut self) -> Result<f64, Error> {
        self.skip();
        let start = self.pos;
        let b = self.s.as_bytes();
        if matches!(b.get(self.pos), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let mut digits = 0;
        while b.get(self.pos).is_some_and(u8::is_ascii_digit) {
            self.pos += 1;
            digits += 1;
        }
        if b.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            while b.get(self.pos).is_some_and(u8::is_ascii_digit) {
                self.pos += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return Err("SVG: expected number");
        }
        if matches!(b.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(b.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let exponent = self.pos;
            while b.get(self.pos).is_some_and(u8::is_ascii_digit) {
                self.pos += 1;
            }
            if exponent == self.pos {
                return Err("SVG: invalid exponent");
            }
        }
        let value: f64 = self.s[start..self.pos]
            .parse()
            .map_err(|_| "SVG: invalid number")?;
        if !value.is_finite() || value.abs() > 1e9 {
            return Err("SVG: number outside limit");
        }
        Ok(value)
    }
    pub(crate) fn pair(&mut self) -> Result<Point, Error> {
        Ok(Point::new(self.number()?, self.number()?))
    }
}
pub(crate) fn one_number(s: &str) -> Result<f64, Error> {
    let s = s.trim();
    let s = s.strip_suffix("px").unwrap_or(s);
    let mut n = Numbers::new(s);
    let v = n.number()?;
    if !n.end() {
        return Err("SVG: unsupported unit or number list");
    }
    Ok(v)
}
pub(crate) fn number_list(s: &str, limit: usize) -> Result<Vec<f64>, Error> {
    let mut parser = Numbers::new(s);
    let mut values = Vec::new();
    while !parser.end() {
        if values.len() == limit {
            return Err("SVG: too many values");
        }
        push(&mut values, parser.number()?)?;
    }
    Ok(values)
}
pub(crate) fn transform(s: &str) -> Result<Matrix, Error> {
    let mut at = 0usize;
    let bytes = s.as_bytes();
    let mut result = Matrix::identity();
    let mut count = 0;
    while at < bytes.len() {
        while bytes
            .get(at)
            .is_some_and(|c| c.is_ascii_whitespace() || *c == b',')
        {
            at += 1;
        }
        if at == bytes.len() {
            break;
        }
        let start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_alphabetic) {
            at += 1;
        }
        if start == at {
            return Err("SVG: invalid transform");
        }
        let name = &s[start..at];
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if bytes.get(at) != Some(&b'(') {
            return Err("SVG: invalid transform");
        }
        at += 1;
        let start = at;
        while at < bytes.len() && bytes[at] != b')' {
            at += 1;
        }
        if at == bytes.len() {
            return Err("SVG: unterminated transform");
        }
        let n = number_list(&s[start..at], 6)?;
        at += 1;
        count += 1;
        if count > 32 {
            return Err("SVG: too many transforms");
        }
        let mut m = Matrix::identity();
        match (name, n.as_slice()) {
            ("matrix", [a, b, c, d, e, f]) => {
                m = Matrix {
                    a: *a,
                    b: *b,
                    c: *c,
                    d: *d,
                    e: *e,
                    f: *f,
                }
            }
            ("translate", [x]) => m.e = *x,
            ("translate", [x, y]) => {
                m.e = *x;
                m.f = *y;
            }
            ("scale", [x]) => {
                m.a = *x;
                m.d = *x;
            }
            ("scale", [x, y]) => {
                m.a = *x;
                m.d = *y;
            }
            ("rotate", [angle]) | ("rotate", [angle, _, _]) => {
                let radians = angle * PI / 180.;
                let (sin, cos) = radians.sin_cos();
                m = Matrix {
                    a: cos,
                    b: sin,
                    c: -sin,
                    d: cos,
                    e: 0.,
                    f: 0.,
                };
                if n.len() == 3 {
                    let mut before = Matrix::identity();
                    before.e = n[1];
                    before.f = n[2];
                    let mut after = Matrix::identity();
                    after.e = -n[1];
                    after.f = -n[2];
                    m = before.then(m).then(after);
                }
            }
            ("skewX", [angle]) => m.c = (angle * PI / 180.).tan(),
            ("skewY", [angle]) => m.b = (angle * PI / 180.).tan(),
            _ => return Err("SVG: unsupported transform"),
        }
        result = result.then(m);
        if [result.a, result.b, result.c, result.d, result.e, result.f]
            .iter()
            .any(|n| !n.is_finite() || n.abs() > 1e9)
        {
            return Err("SVG: transform outside limit");
        }
    }
    Ok(result)
}
pub(crate) fn color(s: &str) -> Result<Option<Color>, Error> {
    let s = s.trim();
    if s == "none" {
        return Ok(None);
    }
    let named = match s {
        "black" => Some((0, 0, 0, 255)),
        "white" => Some((255, 255, 255, 255)),
        "red" => Some((255, 0, 0, 255)),
        "green" => Some((0, 128, 0, 255)),
        "blue" => Some((0, 0, 255, 255)),
        "yellow" => Some((255, 255, 0, 255)),
        "cyan" | "aqua" => Some((0, 255, 255, 255)),
        "magenta" | "fuchsia" => Some((255, 0, 255, 255)),
        "gray" | "grey" => Some((128, 128, 128, 255)),
        "orange" => Some((255, 165, 0, 255)),
        "purple" => Some((128, 0, 128, 255)),
        "transparent" => Some((0, 0, 0, 0)),
        _ => None,
    };
    if let Some((r, g, b, a)) = named {
        return Ok(Some(Color { r, g, b, a }));
    }
    if let Some(hex) = s.strip_prefix('#') {
        let digit = |x: u8| -> Result<u8, Error> {
            (x as char)
                .to_digit(16)
                .map(|v| v as u8)
                .ok_or("SVG: invalid color")
        };
        let b = hex.as_bytes();
        let (r, g, blue, a) = match b.len() {
            3 => (digit(b[0])? * 17, digit(b[1])? * 17, digit(b[2])? * 17, 255),
            4 => (
                digit(b[0])? * 17,
                digit(b[1])? * 17,
                digit(b[2])? * 17,
                digit(b[3])? * 17,
            ),
            6 | 8 => {
                let v = |i| -> Result<u8, Error> { Ok(digit(b[i])? * 16 + digit(b[i + 1])?) };
                (v(0)?, v(2)?, v(4)?, if b.len() == 8 { v(6)? } else { 255 })
            }
            _ => return Err("SVG: invalid color"),
        };
        return Ok(Some(Color { r, g, b: blue, a }));
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|v| v.strip_suffix(')')) {
        let values = number_list(inner, 3)?;
        if values.len() != 3
            || values
                .iter()
                .any(|v| *v < 0. || *v > 255. || v.fract() != 0.)
        {
            return Err("SVG: unsupported rgb color");
        }
        return Ok(Some(Color {
            r: values[0] as u8,
            g: values[1] as u8,
            b: values[2] as u8,
            a: 255,
        }));
    }
    Err("SVG: unsupported color or paint server")
}
pub(crate) fn opacity(s: &str) -> Result<f64, Error> {
    let value = one_number(s)?;
    if !(0. ..=1.).contains(&value) {
        return Err("SVG: opacity outside 0..1");
    }
    Ok(value)
}
pub(crate) fn property(style: &mut Style, name: &str, value: &str) -> Result<(), Error> {
    match name.trim() {
        "fill" => style.fill = color(value)?,
        "stroke" => style.stroke = color(value)?,
        "stroke-width" => {
            style.width = one_number(value)?;
            if style.width < 0. {
                return Err("SVG: invalid stroke width");
            }
        }
        "fill-opacity" => style.fill_alpha = opacity(value)?,
        "stroke-opacity" => style.stroke_alpha = opacity(value)?,
        "opacity" => style.opacity = opacity(value)?,
        "fill-rule" => {
            style.evenodd = match value.trim() {
                "nonzero" => false,
                "evenodd" => true,
                _ => return Err("SVG: unsupported fill rule"),
            }
        }
        "stroke-linecap" => {
            style.cap = match value.trim() {
                "butt" => Cap::Butt,
                "round" => Cap::Round,
                "square" => Cap::Square,
                _ => return Err("SVG: unsupported linecap"),
            }
        }
        "stroke-linejoin" => {
            style.join = match value.trim() {
                "miter" => Join::Miter,
                "round" => Join::Round,
                "bevel" => Join::Bevel,
                _ => return Err("SVG: unsupported linejoin"),
            }
        }
        "stroke-miterlimit" => {
            style.miter = one_number(value)?;
            if !(1. ..=100.).contains(&style.miter) {
                return Err("SVG: invalid miter limit");
            }
        }
        _ => return Err("SVG: unsupported style property"),
    }
    Ok(())
}
