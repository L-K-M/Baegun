use crate::errors::{BaegunError, Result};
use crate::models::{
    BookMetadata, ConvertConfig, ImageAsset, MistralOcrResponse, OcrPage, OcrTable,
    PageProgressionDirection, RenderedBook, RenderedChapter, TableFormat,
};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use pulldown_cmark::{html, Options, Parser};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn normalize_to_rendered_book(
    payload: &MistralOcrResponse,
    cfg: &ConvertConfig,
    metadata: &BookMetadata,
    source_pdf: &Path,
    source_hash: &str,
) -> Result<RenderedBook> {
    let mut pages = payload.pages.clone();
    pages.sort_by_key(|page| page.index);

    if cfg.comic_mode {
        return normalize_comic_to_rendered_book(&pages, cfg, metadata, source_pdf, source_hash);
    }

    let extracted_images = extract_images(&pages, cfg.include_images)?;
    let language = metadata
        .language
        .clone()
        .unwrap_or_else(|| cfg.language.clone());

    let mut page_markdown = Vec::new();
    for page in &pages {
        let mut markdown = page.markdown.clone();

        markdown = strip_header_footer(markdown, page.header.as_deref(), page.footer.as_deref());
        markdown = replace_table_placeholders(markdown, page, cfg.table_format);

        if cfg.include_images {
            markdown = replace_image_placeholders(markdown, &extracted_images.image_map);
        } else {
            markdown = strip_markdown_images(markdown);
        }

        let trimmed = markdown.trim();
        if !trimmed.is_empty() {
            page_markdown.push(trimmed.to_owned());
        }
    }

    let full_markdown = page_markdown.join("\n\n");
    let chapter_drafts = split_into_chapters(&full_markdown);

    if chapter_drafts.is_empty() {
        return Err(BaegunError::ocr_schema(
            "No readable content found after OCR normalization.",
        ));
    }

    let mut slug_counts: HashMap<String, usize> = HashMap::new();
    let mut chapters = Vec::with_capacity(chapter_drafts.len());

    for (index, (title, markdown)) in chapter_drafts.into_iter().enumerate() {
        let base_slug = slugify(&title);
        let counter = slug_counts.entry(base_slug.clone()).or_insert(0);
        *counter += 1;
        let unique_slug = if *counter == 1 {
            base_slug.clone()
        } else {
            format!("{base_slug}-{}", *counter)
        };

        let file_name = format!("chapter-{:03}-{unique_slug}.xhtml", index + 1);
        let html_fragment = render_markdown_to_html(&markdown);
        let xhtml = wrap_xhtml_document(&title, &language, &html_fragment);

        chapters.push(RenderedChapter {
            id: format!("chapter-{:03}", index + 1),
            title,
            file_name,
            markdown,
            xhtml,
        });
    }

    let title = cfg
        .title
        .clone()
        .or_else(|| metadata.title.clone())
        .or_else(|| chapters.first().map(|chapter| chapter.title.clone()))
        .unwrap_or_else(|| sanitize_file_stem(source_pdf));

    Ok(RenderedBook {
        title,
        author: cfg.author.clone().or_else(|| metadata.author.clone()),
        language,
        publisher: cfg.publisher.clone().or_else(|| metadata.publisher.clone()),
        description: metadata.description.clone(),
        subjects: metadata.subjects.clone(),
        source_hash: source_hash.to_owned(),
        chapters,
        images: extracted_images.images,
        cover_image: extracted_images.cover_image,
        fixed_layout: false,
        page_progression_direction: PageProgressionDirection::LeftToRight,
    })
}

