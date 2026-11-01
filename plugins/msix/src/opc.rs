//! The Open Packaging Conventions parts of an MSIX package: part names, `[Content_Types].xml` and `AppxBlockMap.xml`.

use crate::zip::Member;
use anyhow::{Result, bail};
use std::collections::HashMap;
use std::fmt::Write as _;

pub const MANIFEST: &str = "AppxManifest.xml";
pub const BLOCK_MAP: &str = "AppxBlockMap.xml";
pub const CONTENT_TYPES: &str = "[Content_Types].xml";
pub const OCTET_STREAM: &str = "application/octet-stream";

/// Extension, content type, and whether the file is stored rather than compressed, as the Microsoft reference writer
/// (`src/msix/pack/ContentType.cpp` in microsoft/msix-packaging) has them.
const TYPES: &[(&str, &str, bool)] = &[
    ("cab", "application/vnd.ms-cab-compressed", true),
    ("doc", "application/msword", false),
    ("dot", "application/msword", false),
    (
        "docm",
        "application/vnd.ms-word.document.macroenabled.12",
        true,
    ),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        true,
    ),
    (
        "dotm",
        "application/vnd.ms-word.document.macroenabled.12",
        true,
    ),
    (
        "dotx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        true,
    ),
    ("dll", "application/x-msdownload", false),
    ("dtd", "application/xml-dtd", false),
    ("exe", "application/x-msdownload", false),
    ("gz", "application/x-gzip-compressed", true),
    ("java", "application/java", false),
    ("json", "application/json", false),
    ("p7s", "application/x-pkcs7-signature", false),
    ("pdf", "application/pdf", false),
    ("ps", "application/postscript", false),
    (
        "potm",
        "application/vnd.ms-powerpoint.template.macroenabled.12",
        true,
    ),
    (
        "potx",
        "application/vnd.openxmlformats-officedocument.presentationml.template",
        true,
    ),
    (
        "ppam",
        "application/vnd.ms-powerpoint.addin.macroenabled.12",
        true,
    ),
    (
        "ppsm",
        "application/vnd.ms-powerpoint.slideshow.macroenabled.12",
        true,
    ),
    (
        "ppsx",
        "application/vnd.openxmlformats-officedocument.presentationml.slideshow",
        true,
    ),
    ("ppt", "application/vnd.ms-powerpoint", false),
    ("pot", "application/vnd.ms-powerpoint", false),
    ("pps", "application/vnd.ms-powerpoint", false),
    ("ppa", "application/vnd.ms-powerpoint", false),
    (
        "pptm",
        "application/vnd.ms-powerpoint.presentation.macroenabled.12",
        true,
    ),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        true,
    ),
    ("rar", "application/x-rar-compressed", true),
    ("rss", "application/rss+xml", false),
    ("soap", "application/soap+xml", false),
    ("tar", "application/x-tar", true),
    ("xaml", "application/xaml+xml", false),
    ("xap", "application/x-silverlight-app", true),
    ("xbap", "application/x-ms-xbap", false),
    ("xhtml", "application/xhtml+xml", false),
    (
        "xlam",
        "application/vnd.ms-excel.addin.macroenabled.12",
        true,
    ),
    ("xls", "application/vnd.ms-excel", false),
    ("xlt", "application/vnd.ms-excel", false),
    ("xla", "application/vnd.ms-excel", false),
    (
        "xlsb",
        "application/vnd.ms-excel.sheet.binary.macroEnabled.12",
        true,
    ),
    (
        "xlsm",
        "application/vnd.ms-excel.sheet.macroEnabled.12",
        true,
    ),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        true,
    ),
    (
        "xltm",
        "application/vnd.ms-excel.template.macroEnabled.12",
        true,
    ),
    (
        "xltx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.template",
        true,
    ),
    ("xsl", "application/xslt+xml", false),
    ("xslt", "application/xslt+xml", false),
    ("zip", "application/x-zip-compressed", true),
    ("c", "text/plain", false),
    ("cpp", "text/plain", false),
    ("cs", "text/plain", false),
    ("css", "text/css", false),
    ("csv", "text/csv", false),
    ("h", "text/plain", false),
    ("htm", "text/html", false),
    ("html", "text/html", false),
    ("js", "application/x-javascript", false),
    ("rtf", "text/richtext", false),
    ("sct", "text/scriptlet", false),
    ("txt", "text/plain", false),
    ("xml", "text/xml", false),
    ("xsd", "text/xml", false),
    ("aiff", "audio/x-aiff", false),
    ("au", "audio/basic", false),
    ("m4a", "audio/mp4", true),
    ("mid", "audio/mid", false),
    ("mp3", "audio/mpeg", true),
    ("smf", "audio/mid", false),
    ("wav", "audio/wav", false),
    ("wma", "audio/x-ms-wma", true),
    ("bmp", "image/bmp", false),
    ("emf", "image/x-emf", false),
    ("gif", "image/gif", true),
    ("ico", "image/vnd.microsoft.icon", false),
    ("jpg", "image/jpeg", true),
    ("jpeg", "image/jpeg", true),
    ("png", "image/png", true),
    ("svg", "image/svg+xml", false),
    ("tif", "image/tiff", false),
    ("tiff", "image/tiff", false),
    ("wmf", "image/x-wmf", false),
    ("avi", "video/avi", true),
    ("mpeg", "video/mpeg", true),
    ("mpg", "video/mpeg", true),
    ("mov", "video/quicktime", true),
    ("wmv", "video/x-ms-wmv", true),
    ("atom", "application/atom+xml", false),
    ("appx", "application/vnd.ms-appx", true),
    ("b64", "application/base64", false),
];

