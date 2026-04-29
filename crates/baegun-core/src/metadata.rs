use crate::mistral;
use crate::models::{BookMetadata, ConvertConfig, MistralOcrResponse};
use regex::Regex;
use std::collections::HashSet;

pub fn resolve_book_metadata(
    cfg: &ConvertConfig,
    pdf_bytes: &[u8],
    ocr_payload: &MistralOcrResponse,
) -> BookMetadata {
    let pdf_metadata = extract_pdf_metadata(pdf_bytes);
    let cover_metadata = infer_ocr_cover_metadata(ocr_payload);
    let needs_llm = missing_metadata(cfg, &pdf_metadata, &cover_metadata);
    let llm_metadata = if needs_llm {
        mistral::generate_book_metadata(cfg, ocr_payload).ok()
    } else {
        None
    };

    merge_metadata(
        cfg,
        pdf_metadata,
        cover_metadata,
        llm_metadata.unwrap_or_default(),
    )
}

fn missing_metadata(
    cfg: &ConvertConfig,
    pdf_metadata: &BookMetadata,
    cover_metadata: &BookMetadata,
) -> bool {
    missing_opt(cfg.title.as_deref())
        && pdf_metadata.title.is_none()
        && cover_metadata.title.is_none()
        || missing_opt(cfg.author.as_deref())
            && pdf_metadata.author.is_none()
            && cover_metadata.author.is_none()
        || missing_opt(cfg.publisher.as_deref()) && pdf_metadata.publisher.is_none()
        || cfg.language.trim().eq_ignore_ascii_case("en") && pdf_metadata.language.is_none()
        || pdf_metadata.description.is_none()
        || pdf_metadata.subjects.is_empty()
}

fn merge_metadata(
    cfg: &ConvertConfig,
    pdf_metadata: BookMetadata,
    cover_metadata: BookMetadata,
    llm_metadata: BookMetadata,
) -> BookMetadata {
    let mut subjects = Vec::new();
    push_subjects(&mut subjects, pdf_metadata.subjects);
    push_subjects(&mut subjects, llm_metadata.subjects);

    BookMetadata {
        title: first_non_empty([
            cfg.title.clone(),
            cover_metadata.title,
            pdf_metadata.title,
            llm_metadata.title,
        ]),
        author: first_non_empty([
            cfg.author.clone(),
            cover_metadata.author,
            pdf_metadata.author,
            llm_metadata.author,
        ]),
        language: resolve_language(cfg, pdf_metadata.language, llm_metadata.language),
        publisher: first_non_empty([
            cfg.publisher.clone(),
            pdf_metadata.publisher,
            llm_metadata.publisher,
        ]),
        description: first_non_empty([pdf_metadata.description, llm_metadata.description]),
        subjects,
    }
}

fn resolve_language(
    cfg: &ConvertConfig,
    pdf_language: Option<String>,
    llm_language: Option<String>,
) -> Option<String> {
    let configured = clean_metadata_value(&cfg.language);
    if configured
        .as_deref()
        .is_some_and(|language| !language.eq_ignore_ascii_case("en"))
    {
        return configured;
    }

    first_non_empty([pdf_language, llm_language, configured])
}

fn extract_pdf_metadata(pdf_bytes: &[u8]) -> BookMetadata {
    let pdf_text = String::from_utf8_lossy(pdf_bytes);
    let mut metadata = BookMetadata {
        title: find_pdf_info_string(&pdf_text, "Title")
            .or_else(|| find_xml_text(&pdf_text, "dc:title")),
        author: find_pdf_info_string(&pdf_text, "Author")
            .or_else(|| find_xml_text(&pdf_text, "dc:creator")),
        language: find_pdf_info_string(&pdf_text, "Lang")
            .or_else(|| find_xml_text(&pdf_text, "dc:language")),
        publisher: find_pdf_info_string(&pdf_text, "Publisher")
            .or_else(|| find_xml_text(&pdf_text, "dc:publisher")),
        description: find_pdf_info_string(&pdf_text, "Subject")
            .or_else(|| find_xml_text(&pdf_text, "dc:description")),
        subjects: Vec::new(),
    };

    if let Some(keywords) = find_pdf_info_string(&pdf_text, "Keywords")
        .or_else(|| find_xml_text(&pdf_text, "pdf:Keywords"))
    {
        push_subjects(&mut metadata.subjects, split_keywords(&keywords));
    }

    push_subjects(
        &mut metadata.subjects,
        find_xml_bag_items(&pdf_text, "dc:subject"),
    );
    metadata
}

