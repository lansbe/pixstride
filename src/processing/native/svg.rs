//! Admit static SVG without allowing renderer access to external resources.
use crate::Error;

// CSS escapes and comments can obscure tokens; reject these before checking
// URLs. Fragment references may be quoted or unquoted in attributes/styles.
fn safe_css(value: &str) -> bool {
    let value = value.to_ascii_lowercase().replace(char::is_whitespace, "");
    if value.contains(['@', '\\']) || value.contains("/*") {
        return false;
    }
    let mut rest = value.as_str();
    while let Some((_, suffix)) = rest.split_once("url(") {
        let Some((url, tail)) = suffix.split_once(')') else {
            return false;
        };
        let url = if url.len() >= 2
            && (url.starts_with('\'') && url.ends_with('\'')
                || url.starts_with('"') && url.ends_with('"'))
        {
            &url[1..url.len() - 1]
        } else {
            url
        };
        if !url.starts_with('#') || url.len() < 2 || url.contains(['\'', '"', '(']) {
            return false;
        }
        rest = tail;
    }
    true
}

fn reference(value: &str) -> bool {
    if value.starts_with('#') {
        return true;
    }
    let Some((header, body)) = value.split_once(',') else {
        return false;
    };
    let expected = if header.eq_ignore_ascii_case("data:image/png;base64") {
        crate::request::OutputFormat::Png
    } else if header.eq_ignore_ascii_case("data:image/jpeg;base64") {
        crate::request::OutputFormat::Jpeg
    } else {
        return false;
    };
    if body.len() > 10 * 1024 * 1024 {
        return false;
    }
    let body = body
        .replace(char::is_whitespace, "")
        .replace('+', "-")
        .replace('/', "_");
    let Ok(data) = crate::signature::decode_base64url(&body, true) else {
        return false;
    };
    if super::sniff(&data) != Some(expected) {
        return false;
    }
    super::engine::load(&data, 1, false)
        .is_ok_and(|i| super::engine::checked(i.get_width(), i.get_height()).is_ok())
}

fn safe_element(node: roxmltree::Node<'_, '_>) -> bool {
    match node.tag_name().namespace() {
        None | Some("http://www.w3.org/2000/svg") => true,
        Some(
            "http://www.inkscape.org/namespaces/inkscape"
            | "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd",
        ) => true,
        Some(ns) => {
            node.ancestors()
                .skip(1)
                .any(|parent| parent.is_element() && parent.tag_name().name() == "metadata")
                && (matches!(
                    ns,
                    "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                        | "http://purl.org/dc/elements/1.1/"
                        | "http://creativecommons.org/ns#"
                ) || ns.starts_with("http://ns.adobe.com/"))
        }
    }
}

pub(super) fn validate(data: &[u8]) -> Result<(), Error> {
    if data.len() > 10 * 1024 * 1024 {
        return Err("SVG exceeds native limit");
    }
    let text = std::str::from_utf8(data).map_err(|_| "invalid SVG encoding")?;
    let doc = roxmltree::Document::parse(text).map_err(|_| "invalid static SVG")?;
    if doc.root_element().tag_name().name() != "svg" {
        return Err("SVG root required");
    }
    let mut count = 0;
    for node in doc.descendants() {
        count += 1;
        if count > 100_000 || node.ancestors().take(65).count() > 64 {
            return Err("SVG structure exceeds limit");
        }
        if node.pi().is_some_and(|pi| pi.target != "xpacket") {
            return Err("SVG processing instructions unsupported");
        }
        if node.is_element() && !safe_element(node) {
            return Err("active foreign SVG namespace unsupported");
        }
        if node.is_element() && active(node.tag_name().name()) {
            return Err("active SVG content unsupported");
        }
        for a in node.attributes() {
            let name = a.name().to_ascii_lowercase();
            if name.starts_with("on") || name == "base" {
                return Err("active SVG attribute unsupported");
            }
            if matches!(name.as_str(), "href" | "src") {
                if !reference(a.value()) {
                    return Err("external SVG references unsupported");
                }
                continue;
            }
            if !safe_css(a.value()) {
                return Err("external SVG style unsupported");
            }
        }
        if node.is_element() && node.tag_name().name() == "style" {
            let css = node
                .descendants()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .collect::<String>();
            if !safe_css(&css) {
                return Err("external SVG stylesheet unsupported");
            }
        }
    }
    Ok(())
}