fn normalize_comic_to_rendered_book(
    pages: &[OcrPage],
    cfg: &ConvertConfig,
    metadata: &BookMetadata,
    source_pdf: &Path,
    source_hash: &str,
) -> Result<RenderedBook> {
    let extracted_images = extract_images(pages, true)?;
    let language = metadata
        .language
        .clone()
        .unwrap_or_else(|| cfg.language.clone());

    if extracted_images.images.is_empty() {
        return Err(BaegunError::ocr_schema(
            "Comic mode requires OCR image payloads, but no images were found.",
        ));
    }

    let mut chapters = Vec::new();
    for page in pages {
        let Some(image_path) = resolve_comic_page_image_path(page, &extracted_images.image_map)
        else {
            continue;
        };

        let page_number = page.index + 1;
        let title = format!("Page {page_number}");
        let chapter_number = chapters.len() + 1;

        chapters.push(RenderedChapter {
            id: format!("chapter-{chapter_number:03}"),
            title: title.clone(),
            file_name: format!("chapter-{chapter_number:03}-page-{page_number:03}.xhtml"),
            markdown: format!("![{title}]({image_path})"),
            xhtml: render_comic_page_xhtml(&title, &language, &image_path),
        });
    }

    if chapters.is_empty() {
        return Err(BaegunError::ocr_schema(
            "Comic mode could not map any page images from OCR output.",
        ));
    }

    let title = cfg
        .title
        .clone()
        .or_else(|| metadata.title.clone())
        .unwrap_or_else(|| sanitize_file_stem(source_pdf));

    Ok(RenderedBook {
        title,
        author: cfg.author.clone().or_else(|| metadata.author.clone()),
        language,
        publisher: cfg.publisher.clone().or_else(|| metadata.publisher.clone()),
        description: metadata.description.clone(),
        subjects: metadata.subjects.clone(),
        source_hash: source_hash.to_owned(),
        chapters,
        images: extracted_images.images,
        cover_image: extracted_images.cover_image,
        fixed_layout: false,
        page_progression_direction: PageProgressionDirection::LeftToRight,
    })
}

struct ExtractedImages {
    images: Vec<ImageAsset>,
    image_map: HashMap<String, String>,
    cover_image: Option<String>,
}

fn extract_images(pages: &[OcrPage], include_images: bool) -> Result<ExtractedImages> {
    let first_page_index = pages.first().map(|page| page.index);

    let mut images = Vec::new();
    let mut image_map = HashMap::new();
    let mut cover_image = None;
    let mut seen_placeholders = HashSet::new();
    let mut used_filenames = HashSet::new();

    for page in pages {
        for (index, image) in page.images.iter().enumerate() {
            if seen_placeholders.contains(&image.id) {
                continue;
            }

            let Some(encoded) = image.image_base64.as_deref() else {
                continue;
            };

            let is_cover_candidate = cover_image.is_none() && Some(page.index) == first_page_index;
            if !include_images && !is_cover_candidate {
                continue;
            }

            let bytes = decode_ocr_image_base64(encoded).map_err(|error| {
                BaegunError::ocr_schema(format!(
                    "Failed decoding OCR image '{}' as base64: {error}",
                    image.id
                ))
            })?;

            let file_name = unique_asset_name(&image.id, page.index, index, &mut used_filenames);
            let media_type = media_type_from_filename(&file_name).to_owned();

            if include_images {
                image_map.insert(image.id.clone(), format!("../images/{file_name}"));
            }
            if is_cover_candidate {
                cover_image = Some(file_name.clone());
            }
            seen_placeholders.insert(image.id.clone());

            images.push(ImageAsset {
                file_name,
                media_type,
                bytes,
            });
        }
    }

    Ok(ExtractedImages {
        images,
        image_map,
        cover_image,
    })
}