fn infer_ocr_cover_metadata(ocr_payload: &MistralOcrResponse) -> BookMetadata {
    let lines = cover_candidate_lines(ocr_payload);
    let title = infer_cover_title(&lines);
    let author = infer_cover_author(&lines, title.as_deref());

    BookMetadata {
        title,
        author,
        ..BookMetadata::default()
    }
}

fn cover_candidate_lines(ocr_payload: &MistralOcrResponse) -> Vec<String> {
    let mut pages = ocr_payload.pages.iter().collect::<Vec<_>>();
    pages.sort_by_key(|page| page.index);

    pages
        .into_iter()
        .take(1)
        .flat_map(|page| page.markdown.lines())
        .filter_map(clean_cover_line)
        .take(24)
        .collect()
}

fn infer_cover_title(lines: &[String]) -> Option<String> {
    lines
        .iter()
        .find(|line| !is_author_marker_line(line) && !is_cover_noise(line))
        .and_then(|line| clean_metadata_value(line))
}

fn infer_cover_author(lines: &[String], title: Option<&str>) -> Option<String> {
    if let Some(author) = lines.iter().find_map(|line| author_from_marker_line(line)) {
        return Some(author);
    }

    let start = title
        .and_then(|title| lines.iter().position(|line| line == title))
        .map(|index| index + 1)
        .unwrap_or(1);

    lines
        .iter()
        .skip(start)
        .take(6)
        .find(|line| looks_like_person_name(line))
        .and_then(|line| clean_metadata_value(line))
}

fn clean_cover_line(raw: &str) -> Option<String> {
    let mut line = raw.trim();
    if line.is_empty() || line.starts_with("![") || line.starts_with('|') {
        return None;
    }

    while let Some(stripped) = line.strip_prefix('#') {
        line = stripped.trim_start();
    }

    line = line
        .trim_start_matches(['>', '-', '*', '+'])
        .trim()
        .trim_matches(['*', '_', '`'])
        .trim();

    if let Some(link_text) = markdown_link_text(line) {
        line = link_text;
    }

    if line.starts_with('<') && line.ends_with('>') || line.contains("</") {
        return None;
    }

    clean_metadata_value(line).filter(|value| !is_cover_noise(value))
}

fn markdown_link_text(line: &str) -> Option<&str> {
    let close = line.strip_prefix('[')?.find(']')? + 1;
    let after = line.get(close + 1..)?.trim_start();
    after.starts_with('(').then_some(line.get(1..close)?)
}

fn author_from_marker_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let lowered = trimmed.to_ascii_lowercase();
    for marker in [
        "by ",
        "author:",
        "written by ",
        "edited by ",
        "translated by ",
    ] {
        if lowered.starts_with(marker) {
            let author = trimmed.get(marker.len()..)?.trim();
            return clean_metadata_value(author);
        }
    }

    None
}

fn is_author_marker_line(line: &str) -> bool {
    author_from_marker_line(line).is_some()
}

fn looks_like_person_name(line: &str) -> bool {
    if is_cover_noise(line) || line.contains(':') || line.ends_with('?') || line.ends_with('!') {
        return false;
    }

    let words = line.split_whitespace().collect::<Vec<_>>();
    if !(2..=6).contains(&words.len()) {
        return false;
    }

    words.iter().all(|word| {
        let cleaned = word.trim_matches(|ch: char| matches!(ch, ',' | '.' | '-' | '’' | '\''));
        if cleaned.is_empty() || cleaned.chars().any(|ch| ch.is_ascii_digit()) {
            return false;
        }

        let lower = cleaned.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "de" | "del" | "da" | "van" | "von" | "la" | "le"
        ) {
            return true;
        }

        cleaned
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase())
            || cleaned.chars().all(|ch| ch.is_ascii_uppercase())
    })
}