fn active(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "foreignObject"
            | "animate"
            | "animateTransform"
            | "animateMotion"
            | "set"
            | "discard"
    )
}
fn text(out: &mut String, value: &str) -> Result<(), Error> {
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
        if out.len() > 10 * 1024 * 1024 {
            return Err("sanitized SVG exceeds limit");
        }
    }
    Ok(())
}
fn qualified(node: roxmltree::Node<'_, '_>, namespace: Option<&str>, name: &str) -> String {
    namespace
        .and_then(|ns| node.lookup_prefix(ns))
        .map_or_else(|| name.to_owned(), |p| format!("{p}:{name}"))
}
fn emit(node: roxmltree::Node<'_, '_>, out: &mut String) -> Result<(), Error> {
    if node.is_text() {
        return text(out, node.text().unwrap_or(""));
    }
    if !node.is_element() || !safe_element(node) || active(node.tag_name().name()) {
        return Ok(());
    }
    if node.tag_name().name() == "style"
        && !safe_css(
            &node
                .descendants()
                .filter(|n| n.is_text())
                .map(|n| n.text().unwrap_or(""))
                .collect::<String>(),
        )
    {
        return Ok(());
    }
    let name = qualified(node, node.tag_name().namespace(), node.tag_name().name());
    out.push('<');
    out.push_str(&name);
    for ns in node.namespaces() {
        out.push_str(" xmlns");
        if let Some(prefix) = ns.name() {
            out.push(':');
            out.push_str(prefix);
        }
        out.push_str("=\"");
        text(out, ns.uri())?;
        out.push('"');
    }
    for a in node.attributes() {
        let lower = a.name().to_ascii_lowercase();
        if lower.starts_with("on")
            || lower == "base"
            || if matches!(lower.as_str(), "href" | "src") {
                !reference(a.value())
            } else {
                !safe_css(a.value())
            }
        {
            continue;
        }
        out.push(' ');
        out.push_str(&qualified(node, a.namespace(), a.name()));
        out.push_str("=\"");
        text(out, a.value())?;
        out.push('"');
    }
    out.push('>');
    for child in node.children() {
        emit(child, out)?;
    }
    out.push_str("</");
    out.push_str(&name);
    out.push('>');
    if out.len() > 10 * 1024 * 1024 {
        return Err("sanitized SVG exceeds limit");
    }
    Ok(())
}
pub(super) fn sanitize(data: &[u8]) -> Result<Vec<u8>, Error> {
    if data.len() > 10 * 1024 * 1024 {
        return Err("SVG exceeds native limit");
    }
    let doc =
        roxmltree::Document::parse(std::str::from_utf8(data).map_err(|_| "invalid SVG encoding")?)
            .map_err(|_| "invalid static SVG")?;
    if doc.root_element().tag_name().name() != "svg" {
        return Err("SVG root required");
    }
    for (index, node) in doc.descendants().enumerate() {
        if index >= 100_000 || node.ancestors().take(65).count() > 64 {
            return Err("SVG structure exceeds limit");
        }
    }
    let mut out = String::new();
    emit(doc.root_element(), &mut out)?;
    validate(out.as_bytes())?;
    Ok(out.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_css_references_are_safe_but_external_obfuscated_urls_are_rejected() {
        for css in [
            "fill:url(#g)",
            "mask: url( '#mask' )",
            "rect{fill:url(\"#g\")}",
        ] {
            assert!(safe_css(css), "{css}");
        }
        for css in [
            "fill:url(https://example.invalid/a)",
            "fill:url('file:///x')",
            "@import 'x'",
            "u/**/rl(x)",
            "u\\72l(x)",
            "url(#x) url(//example.invalid)",
        ] {
            assert!(!safe_css(css), "{css}");
        }
    }
}
