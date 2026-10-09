//! .pptx import as zip listing + placeholder pages.
//!
//! SPEC section 3: real PowerPoint COM control is out of scope for v1.
//! Instead, a `.pptx` file (an OOXML zip) is opened, its slide parts
//! (`ppt/slides/slideN.xml`) are listed, and one placeholder page per slide
//! is produced. Rendering real slide content requires PowerPoint/COM or a
//! PDF export and is documented as a limitation (see README).
//!
//! The zip central directory is parsed by hand (EOCD + central-directory
//! entries) so this module needs no extra dependencies.

/// One entry of a zip central directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZipEntry {
    pub name: String,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub method: u16,
}

/// One imported slide: metadata only, no rendered content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PptxSlidePlaceholder {
    /// 1-based slide number parsed from `slideN.xml`.
    pub number: u32,
    /// Part name, e.g. `ppt/slides/slide3.xml`.
    pub part: String,
    pub note: String,
}

/// Result of importing a `.pptx` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PptxImport {
    pub file_name: String,
    pub entry_count: usize,
    pub slides: Vec<PptxSlidePlaceholder>,
}

fn u16le(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]))
}

fn u32le(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
}

/// Locate the End-Of-Central-Directory record (scans the last 64 KiB + 22).
fn find_eocd(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 22 {
        return None;
    }
    let start = bytes.len().saturating_sub(65557 + 22);
    let mut i = bytes.len() - 22;
    loop {
        if &bytes[i..i + 4] == b"PK\x05\x06" {
            return Some(i);
        }
        if i == start {
            return None;
        }
        i -= 1;
    }
}

/// List zip entries via the central directory (no decompression).
pub fn list_zip_entries(bytes: &[u8]) -> Result<Vec<ZipEntry>, String> {
    let eocd = find_eocd(bytes).ok_or_else(|| "not a zip file (EOCD not found)".to_string())?;
    let count = u16le(bytes, eocd + 10).ok_or("truncated EOCD")? as usize;
    let cd_off = u32le(bytes, eocd + 16).ok_or("truncated EOCD")? as usize;
    let mut entries = Vec::with_capacity(count);
    let mut off = cd_off;
    for _ in 0..count {
        if bytes.get(off..off + 4) != Some(b"PK\x01\x02".as_slice()) {
            return Err("bad central directory signature".to_string());
        }
        let method = u16le(bytes, off + 10).ok_or("truncated entry")?;
        let comp = u32le(bytes, off + 20).ok_or("truncated entry")?;
        let uncomp = u32le(bytes, off + 24).ok_or("truncated entry")?;
        // Central-directory layout: name/extra/comment lens at +28/+30/+32.
        let name_len = u16le(bytes, off + 28).ok_or("truncated entry")?;
        let extra_len = u16le(bytes, off + 30).ok_or("truncated entry")?;
        let comment_len = u16le(bytes, off + 32).ok_or("truncated entry")?;
        let name_off = off + 46;
        let name_bytes = bytes
            .get(name_off..name_off + name_len as usize)
            .ok_or("truncated entry name")?;
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        entries.push(ZipEntry {
            name,
            compressed_size: comp,
            uncompressed_size: uncomp,
            method,
        });
        off = name_off + name_len as usize + extra_len as usize + comment_len as usize;
    }
    Ok(entries)
}

