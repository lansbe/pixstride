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
        if node.is_element()
            && matches!(
                node.tag_name().name(),
                "script" | "foreignObject" | "animate" | "animateTransform" | "set"
            )
        {
            return Err("active SVG content unsupported");
        }
        for a in node.attributes() {
            let name = a.name().to_ascii_lowercase();
            if name.starts_with("on") || name == "base" {
                return Err("active SVG attribute unsupported");
            }
            if matches!(name.as_str(), "href" | "src") && !a.value().starts_with('#') {
                return Err("external SVG references unsupported");
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
