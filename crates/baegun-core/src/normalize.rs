use crate::errors::{BaegunError, Result};
use crate::models::{
    ConvertConfig, ImageAsset, MistralOcrResponse, OcrPage, RenderedBook, RenderedChapter, TableFormat,
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
    source_pdf: &Path,
    source_hash: &str,
) -> Result<RenderedBook> {
    let mut pages = payload.pages.clone();
    pages.sort_by_key(|page| page.index);

    let (images, image_map) = extract_images(&pages, cfg.include_images)?;

    let mut page_markdown = Vec::new();
    for page in &pages {
        let mut markdown = page.markdown.clone();

        markdown = strip_header_footer(markdown, page.header.as_deref(), page.footer.as_deref());
        markdown = replace_table_placeholders(markdown, page, cfg.table_format);

        if cfg.include_images {
            markdown = replace_image_placeholders(markdown, &image_map);
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
        let xhtml = wrap_xhtml_document(&title, &cfg.language, &html_fragment);

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
        .or_else(|| chapters.first().map(|chapter| chapter.title.clone()))
        .unwrap_or_else(|| sanitize_file_stem(source_pdf));

    Ok(RenderedBook {
        title,
        author: cfg.author.clone(),
        language: cfg.language.clone(),
        publisher: cfg.publisher.clone(),
        source_hash: source_hash.to_owned(),
        chapters,
        images,
    })
}

fn extract_images(pages: &[OcrPage], include_images: bool) -> Result<(Vec<ImageAsset>, HashMap<String, String>)> {
    if !include_images {
        return Ok((Vec::new(), HashMap::new()));
    }

    let mut images = Vec::new();
    let mut image_map = HashMap::new();
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

            let bytes = BASE64.decode(encoded).map_err(|error| {
                BaegunError::ocr_schema(format!(
                    "Failed decoding OCR image '{}' as base64: {error}",
                    image.id
                ))
            })?;

            let file_name = unique_asset_name(&image.id, page.index, index, &mut used_filenames);
            let media_type = media_type_from_filename(&file_name).to_owned();

            image_map.insert(image.id.clone(), format!("../images/{file_name}"));
            seen_placeholders.insert(image.id.clone());

            images.push(ImageAsset {
                id: image.id.clone(),
                file_name,
                media_type,
                bytes,
            });
        }
    }

    Ok((images, image_map))
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

fn replace_table_placeholders(mut markdown: String, page: &OcrPage, table_format: TableFormat) -> String {
    for (index, table) in page.tables.iter().enumerate() {
        let table_id = table
            .id
            .clone()
            .unwrap_or_else(|| format!("tbl-{}-{}.html", page.index, index));

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

        let direct_placeholder = format!("[{table_id}]({table_id})");
        if markdown.contains(&direct_placeholder) {
            markdown = markdown.replace(&direct_placeholder, &format!("\n{content}\n"));
            continue;
        }

        if let Ok(regex) = Regex::new(&format!(r"\[[^\]]*\]\({}\)", regex::escape(&table_id))) {
            markdown = regex
                .replace_all(&markdown, format!("\n{content}\n"))
                .to_string();
        }
    }

    markdown
}

fn replace_image_placeholders(mut markdown: String, image_map: &HashMap<String, String>) -> String {
    for (placeholder, relative_path) in image_map {
        markdown = markdown.replace(
            &format!("]({placeholder})"),
            &format!("]({relative_path})"),
        );
    }
    markdown
}

fn strip_markdown_images(markdown: String) -> String {
    let Ok(regex) = Regex::new(r"!\[[^\]]*\]\([^\)]+\)") else {
        return markdown;
    };
    regex.replace_all(&markdown, "").to_string()
}

fn split_into_chapters(markdown: &str) -> Vec<(String, String)> {
    if markdown.trim().is_empty() {
        return Vec::new();
    }

    let mut chapters: Vec<(String, String)> = Vec::new();
    let mut current_title = String::from("Introduction");
    let mut current_body = String::new();
    let mut found_h1 = false;

    for line in markdown.lines() {
        if let Some(title) = line.strip_prefix("# ") {
            if !current_body.trim().is_empty() {
                chapters.push((current_title.clone(), current_body.trim().to_owned()));
                current_body.clear();
            }

            current_title = title.trim().to_owned();
            found_h1 = true;
        }

        current_body.push_str(line);
        current_body.push('\n');
    }

    if !current_body.trim().is_empty() {
        chapters.push((current_title, current_body.trim().to_owned()));
    }

    if !found_h1 && chapters.len() > 1 {
        let joined = chapters
            .into_iter()
            .map(|(_, chunk)| chunk)
            .collect::<Vec<_>>()
            .join("\n\n");
        return vec![(String::from("Document"), joined)];
    }

    if chapters.len() >= 2 {
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

    add_heading_anchors(html_output)
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
    use super::split_into_chapters;

    #[test]
    fn split_uses_h1_boundaries() {
        let markdown = "# One\nA\n\n# Two\nB";
        let chapters = split_into_chapters(markdown);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].0, "One");
        assert_eq!(chapters[1].0, "Two");
    }
}