fn decode_ocr_image_base64(encoded: &str) -> std::result::Result<Vec<u8>, base64::DecodeError> {
    let trimmed = encoded.trim();

    let payload = if let Some((metadata, data)) = trimmed.split_once(',') {
        let lowered_metadata = metadata.to_ascii_lowercase();
        if lowered_metadata.starts_with("data:") && lowered_metadata.contains(";base64") {
            data
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    if payload.bytes().any(|byte| byte.is_ascii_whitespace()) {
        let compact = payload
            .chars()
            .filter(|ch| !ch.is_ascii_whitespace())
            .collect::<String>();
        BASE64.decode(compact.as_bytes())
    } else {
        BASE64.decode(payload)
    }
}

fn strip_header_footer(mut markdown: String, header: Option<&str>, footer: Option<&str>) -> String {
    if let Some(header_text) = header.map(str::trim).filter(|value| !value.is_empty()) {
        let trimmed_start = markdown.trim_start().to_owned();
        if trimmed_start.starts_with(header_text) {
            markdown = trimmed_start
                .strip_prefix(header_text)
                .unwrap_or(&trimmed_start)
                .trim_start_matches('\n')
                .to_owned();
        }
    }

    if let Some(footer_text) = footer.map(str::trim).filter(|value| !value.is_empty()) {
        let trimmed_end = markdown.trim_end().to_owned();
        if trimmed_end.ends_with(footer_text) {
            markdown = trimmed_end
                .strip_suffix(footer_text)
                .unwrap_or(&trimmed_end)
                .trim_end_matches('\n')
                .to_owned();
        }
    }

    markdown
}

fn replace_table_placeholders(
    mut markdown: String,
    page: &OcrPage,
    table_format: TableFormat,
) -> String {
    for (index, table) in page.tables.iter().enumerate() {
        let replacement = match table_format {
            TableFormat::Html => table
                .html
                .clone()
                .or_else(|| table.content.clone())
                .or_else(|| table.markdown.clone()),
            TableFormat::Markdown => table
                .markdown
                .clone()
                .or_else(|| table.content.clone())
                .or_else(|| table.html.clone()),
        };

        let Some(content) = replacement.filter(|value| !value.trim().is_empty()) else {
            continue;
        };

        let identifiers = collect_table_identifiers(table, page.index, index);
        for identifier in identifiers {
            markdown = replace_table_reference(markdown, &identifier, &content);
        }
    }

    markdown
}

fn collect_table_identifiers(table: &OcrTable, page_index: usize, index: usize) -> Vec<String> {
    let mut identifiers = Vec::new();

    let mut push_unique = |value: Option<&str>| {
        let Some(raw) = value else {
            return;
        };

        let candidate = raw.trim();
        if candidate.is_empty() {
            return;
        }

        if !identifiers.iter().any(|existing| existing == candidate) {
            identifiers.push(candidate.to_owned());
        }
    };

    push_unique(table.id.as_deref());
    push_unique(table.extra.get("table_id").and_then(|value| value.as_str()));
    push_unique(table.extra.get("id").and_then(|value| value.as_str()));
    push_unique(
        table
            .extra
            .get("file_name")
            .and_then(|value| value.as_str()),
    );
    push_unique(table.extra.get("path").and_then(|value| value.as_str()));
    push_unique(
        table
            .extra
            .get("placeholder")
            .and_then(|value| value.as_str()),
    );

    let fallback = format!("tbl-{page_index}-{index}.html");
    if !identifiers.iter().any(|existing| existing == &fallback) {
        identifiers.push(fallback);
    }

    identifiers
}

fn replace_table_reference(markdown: String, table_id: &str, content: &str) -> String {
    let escaped_id = regex::escape(table_id);
    let replacement = format!("\n{content}\n");

    let patterns = [
        format!(
            r#"!?\[[^\]]*\]\(\s*(?:\./)?{escaped_id}(?:#[^\s\)"']+)?(?:\s+(?:"[^"]*"|'[^']*'))?\s*\)"#
        ),
        format!(r"<\s*(?:\./)?{escaped_id}\s*>"),
        format!(r"(?m)^\s*\[\s*(?:\./)?{escaped_id}\s*\]\s*$"),
        format!(r"(?m)^\s*(?:\./)?{escaped_id}\s*$"),
    ];

    let mut updated = markdown;
    for pattern in patterns {
        if let Ok(regex) = Regex::new(&pattern) {
            updated = regex
                .replace_all(&updated, replacement.as_str())
                .to_string();
        }
    }

    updated
}

fn replace_image_placeholders(mut markdown: String, image_map: &HashMap<String, String>) -> String {
    for (placeholder, relative_path) in image_map {
        markdown = markdown.replace(&format!("]({placeholder})"), &format!("]({relative_path})"));
    }
    markdown
}

fn strip_markdown_images(markdown: String) -> String {
    let Ok(regex) = Regex::new(r"!\[[^\]]*\]\([^\)]+\)") else {
        return markdown;
    };
    regex.replace_all(&markdown, "").to_string()
}

fn resolve_comic_page_image_path(
    page: &OcrPage,
    image_map: &HashMap<String, String>,
) -> Option<String> {
    for image in &page.images {
        if let Some(mapped) = image_map.get(&image.id) {
            return Some(mapped.clone());
        }
    }

    let first_markdown_target = first_markdown_image_target(&page.markdown)?;
    image_map.get(&first_markdown_target).cloned().or_else(|| {
        if first_markdown_target.starts_with("../images/") {
            Some(first_markdown_target)
        } else {
            None
        }
    })
}

fn first_markdown_image_target(markdown: &str) -> Option<String> {
    let Ok(image_re) = Regex::new(r"!\[[^\]]*\]\(([^\s\)]+)") else {
        return None;
    };

    image_re
        .captures(markdown)
        .and_then(|captures| captures.get(1))
        .map(|capture| capture.as_str().trim().to_owned())
        .filter(|target| !target.is_empty())
}

fn split_into_chapters(markdown: &str) -> Vec<(String, String)> {
    if markdown.trim().is_empty() {
        return Vec::new();
    }

    let lines: Vec<&str> = markdown.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let heading_candidates = collect_heading_candidates(&lines);
    let mut boundaries = choose_split_boundaries(&heading_candidates);

    if boundaries.is_empty() {
        boundaries = collect_chapter_line_boundaries(&lines);
    }

    if boundaries.is_empty() {
        return vec![(String::from("Document"), markdown.trim().to_owned())];
    }

    boundaries.sort_by_key(|boundary| boundary.start_line);
    boundaries.dedup_by_key(|boundary| boundary.start_line);

    let mut chapters: Vec<(String, String)> = Vec::new();
    let mut current_title = String::from("Introduction");
    let mut cursor = 0_usize;

    for boundary in &boundaries {
        if boundary.start_line > cursor {
            let chunk = join_lines_trimmed(&lines[cursor..boundary.start_line]);
            if !chunk.is_empty() {
                chapters.push((current_title.clone(), chunk));
            }
        }

        current_title = boundary.title.clone();
        cursor = boundary.start_line;
    }

    if cursor < lines.len() {
        let chunk = join_lines_trimmed(&lines[cursor..]);
        if !chunk.is_empty() {
            chapters.push((current_title, chunk));
        }
    }

    if chapters.is_empty() {
        return vec![(String::from("Document"), markdown.trim().to_owned())];
    }

    if chapters.len() >= 3 {
        let trailing_chars = chapters
            .last()
            .map(|(_, chunk)| chunk.chars().count())
            .unwrap_or(0);
        if trailing_chars < 400 {
            if let Some((_, trailing)) = chapters.pop() {
                if let Some((_, previous)) = chapters.last_mut() {
                    previous.push_str("\n\n");
                    previous.push_str(&trailing);
                }
            }
        }
    }

    chapters
}

#[derive(Debug, Clone)]
struct ChapterBoundary {
    start_line: usize,
    level: u8,
    title: String,
}

fn collect_heading_candidates(lines: &[&str]) -> Vec<ChapterBoundary> {
    let mut boundaries = Vec::new();

    let heading_re = Regex::new(r"^\s{0,3}(#{1,6})\s+(.+?)\s*#*\s*$").ok();
    let setext_re = Regex::new(r"^\s{0,3}(=+|-+)\s*$").ok();

    let mut index = 0_usize;
    while index < lines.len() {
        let line = lines[index];

        if let Some(regex) = &heading_re {
            if let Some(captures) = regex.captures(line) {
                let level = captures
                    .get(1)
                    .map(|value| value.as_str().len() as u8)
                    .unwrap_or(1);
                let title = captures
                    .get(2)
                    .map(|value| sanitize_heading_title(value.as_str()))
                    .unwrap_or_default();
                if !title.is_empty() {
                    boundaries.push(ChapterBoundary {
                        start_line: index,
                        level,
                        title,
                    });
                    index += 1;
                    continue;
                }
            }
        }

        if index + 1 < lines.len() {
            let current = lines[index].trim();
            if !current.is_empty() {
                if let Some(regex) = &setext_re {
                    if let Some(captures) = regex.captures(lines[index + 1]) {
                        let marker = captures.get(1).map(|value| value.as_str()).unwrap_or("");
                        let level = if marker.starts_with('=') { 1 } else { 2 };
                        let title = sanitize_heading_title(current);
                        if !title.is_empty() {
                            boundaries.push(ChapterBoundary {
                                start_line: index,
                                level,
                                title,
                            });
                            index += 2;
                            continue;
                        }
                    }
                }
            }
        }

        index += 1;
    }

    boundaries
}

fn choose_split_boundaries(candidates: &[ChapterBoundary]) -> Vec<ChapterBoundary> {
    if candidates.is_empty() {
        return Vec::new();
    }

    if candidates.iter().any(|boundary| boundary.level == 1) {
        return candidates
            .iter()
            .filter(|boundary| boundary.level == 1)
            .cloned()
            .collect();
    }

    let level_two_count = candidates
        .iter()
        .filter(|boundary| boundary.level == 2)
        .count();
    if level_two_count >= 2 {
        return candidates
            .iter()
            .filter(|boundary| boundary.level == 2)
            .cloned()
            .collect();
    }

    Vec::new()
}

fn collect_chapter_line_boundaries(lines: &[&str]) -> Vec<ChapterBoundary> {
    let chapter_line_re = Regex::new(r"(?i)^\s*(chapter|part)\s+([0-9ivxlcdm]+)\b.*$").ok();
    let Some(regex) = chapter_line_re else {
        return Vec::new();
    };

    let mut boundaries = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.chars().count() > 90 {
            continue;
        }

        if !regex.is_match(trimmed) {
            continue;
        }

        let prev_is_blank = index == 0 || lines[index - 1].trim().is_empty();
        if !prev_is_blank {
            continue;
        }

        boundaries.push(ChapterBoundary {
            start_line: index,
            level: 1,
            title: sanitize_heading_title(trimmed),
        });
    }

    if boundaries.len() >= 2 {
        boundaries
    } else {
        Vec::new()
    }
}

fn sanitize_heading_title(value: &str) -> String {
    value.trim().trim_matches('#').trim().to_owned()
}

fn join_lines_trimmed(lines: &[&str]) -> String {
    lines.join("\n").trim().to_owned()
}

fn render_markdown_to_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    let html_output = html_output
        .replace("<blockquote>", "<aside class=\"callout\">")
        .replace("</blockquote>", "</aside>");

    let html_output = make_xhtml_compatible(&html_output);

    add_heading_anchors(html_output)
}