/// The lowercase extension of the last segment of `name`, if it has a dot.
fn extension(name: &str) -> Option<String> {
    let last = name.rsplit('/').next()?;
    last.rfind('.')
        .map(|dot| last[dot + 1..].to_ascii_lowercase())
}

/// The content type of a file, by its extension. Unknown extensions are `application/octet-stream`.
pub fn content_type(name: &str) -> &'static str {
    let ext = extension(name).unwrap_or_default();
    TYPES
        .iter()
        .find(|(known, _, _)| *known == ext)
        .map_or(OCTET_STREAM, |(_, mime, _)| mime)
}

/// Whether a file is stored, not compressed, in the package. Unknown extensions are compressed.
pub fn stored(name: &str) -> bool {
    let ext = extension(name).unwrap_or_default();
    TYPES
        .iter()
        .find(|(known, _, _)| *known == ext)
        .is_some_and(|(_, _, stored)| *stored)
}

/// Checks a package-relative file name (`/` separators) that a payload file may take.
pub fn check_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("an empty file name");
    }
    if name.contains('\\') {
        bail!("{name:?} has a backslash, which MSIX file names cannot have");
    }
    if let Some(bad) = name
        .chars()
        .find(|c| (*c as u32) < 0x20 || matches!(c, '"' | '*' | ':' | '<' | '>' | '?' | '|'))
    {
        bail!("{name:?} has the character {bad:?}, which MSIX file names cannot have");
    }
    let segments: Vec<&str> = name.split('/').collect();
    if segments
        .iter()
        .any(|s| s.is_empty() || *s == "." || *s == "..")
    {
        bail!("{name:?} has an empty, \".\" or \"..\" segment");
    }
    let first = segments[0].to_ascii_lowercase();
    if first == "_rels"
        || first.starts_with("appxmetadata")
        || first.starts_with("microsoft.system.package.metadata")
    {
        bail!("{name:?} is in a folder MSIX reserves");
    }
    if segments.len() == 1 {
        let lower = first.as_str();
        if matches!(
            lower,
            "appxmanifest.xml" | "appxblockmap.xml" | "[content_types].xml" | "appxsignature.p7x"
        ) {
            bail!("{name:?} is a file MSIX reserves");
        }
    }
    Ok(())
}

/// The zip member name of a part: percent-encoded like any part name, except `[Content_Types].xml`, which OPC keeps literal.
pub fn zip_name(name: &str) -> String {
    if name == CONTENT_TYPES {
        name.to_owned()
    } else {
        encode(name)
    }
}

/// The part name of a package-relative file name in the zip: percent-encoded as the Microsoft writer does it.
pub fn encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        match byte {
            b' '
            | b'!'
            | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'+'
            | b','
            | b';'
            | b'='
            | b'@'
            | b'['
            | b']'
            | b'^'
            | b'`'
            | b'{'
            | b'}'
            | 0x80..=0xFF => {
                let _ = write!(out, "%{byte:02X}");
            }
            _ => out.push(byte as char),
        }
    }
    out
}

/// Escapes text for an XML attribute or element.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// `[Content_Types].xml`: one default per extension, and an override for every other part.
#[derive(Default)]
pub struct ContentTypes {
    defaults: HashMap<String, String>,
    lines: Vec<String>,
}