fn is_cover_noise(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    if lower.len() <= 2 && lower.chars().all(|ch| ch.is_ascii_digit()) {
        return true;
    }

    lower.starts_with("isbn")
        || lower.starts_with("chapter ")
        || lower == "contents"
        || lower == "table of contents"
        || lower.contains("all rights reserved")
        || lower.contains("copyright")
        || lower.contains('\u{a9}')
        || lower.contains("www.")
        || lower.contains("http://")
        || lower.contains("https://")
}

fn find_pdf_info_string(pdf_text: &str, key: &str) -> Option<String> {
    let pattern = format!(r"/{}\s*(\((?:\\.|[^\\)])*\)|<[^>]+>)", regex::escape(key));
    let regex = Regex::new(&pattern).ok()?;
    let captures = regex.captures(pdf_text)?;
    decode_pdf_string(captures.get(1)?.as_str()).and_then(|value| clean_metadata_value(&value))
}

fn decode_pdf_string(raw: &str) -> Option<String> {
    if raw.starts_with('(') && raw.ends_with(')') {
        return Some(decode_pdf_literal_string(
            &raw[1..raw.len().saturating_sub(1)],
        ));
    }

    if raw.starts_with('<') && raw.ends_with('>') {
        return decode_pdf_hex_string(&raw[1..raw.len().saturating_sub(1)]);
    }

    None
}

fn decode_pdf_literal_string(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'\\' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }

        index += 1;
        if index >= bytes.len() {
            break;
        }

        match bytes[index] {
            b'n' => decoded.push(b'\n'),
            b'r' => decoded.push(b'\r'),
            b't' => decoded.push(b'\t'),
            b'b' => decoded.push(8),
            b'f' => decoded.push(12),
            b'(' | b')' | b'\\' => decoded.push(bytes[index]),
            b'\r' | b'\n' => {
                while index + 1 < bytes.len() && matches!(bytes[index + 1], b'\r' | b'\n') {
                    index += 1;
                }
            }
            b'0'..=b'7' => {
                let start = index;
                while index + 1 < bytes.len()
                    && index + 1 - start < 3
                    && matches!(bytes[index + 1], b'0'..=b'7')
                {
                    index += 1;
                }
                if let Ok(value) = u8::from_str_radix(&raw[start..=index], 8) {
                    decoded.push(value);
                }
            }
            other => decoded.push(other),
        }

        index += 1;
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

fn decode_pdf_hex_string(raw: &str) -> Option<String> {
    let mut hex = raw
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>();
    if hex.len() % 2 == 1 {
        hex.push('0');
    }

    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for chunk in hex.as_bytes().chunks(2) {
        let pair = std::str::from_utf8(chunk).ok()?;
        bytes.push(u8::from_str_radix(pair, 16).ok()?);
    }

    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16(&units).ok();
    }

    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn find_xml_text(document: &str, tag: &str) -> Option<String> {
    let pattern = format!(
        r"(?s)<{}(?:\s[^>]*)?>\s*(?:(?:<rdf:(?:Alt|Seq|Bag)(?:\s[^>]*)?>\s*)?<rdf:li(?:\s[^>]*)?>)?\s*([^<]+)",
        regex::escape(tag)
    );
    let regex = Regex::new(&pattern).ok()?;
    let captures = regex.captures(document)?;
    clean_metadata_value(&xml_unescape(captures.get(1)?.as_str()))
}