/// Best-effort hardening of a rendered HTML fragment toward well-formed XHTML.
///
/// `pulldown-cmark` passes raw inline/block HTML straight through, and OCR table
/// payloads are injected verbatim before rendering. EPUB chapter documents must be
/// well-formed XML, so unclosed void elements (`<br>`, `<img>`) and bare ampersands
/// coming from that raw HTML would otherwise make the chapter invalid and fail
/// epubcheck. This fixes the two failure modes seen most often in real OCR output.
/// It is not a full HTML5 parser, so deeply malformed markup (e.g. unclosed
/// `<td>`) is still out of scope.
fn make_xhtml_compatible(html: &str) -> String {
    let closed = close_void_elements(html);
    escape_bare_ampersands(&closed)
}

fn close_void_elements(html: &str) -> String {
    // Self-close HTML void elements (with or without an existing trailing slash) so
    // the output is valid XML. Attribute values containing `>` are not handled, but
    // that essentially never occurs in OCR table markup.
    let Ok(regex) = Regex::new(
        r"(?i)<(br|hr|img|col|input|meta|link|area|base|source|track|wbr)([^>]*?)\s*/?>",
    ) else {
        return html.to_owned();
    };

    regex
        .replace_all(html, |caps: &regex::Captures<'_>| {
            let tag = caps[1].to_ascii_lowercase();
            let attrs = caps.get(2).map(|value| value.as_str().trim()).unwrap_or("");
            if attrs.is_empty() {
                format!("<{tag} />")
            } else {
                format!("<{tag} {attrs} />")
            }
        })
        .to_string()
}