impl ContentTypes {
    /// Adds a payload file, by its package-relative name, with the content type of its extension. The first file with an
    /// extension sets its default; a later file with a different type gets an override.
    pub fn add_payload(&mut self, name: &str, content_type: &str) {
        let part = format!("/{}", encode(name));
        match extension(name) {
            None => self.override_part(&part, content_type),
            Some(ext) => match self.defaults.get(&ext) {
                None => {
                    self.lines.push(format!(
                        "<Default Extension=\"{}\" ContentType=\"{}\"/>",
                        escape(&ext),
                        escape(content_type)
                    ));
                    self.defaults.insert(ext, content_type.to_owned());
                }
                Some(existing) if existing != content_type => {
                    self.override_part(&part, content_type);
                }
                Some(_) => {}
            },
        }
    }

    /// Adds an override for a part that does not take its type from its extension.
    pub fn override_part(&mut self, part: &str, content_type: &str) {
        self.lines.push(format!(
            "<Override PartName=\"{}\" ContentType=\"{}\"/>",
            escape(part),
            escape(content_type)
        ));
    }

    pub fn to_xml(&self) -> String {
        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n",
        );
        for line in &self.lines {
            xml.push_str(line);
            xml.push('\n');
        }
        xml.push_str("</Types>\n");
        xml
    }
}

/// `AppxBlockMap.xml`: each file's uncompressed size and the SHA-256 of each of its 64 KiB blocks, so that Windows
/// can check the package as it reads it.
pub fn block_map(members: &[Member]) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <BlockMap xmlns=\"http://schemas.microsoft.com/appx/2010/blockmap\" \
         HashMethod=\"http://www.w3.org/2001/04/xmlenc#sha256\">\n",
    );
    for member in members {
        let name = escape(&member.name.replace('/', "\\"));
        if member.blocks.is_empty() {
            let _ = writeln!(
                xml,
                "  <File Size=\"{}\" Name=\"{name}\" LfhSize=\"{}\"/>",
                member.size, member.lfh_size
            );
            continue;
        }
        let _ = writeln!(
            xml,
            "  <File Size=\"{}\" Name=\"{name}\" LfhSize=\"{}\">",
            member.size, member.lfh_size
        );
        for block in &member.blocks {
            match block.compressed {
                Some(size) => {
                    let _ = writeln!(xml, "    <Block Size=\"{size}\" Hash=\"{}\"/>", block.hash);
                }
                None => {
                    let _ = writeln!(xml, "    <Block Hash=\"{}\"/>", block.hash);
                }
            }
        }
        xml.push_str("  </File>\n");
    }
    xml.push_str("</BlockMap>\n");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_percent_encoded_like_the_reference_writer() {
        assert_eq!(encode("bin/my app (1).exe"), "bin/my%20app%20%281%29.exe");
        assert_eq!(encode("data/café"), "data/caf%C3%A9");
    }

    #[test]
    fn unsafe_names_are_refused() {
        assert!(check_name("bin/hi.exe").is_ok());
        assert!(check_name("bin/a:b").is_err());
        assert!(check_name("bin\\hi.exe").is_err());
        assert!(check_name("../x").is_err());
        assert!(check_name("AppxManifest.xml").is_err());
        assert!(check_name("sub/AppxManifest.xml").is_ok());
        assert!(check_name("_rels/x").is_err());
    }

    #[test]
    fn content_types_follow_the_extension() {
        assert_eq!(content_type("bin/hi.exe"), "application/x-msdownload");
        assert_eq!(content_type("README"), OCTET_STREAM);
        assert!(stored("Assets/logo.PNG"));
        assert!(!stored("bin/hi.exe"));
    }

    #[test]
    fn a_second_type_for_an_extension_is_an_override() {
        let mut types = ContentTypes::default();
        types.add_payload("a/x.txt", "text/plain");
        types.add_payload("b/y.txt", "text/plain");
        types.add_payload("c/z.txt", "text/csv");
        types.add_payload("d/noext", OCTET_STREAM);
        let xml = types.to_xml();
        assert_eq!(xml.matches("<Default ").count(), 1);
        assert!(xml.contains("<Override PartName=\"/c/z.txt\" ContentType=\"text/csv\"/>"));
        assert!(xml.contains("<Override PartName=\"/d/noext\""));
    }
}
