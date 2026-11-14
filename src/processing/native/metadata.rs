//! Preserve selected copyright fields while dropping unrelated private metadata.
use crate::{request::Request, Error};
use libvips::{bindings, voption::Setter, VipsImage};
use std::ffi::CStr;

fn names(image: &VipsImage) -> Vec<String> {
    // SAFETY: image owns a live VipsImage for this entire call. libvips returns
    // a newly allocated, NULL-terminated string vector; g_strfreev frees it once.
    unsafe {
        let list = bindings::vips_image_get_fields(image.as_mut_ptr());
        if list.is_null() {
            return Vec::new();
        }
        let mut result = Vec::new();
        let mut p = list;
        while !(*p).is_null() {
            result.push(CStr::from_ptr(*p).to_string_lossy().into_owned());
            p = p.add(1);
        }
        bindings::g_strfreev(list);
        result
    }
}

pub(super) fn strip(image: &VipsImage, r: &Request) -> Result<VipsImage, Error> {
    let mut image = if r.strip_metadata {
        image.copy_with_opts(
            libvips::voption::VOption::new()
                .set("xres", 72.0 / 25.4)
                .set("yres", 72.0 / 25.4),
        )
    } else {
        image.copy()
    }
    .map_err(|_| "native metadata copy failed")?;
    if r.strip_color_profile {
        image
            .remove("icc-profile-data")
            .map_err(|_| "cannot strip colour profile")?;
    }
    if !r.strip_metadata {
        return Ok(image);
    }
    let xmp = if r.extra.keep_copyright {
        image
            .get_blob("xmp-data")
            .ok()
            .and_then(|b| copyright_xmp(&b))
    } else {
        None
    };
    let iptc = if r.extra.keep_copyright {
        image
            .get_blob("iptc-data")
            .ok()
            .and_then(|b| copyright_iptc(&b))
    } else {
        None
    };
    for name in names(&image) {
        if matches!(
            name.as_str(),
            "icc-profile-data"
                | "page-height"
                | "n-pages"
                | "delay"
                | "loop"
                | "vips-loader"
                | "bits-per-sample"
        ) {
            continue;
        }
        if r.extra.keep_copyright
            && matches!(name.as_str(), "exif-ifd0-Copyright" | "exif-ifd0-Artist")
        {
            continue;
        }
        image
            .remove(&name)
            .map_err(|_| "cannot strip image metadata")?;
    }
    if let Some(blob) = xmp {
        image
            .set_blob_copy("xmp-data", &blob)
            .map_err(|_| "cannot retain copyright XMP")?;
    }
    if let Some(blob) = iptc {
        image
            .set_blob_copy("iptc-data", &blob)
            .map_err(|_| "cannot retain copyright IPTC")?;
    }
    Ok(image)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn prefix(ns: &str) -> Option<&'static str> {
    match ns {
        "http://purl.org/dc/elements/1.1/" => Some("dc"),
        "http://ns.adobe.com/xap/1.0/rights/" => Some("xmpRights"),
        "http://creativecommons.org/ns#" => Some("cc"),
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#" => Some("rdf"),
        "http://www.w3.org/XML/1998/namespace" => Some("xml"),
        _ => None,
    }
}
fn emit(node: roxmltree::Node<'_, '_>, out: &mut String, depth: usize) {
    if depth > 32 {
        return;
    }
    if node.is_text() {
        out.push_str(&escape(node.text().unwrap_or("")));
        return;
    }
    if !node.is_element() {
        return;
    }
    let Some(p) = node.tag_name().namespace().and_then(prefix) else {
        return;
    };
    out.push_str(&format!("<{p}:{}", node.tag_name().name()));
    for a in node.attributes() {
        if let Some(p) = a.namespace().and_then(prefix) {
            out.push_str(&format!(" {p}:{}=\"{}\"", a.name(), escape(a.value())));
        }
    }
    out.push('>');
    for child in node.children() {
        emit(child, out, depth + 1);
    }
    out.push_str(&format!("</{p}:{}>", node.tag_name().name()));
}
fn copyright_xmp(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() > 1024 * 1024 {
        return None;
    }
    let doc = roxmltree::Document::parse(std::str::from_utf8(bytes).ok()?).ok()?;
    let mut body = String::new();
    for desc in doc.descendants().filter(|n| {
        n.is_element()
            && n.tag_name().name() == "Description"
            && n.tag_name().namespace().and_then(prefix) == Some("rdf")
    }) {
        for a in desc.attributes() {
            let p = a.namespace().and_then(prefix);
            if p == Some("xmpRights")
                || p == Some("cc")
                || (p == Some("dc")
                    && matches!(a.name(), "rights" | "creator" | "contributor" | "publisher"))
            {
                let p = p.unwrap();
                body.push_str(&format!(
                    "<{p}:{}>{}</{p}:{}>",
                    a.name(),
                    escape(a.value()),
                    a.name()
                ));
            }
        }
        for node in desc.children().filter(|n| n.is_element()) {
            let p = node.tag_name().namespace().and_then(prefix);
            if p == Some("xmpRights")
                || p == Some("cc")
                || (p == Some("dc")
                    && matches!(
                        node.tag_name().name(),
                        "rights" | "creator" | "contributor" | "publisher"
                    ))
            {
                emit(node, &mut body, 0);
            }
        }
    }
    if body.is_empty() {
        return None;
    }
    Some(format!("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:xmpRights=\"http://ns.adobe.com/xap/1.0/rights/\" xmlns:cc=\"http://creativecommons.org/ns#\">{body}</rdf:Description></rdf:RDF></x:xmpmeta>").into_bytes())
}