fn escape_bare_ampersands(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(position) = rest.find('&') {
        output.push_str(&rest[..position]);
        let after = &rest[position + 1..];
        if is_entity_reference(after) {
            output.push('&');
        } else {
            output.push_str("&amp;");
        }
        rest = after;
    }

    output.push_str(rest);
    output
}

/// Returns true when `after` (the text immediately following an `&`) begins a valid
/// character reference: `name;`, `#123;`, or `#xAB;`.
fn is_entity_reference(after: &str) -> bool {
    let bytes = after.as_bytes();
    if bytes.is_empty() {
        return false;
    }

    let mut index = 0;
    if bytes[index] == b'#' {
        index += 1;
        if index < bytes.len() && matches!(bytes[index], b'x' | b'X') {
            index += 1;
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_hexdigit() {
                index += 1;
            }
            return index > start && bytes.get(index) == Some(&b';');
        }

        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        return index > start && bytes.get(index) == Some(&b';');
    }

    let start = index;
    while index < bytes.len() && bytes[index].is_ascii_alphanumeric() {
        index += 1;
    }
    index > start && bytes.get(index) == Some(&b';')
}

fn add_heading_anchors(input: String) -> String {
    let Ok(heading_re) = Regex::new(r"(?s)<h([1-6])>(.*?)</h[1-6]>") else {
        return input;
    };
    let Ok(tag_re) = Regex::new(r"<[^>]+>") else {
        return input;
    };

    let mut seen = HashMap::<String, usize>::new();
    heading_re
        .replace_all(&input, |caps: &regex::Captures<'_>| {
            let level = caps.get(1).map(|match_| match_.as_str()).unwrap_or("2");
            let inner_html = caps.get(2).map(|match_| match_.as_str()).unwrap_or("");
            let plain = tag_re.replace_all(inner_html, "").to_string();
            let slug_base = slugify(&plain);
            let counter = seen.entry(slug_base.clone()).or_insert(0);
            *counter += 1;
            let slug = if *counter == 1 {
                slug_base
            } else {
                format!("{}-{}", slug_base, *counter)
            };

            format!("<h{level} id=\"{slug}\">{inner_html}</h{level}>")
        })
        .to_string()
}