fn find_xml_bag_items(document: &str, tag: &str) -> Vec<String> {
    let section_pattern = format!(
        r"(?s)<{}(?:\s[^>]*)?>(.*?)</{}>",
        regex::escape(tag),
        regex::escape(tag)
    );
    let Some(section_regex) = Regex::new(&section_pattern).ok() else {
        return Vec::new();
    };
    let Some(section) = section_regex
        .captures(document)
        .and_then(|captures| captures.get(1).map(|value| value.as_str()))
    else {
        return Vec::new();
    };
    let Some(item_regex) = Regex::new(r"(?s)<rdf:li(?:\s[^>]*)?>(.*?)</rdf:li>").ok() else {
        return Vec::new();
    };

    item_regex
        .captures_iter(section)
        .filter_map(|captures| captures.get(1))
        .filter_map(|value| clean_metadata_value(&xml_unescape(value.as_str())))
        .collect()
}

fn split_keywords(value: &str) -> Vec<String> {
    value
        .split([',', ';', '\n'])
        .filter_map(clean_metadata_value)
        .collect()
}

fn push_subjects(subjects: &mut Vec<String>, new_subjects: Vec<String>) {
    let mut seen = subjects
        .iter()
        .map(|subject| subject.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    for subject in new_subjects {
        if let Some(cleaned) = clean_metadata_value(&subject) {
            if seen.insert(cleaned.to_ascii_lowercase()) {
                subjects.push(cleaned);
            }
        }
    }
}

fn first_non_empty<const N: usize>(values: [Option<String>; N]) -> Option<String> {
    values
        .into_iter()
        .find_map(|value| value.and_then(|value| clean_metadata_value(&value)))
}

fn missing_opt(value: Option<&str>) -> bool {
    value.and_then(clean_metadata_value).is_none()
}

fn clean_metadata_value(value: &str) -> Option<String> {
    let cleaned = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("unknown") {
        None
    } else {
        Some(cleaned)
    }
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::{decode_pdf_string, extract_pdf_metadata, infer_ocr_cover_metadata};
    use crate::models::{MistralOcrResponse, OcrPage};

    #[test]
    fn extracts_pdf_info_metadata() {
        let pdf = br#"%PDF-1.4
1 0 obj
<< /Title (Example Book) /Author (Ada Lovelace) /Subject (A compact description) /Keywords (math, computing; history) /Lang (en-US) >>
endobj
"#;

        let metadata = extract_pdf_metadata(pdf);
        assert_eq!(metadata.title.as_deref(), Some("Example Book"));
        assert_eq!(metadata.author.as_deref(), Some("Ada Lovelace"));
        assert_eq!(
            metadata.description.as_deref(),
            Some("A compact description")
        );
        assert_eq!(metadata.language.as_deref(), Some("en-US"));
        assert_eq!(metadata.subjects, vec!["math", "computing", "history"]);
    }

    #[test]
    fn decodes_utf16_pdf_hex_string() {
        let decoded = decode_pdf_string("<FEFF00420061006500670075006E>");
        assert_eq!(decoded.as_deref(), Some("Baegun"));
    }

    #[test]
    fn infers_title_and_author_from_ocr_cover_page() {
        let metadata = infer_ocr_cover_metadata(&ocr_payload(
            "# The Analytical Engine\n\nby Ada Lovelace\n\n![Cover](cover.png)",
        ));

        assert_eq!(metadata.title.as_deref(), Some("The Analytical Engine"));
        assert_eq!(metadata.author.as_deref(), Some("Ada Lovelace"));
    }

    #[test]
    fn infers_author_from_name_after_cover_title() {
        let metadata =
            infer_ocr_cover_metadata(&ocr_payload("Deep Work\n\nCal Newport\n\nCopyright 2016"));

        assert_eq!(metadata.title.as_deref(), Some("Deep Work"));
        assert_eq!(metadata.author.as_deref(), Some("Cal Newport"));
    }

    fn ocr_payload(markdown: &str) -> MistralOcrResponse {
        MistralOcrResponse {
            pages: vec![OcrPage {
                index: 0,
                markdown: markdown.to_string(),
                images: Vec::new(),
                tables: Vec::new(),
                hyperlinks: Vec::new(),
                header: None,
                footer: None,
                dimensions: None,
            }],
            model: None,
            usage_info: None,
        }
    }
}