fn datasets(data: &[u8]) -> Option<Vec<u8>> {
    let mut i = 0;
    let mut out = Vec::new();
    while i < data.len() {
        let h = data.get(i..i + 5)?;
        if h[0] != 0x1c {
            return None;
        }
        let len = u16::from_be_bytes([h[3], h[4]]) as usize;
        if len & 0x8000 != 0 {
            return None;
        }
        let end = i.checked_add(5 + len)?;
        let field = data.get(i..end)?;
        if h[1] == 2 && matches!(h[2], 80 | 110 | 116) {
            out.extend_from_slice(field);
        }
        i = end;
    }
    Some(out)
}
fn copyright_iptc(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() > 1024 * 1024 {
        return None;
    }
    if data.first() == Some(&0x1c) {
        return datasets(data).filter(|b| !b.is_empty());
    }
    let mut i = 0usize;
    while i < data.len() {
        if data.get(i..i + 4) != Some(b"8BIM") {
            return None;
        }
        let id = u16::from_be_bytes(data.get(i + 4..i + 6)?.try_into().ok()?);
        let name = usize::from(*data.get(i + 6)?) + 1;
        let start = i.checked_add(6 + name + (name % 2))?;
        let size = u32::from_be_bytes(data.get(start..start + 4)?.try_into().ok()?) as usize;
        let body = data.get(start + 4..start + 4 + size)?;
        if id == 0x0404 {
            let fields = datasets(body)?;
            if fields.is_empty() {
                return None;
            }
            let mut out = b"8BIM\x04\x04\0\0".to_vec();
            out.extend_from_slice(&(fields.len() as u32).to_be_bytes());
            out.extend_from_slice(&fields);
            if fields.len() % 2 == 1 {
                out.push(0);
            }
            return Some(out);
        }
        i = start.checked_add(4 + size + size % 2)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_xmp_has_only_copyright_fields_and_resolved_namespaces() {
        let input=br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:z="http://purl.org/dc/elements/1.1/" z:rights="Alice &amp; Bob" z:title="private"><z:creator><rdf:Seq><rdf:li>Author</rdf:li></rdf:Seq></z:creator><z:description>private</z:description></rdf:Description></rdf:RDF>"#;
        let result = copyright_xmp(input).unwrap();
        let text = std::str::from_utf8(&result).unwrap();
        assert!(roxmltree::Document::parse(text).is_ok());
        assert!(text.contains("Alice &amp; Bob"));
        assert!(text.contains("Author"));
        assert!(!text.contains("private"));
        assert!(
            copyright_xmp(b"<!DOCTYPE x [<!ENTITY e SYSTEM 'file:///x'>]><x>&e;</x>").is_none()
        );
    }
    #[test]
    fn iptc_private_fields_and_truncations_do_not_survive() {
        let data = b"\x1c\x02\x74\0\x05Alice\x1c\x02\x5a\0\x07private";
        assert_eq!(datasets(data).unwrap(), b"\x1c\x02\x74\0\x05Alice");
        assert!(datasets(&data[..data.len() - 1]).is_none());
    }
}