fn wrap_xhtml_document(title: &str, language: &str, body_html: &str) -> String {
    let escaped_title = xml_escape(title);
    let escaped_lang = xml_escape(language);
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"{escaped_lang}\">\n  <head>\n    <meta charset=\"utf-8\" />\n    <title>{escaped_title}</title>\n    <link rel=\"stylesheet\" type=\"text/css\" href=\"../styles/book.css\" />\n  </head>\n  <body class=\"chapter\">\n{body_html}\n  </body>\n</html>\n"
    )
}

fn render_comic_page_xhtml(title: &str, language: &str, image_src: &str) -> String {
    let escaped_title = xml_escape(title);
    let escaped_lang = xml_escape(language);
    let escaped_src = xml_escape(image_src);

    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"{escaped_lang}\">\n  <head>\n    <meta charset=\"utf-8\" />\n    <title>{escaped_title}</title>\n    <link rel=\"stylesheet\" type=\"text/css\" href=\"../styles/book.css\" />\n  </head>\n  <body class=\"chapter comic-page\">\n    <div class=\"comic-frame\">\n      <img src=\"{escaped_src}\" alt=\"{escaped_title}\" />\n    </div>\n  </body>\n</html>\n"
    )
}

fn unique_asset_name(
    original_id: &str,
    page_index: usize,
    index_in_page: usize,
    used: &mut HashSet<String>,
) -> String {
    let sanitized = sanitize_asset_name(original_id);
    if used.insert(sanitized.clone()) {
        return sanitized;
    }

    let stem = sanitized
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or("image");
    let ext = sanitized
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .unwrap_or("bin");

    let candidate = format!("{stem}-p{page_index:04}-{index_in_page:02}.{ext}");
    used.insert(candidate.clone());
    candidate
}