/// Import a `.pptx` byte stream into placeholder pages.
pub fn import_pptx(file_name: &str, bytes: &[u8]) -> Result<PptxImport, String> {
    let entries = list_zip_entries(bytes)?;
    let mut slides: Vec<PptxSlidePlaceholder> = entries
        .iter()
        .filter_map(|e| {
            let rest = e.name.strip_prefix("ppt/slides/slide")?;
            let num: &str = rest.strip_suffix(".xml")?;
            // Skip slide layouts/masters (ppt/slides has only slides, but
            // be strict: the remainder must be a plain number).
            let number: u32 = num.parse().ok()?;
            Some(PptxSlidePlaceholder {
                number,
                part: e.name.clone(),
                note: format!(
                    "Slide {number} (placeholder — real rendering needs PowerPoint/COM or a PDF export)"
                ),
            })
        })
        .collect();
    slides.sort_by_key(|s| s.number);
    Ok(PptxImport {
        file_name: file_name.to_string(),
        entry_count: entries.len(),
        slides,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal stored-entry zip in memory (no compression).
    fn make_zip(names: &[&str]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        for name in names {
            let local_off = out.len() as u32;
            out.extend_from_slice(b"PK\x03\x04"); // local header sig
            out.extend_from_slice(&20u16.to_le_bytes()); // version
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&0u16.to_le_bytes()); // method = stored
            out.extend_from_slice(&0u16.to_le_bytes()); // time
            out.extend_from_slice(&0u16.to_le_bytes()); // date
            out.extend_from_slice(&0u32.to_le_bytes()); // crc (ignored)
            out.extend_from_slice(&0u32.to_le_bytes()); // comp size
            out.extend_from_slice(&0u32.to_le_bytes()); // uncomp size
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra len
            out.extend_from_slice(name.as_bytes());
            central.extend_from_slice(b"PK\x01\x02");
            central.extend_from_slice(&20u16.to_le_bytes()); // made by
            central.extend_from_slice(&20u16.to_le_bytes()); // needed
            central.extend_from_slice(&0u16.to_le_bytes()); // flags
            central.extend_from_slice(&0u16.to_le_bytes()); // method
            central.extend_from_slice(&0u16.to_le_bytes()); // time
            central.extend_from_slice(&0u16.to_le_bytes()); // date
            central.extend_from_slice(&0u32.to_le_bytes()); // crc
            central.extend_from_slice(&0u32.to_le_bytes()); // comp
            central.extend_from_slice(&0u32.to_le_bytes()); // uncomp
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes()); // extra
            central.extend_from_slice(&0u16.to_le_bytes()); // comment
            central.extend_from_slice(&0u16.to_le_bytes()); // disk
            central.extend_from_slice(&0u16.to_le_bytes()); // int attr
            central.extend_from_slice(&0u32.to_le_bytes()); // ext attr
            central.extend_from_slice(&local_off.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&central);
        let cd_len = central.len() as u32;
        out.extend_from_slice(b"PK\x05\x06"); // EOCD
        out.extend_from_slice(&0u16.to_le_bytes()); // disk
        out.extend_from_slice(&0u16.to_le_bytes()); // cd disk
        out.extend_from_slice(&(names.len() as u16).to_le_bytes());
        out.extend_from_slice(&(names.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_len.to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // comment len
        out
    }

    #[test]
    fn imports_slide_placeholders_in_order() {
        let zip = make_zip(&[
            "[Content_Types].xml",
            "ppt/slides/slide2.xml",
            "ppt/slides/slide1.xml",
            "ppt/slides/slide10.xml",
            "ppt/slideLayouts/slideLayout1.xml",
        ]);
        let imp = import_pptx("deck.pptx", &zip).unwrap();
        assert_eq!(imp.entry_count, 5);
        assert_eq!(imp.slides.len(), 3);
        assert_eq!(imp.slides[0].number, 1);
        assert_eq!(imp.slides[2].number, 10);
        assert!(imp.slides[0].note.contains("placeholder"));
    }

    #[test]
    fn rejects_non_zip_bytes() {
        assert!(import_pptx("x.pptx", b"definitely not a zip").is_err());
        assert!(import_pptx("x.pptx", b"").is_err());
    }

    #[test]
    fn zip_with_no_slides_imports_empty() {
        let zip = make_zip(&["[Content_Types].xml"]);
        let imp = import_pptx("empty.pptx", &zip).unwrap();
        assert!(imp.slides.is_empty());
    }
}