fn sanitize_asset_name(input: &str) -> String {
    let mut cleaned = input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();

    cleaned = cleaned.trim_matches('-').to_owned();
    if cleaned.is_empty() {
        return String::from("image.bin");
    }

    if cleaned.contains('.') {
        cleaned
    } else {
        format!("{cleaned}.bin")
    }
}

fn media_type_from_filename(filename: &str) -> &'static str {
    if let Some((_, ext)) = filename.rsplit_once('.') {
        match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            "bmp" => "image/bmp",
            _ => "application/octet-stream",
        }
    } else {
        "application/octet-stream"
    }
}

fn sanitize_file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| String::from("Document"))
}

fn slugify(value: &str) -> String {
    let mut slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();

    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }

    slug = slug.trim_matches('-').to_owned();
    if slug.is_empty() {
        String::from("chapter")
    } else {
        slug
    }
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::{
        decode_ocr_image_base64, make_xhtml_compatible, render_markdown_to_html,
        replace_table_placeholders, resolve_comic_page_image_path, split_into_chapters,
    };
    use crate::models::{OcrPage, OcrTable, TableFormat};
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn split_uses_h1_boundaries() {
        let markdown = "# One\nA\n\n# Two\nB";
        let chapters = split_into_chapters(markdown);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].0, "One");
        assert_eq!(chapters[1].0, "Two");
    }

    #[test]
    fn split_uses_h2_boundaries_when_no_h1_exists() {
        let markdown = "## First\nA\n\n## Second\nB";
        let chapters = split_into_chapters(markdown);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].0, "First");
        assert_eq!(chapters[1].0, "Second");
    }

    #[test]
    fn split_detects_setext_h1_boundaries() {
        let markdown = "One\n===\nBody A\n\nTwo\n===\nBody B";
        let chapters = split_into_chapters(markdown);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].0, "One");
        assert_eq!(chapters[1].0, "Two");
    }

    #[test]
    fn split_detects_chapter_style_lines() {
        let markdown = "CHAPTER 1\nBody A\n\nCHAPTER 2\nBody B";
        let chapters = split_into_chapters(markdown);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].0, "CHAPTER 1");
        assert_eq!(chapters[1].0, "CHAPTER 2");
    }

    #[test]
    fn table_replacement_handles_multiple_placeholder_shapes() {
        let page = OcrPage {
            index: 0,
            markdown: String::new(),
            images: Vec::new(),
            tables: vec![OcrTable {
                id: Some("table-main.html".to_string()),
                html: Some("<table><tr><td>X</td></tr></table>".to_string()),
                markdown: None,
                content: None,
                extra: [("table_id".to_string(), json!("table-main.html"))]
                    .into_iter()
                    .collect(),
            }],
            hyperlinks: Vec::new(),
            header: None,
            footer: None,
            dimensions: None,
        };

        let markdown = [
            "[table-main.html](table-main.html)",
            "[Table](./table-main.html \"Main table\")",
            "<table-main.html>",
            "table-main.html",
        ]
        .join("\n\n");

        let replaced = replace_table_placeholders(markdown, &page, TableFormat::Html);
        assert!(replaced.contains("<table><tr><td>X</td></tr></table>"));
        assert!(!replaced.contains("table-main.html)"));
        assert!(!replaced.contains("<table-main.html>"));
    }

    #[test]
    fn image_decoder_accepts_data_uri_base64_payload() {
        let decoded = decode_ocr_image_base64("data:image/jpeg;base64,dGVzdA==")
            .expect("data uri payload should decode");
        assert_eq!(decoded, b"test");
    }

    #[test]
    fn image_decoder_ignores_embedded_whitespace() {
        let decoded = decode_ocr_image_base64("data:image/png;base64,dG Vz\ndA==")
            .expect("payload with whitespace should decode");
        assert_eq!(decoded, b"test");
    }

    #[test]
    fn raw_table_html_is_made_xhtml_compatible() {
        // Raw HTML blocks (e.g. OCR table payloads) pass straight through the markdown
        // renderer, so void elements and bare ampersands must be normalized for XHTML.
        let rendered = render_markdown_to_html(
            "<table><tr><td>AT&T<br></td><td><img src=\"a.png\"></td></tr></table>",
        );

        assert!(rendered.contains("AT&amp;T"));
        assert!(rendered.contains("<br />"));
        assert!(rendered.contains("<img src=\"a.png\" />"));
        assert!(!rendered.contains("<br>"));
    }

    #[test]
    fn existing_entities_and_self_closed_tags_are_preserved() {
        let input = "<p>a &amp; b &#160; c &#x41; <br /> <img src=\"x.png\" /></p>";
        let normalized = make_xhtml_compatible(input);

        assert!(normalized.contains("&amp;"));
        assert!(normalized.contains("&#160;"));
        assert!(normalized.contains("&#x41;"));
        assert!(!normalized.contains("&amp;amp;"));
        assert!(!normalized.contains("<br /> />"));
        assert_eq!(normalized.matches("<br />").count(), 1);
        assert_eq!(normalized.matches("<img src=\"x.png\" />").count(), 1);
    }

    #[test]
    fn comic_image_resolution_prefers_page_image_ids() {
        let page = OcrPage {
            index: 0,
            markdown: String::from("![Page](img-1.png)"),
            images: vec![crate::models::OcrImage {
                id: String::from("img-1.png"),
                image_base64: Some(String::from("AAA=")),
                extra: HashMap::new(),
            }],
            tables: Vec::new(),
            hyperlinks: Vec::new(),
            header: None,
            footer: None,
            dimensions: None,
        };

        let map = HashMap::from([(
            String::from("img-1.png"),
            String::from("../images/img-1.png"),
        )]);
        let resolved = resolve_comic_page_image_path(&page, &map);
        assert_eq!(resolved.as_deref(), Some("../images/img-1.png"));
    }

    #[test]
    fn comic_image_resolution_falls_back_to_markdown_reference() {
        let page = OcrPage {
            index: 1,
            markdown: String::from("![Page](../images/page-002.jpg)"),
            images: Vec::new(),
            tables: Vec::new(),
            hyperlinks: Vec::new(),
            header: None,
            footer: None,
            dimensions: None,
        };

        let resolved = resolve_comic_page_image_path(&page, &HashMap::new());
        assert_eq!(resolved.as_deref(), Some("../images/page-002.jpg"));
    }
}
