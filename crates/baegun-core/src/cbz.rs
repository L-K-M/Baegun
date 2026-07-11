use crate::errors::{BaegunError, Result};
use crate::models::{
    BookMetadata, ConvertConfig, ImageAsset, PageProgressionDirection, RenderedBook,
    RenderedChapter,
};
use quick_xml::encoding::Decoder;
use quick_xml::events::BytesStart;
use quick_xml::events::Event;
use quick_xml::Reader;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::{Component, Path};
use zip::result::ZipError;
use zip::ZipArchive;

const MAX_ARCHIVE_ENTRIES: usize = 10_000;
const MAX_PAGES: usize = 2_000;
const MAX_ENTRY_BYTES: u64 = 100 * 1024 * 1024;
const MAX_TOTAL_EXPANDED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_COMPRESSION_RATIO: u64 = 1_000;
const MAX_COMIC_INFO_BYTES: u64 = 1024 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 100_000;
const MAX_IMAGE_PIXELS: u64 = 100_000_000;
const MAX_DECODED_IMAGE_BYTES: usize = 512 * 1024 * 1024;
const MAX_DECODER_WORK_BYTES: usize = 64 * 1024 * 1024;
const READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug)]
struct EntryDescriptor {
    index: usize,
    path: String,
    kind: EntryKind,
    declared_size: u64,
    compressed_size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    ImageCandidate,
    ComicInfo,
    Ignored,
}

#[derive(Debug)]
struct ComicPage {
    source_index: usize,
    source_path: String,
    media_type: &'static str,
    extension: &'static str,
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

#[derive(Debug, Default)]
struct ComicInfo {
    metadata: BookMetadata,
    right_to_left: bool,
    pages: Vec<ComicInfoPage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ComicInfoPage {
    image_index: usize,
    deleted: bool,
    front_cover: bool,
}

pub(crate) fn load_cbz(cfg: &ConvertConfig) -> Result<(RenderedBook, usize)> {
    if cfg.comic_mode {
        return Err(BaegunError::bad_args(
            "--comic is PDF-only; CBZ files are always rendered as fixed-layout image pages.",
        ));
    }

    let mut file = File::open(&cfg.input_pdf).map_err(|error| {
        BaegunError::bad_args(format!(
            "Failed opening CBZ '{}': {error}",
            cfg.input_pdf.display()
        ))
    })?;
    let source_hash = hash_source(&mut file)?;
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        BaegunError::internal(format!(
            "Failed rewinding CBZ source after hashing: {error}"
        ))
    })?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        BaegunError::bad_args(format!(
            "Invalid CBZ ZIP archive '{}': {error}",
            cfg.input_pdf.display()
        ))
    })?;

    let descriptors = inspect_archive(&mut archive)?;
    let mut comic_info = ComicInfo::default();
    let mut pages = Vec::new();
    let mut total_expanded = 0_u64;

    for descriptor in descriptors {
        let bytes = read_entry(&mut archive, &descriptor, &mut total_expanded)?;
        match descriptor.kind {
            EntryKind::ComicInfo => comic_info = parse_comic_info(&bytes)?,
            EntryKind::Ignored => {}
            EntryKind::ImageCandidate => match inspect_image(&bytes) {
                Ok((media_type, extension, width, height)) => {
                    if pages.len() >= MAX_PAGES {
                        return Err(BaegunError::bad_args(format!(
                            "CBZ archive has more than {MAX_PAGES} image pages."
                        )));
                    }
                    pages.push(ComicPage {
                        source_index: descriptor.index,
                        source_path: descriptor.path,
                        media_type,
                        extension,
                        width,
                        height,
                        bytes,
                    });
                }
                Err(reason)
                    if has_image_extension(&descriptor.path)
                        || bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                        || bytes.starts_with(&[0xff, 0xd8]) =>
                {
                    return Err(BaegunError::bad_args(format!(
                        "Unsupported or malformed CBZ image '{}': {reason}",
                        descriptor.path
                    )))
                }
                Err(_) => {}
            },
        }
    }

    if pages.is_empty() {
        return Err(BaegunError::bad_args(
            "CBZ archive contains no supported JPEG or PNG pages.",
        ));
    }

    pages.sort_by(|left, right| {
        natural_path_cmp(&left.source_path, &right.source_path)
            .then_with(|| left.source_index.cmp(&right.source_index))
    });

    let (pages, cover_page) = apply_comic_info_pages(pages, &comic_info.pages);
    if pages.is_empty() {
        return Err(BaegunError::bad_args(
            "ComicInfo.xml marks every CBZ image page as deleted.",
        ));
    }
    let page_count = pages.len();
    let rendered = render_book(cfg, comic_info, pages, cover_page, source_hash);
    Ok((rendered, page_count))
}

fn inspect_archive(archive: &mut ZipArchive<File>) -> Result<Vec<EntryDescriptor>> {
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(BaegunError::bad_args(format!(
            "CBZ archive has {} entries; limit is {MAX_ARCHIVE_ENTRIES}.",
            archive.len()
        )));
    }

    let mut descriptors = Vec::new();
    let mut root_comic_info_seen = false;
    let mut declared_total_expanded = 0_u64;

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| map_entry_error(index, error))?;
        let raw_name = entry.name().to_owned();
        if unsafe_raw_name(&raw_name) {
            return Err(BaegunError::bad_args(format!(
                "Unsafe CBZ entry path '{raw_name}'."
            )));
        }
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| BaegunError::bad_args(format!("Unsafe CBZ entry path '{raw_name}'.")))?;

        validate_entry_type(&entry, &raw_name)?;
        if entry.is_dir() {
            continue;
        }

        if entry.size() > MAX_ENTRY_BYTES {
            return Err(BaegunError::bad_args(format!(
                "CBZ entry '{raw_name}' expands to {} bytes; per-entry limit is {MAX_ENTRY_BYTES} bytes.",
                entry.size()
            )));
        }
        declared_total_expanded = declared_total_expanded
            .checked_add(entry.size())
            .ok_or_else(|| BaegunError::bad_args("CBZ declared expanded size overflowed."))?;
        if declared_total_expanded > MAX_TOTAL_EXPANDED_BYTES {
            return Err(BaegunError::bad_args(format!(
                "CBZ metadata declares more than {MAX_TOTAL_EXPANDED_BYTES} expanded bytes."
            )));
        }
        if exceeds_compression_ratio(entry.size(), entry.compressed_size()) {
            return Err(BaegunError::bad_args(format!(
                "CBZ entry '{raw_name}' exceeds the {MAX_COMPRESSION_RATIO}:1 compression-ratio limit."
            )));
        }

        let path = normalized_relative_path(enclosed)?;
        let kind = if should_ignore(&path) {
            EntryKind::Ignored
        } else if path.eq_ignore_ascii_case("ComicInfo.xml") {
            if root_comic_info_seen {
                return Err(BaegunError::bad_args(
                    "CBZ archive contains duplicate root ComicInfo.xml entries.",
                ));
            }
            root_comic_info_seen = true;
            if entry.size() > MAX_COMIC_INFO_BYTES {
                return Err(BaegunError::bad_args(format!(
                    "ComicInfo.xml exceeds the {MAX_COMIC_INFO_BYTES}-byte limit."
                )));
            }
            EntryKind::ComicInfo
        } else {
            EntryKind::ImageCandidate
        };

        descriptors.push(EntryDescriptor {
            index,
            path,
            kind,
            declared_size: entry.size(),
            compressed_size: entry.compressed_size(),
        });
    }

    Ok(descriptors)
}

fn unsafe_raw_name(name: &str) -> bool {
    let normalized = name.replace('\\', "/");
    normalized.starts_with('/')
        || normalized
            .as_bytes()
            .get(1)
            .is_some_and(|value| *value == b':')
        || normalized
            .split('/')
            .any(|component| component == ".." || component.contains('\0'))
}

fn validate_entry_type(entry: &zip::read::ZipFile<'_>, name: &str) -> Result<()> {
    let Some(mode) = entry.unix_mode() else {
        return Ok(());
    };
    let file_type = mode & 0o170000;
    if file_type == 0 || file_type == 0o100000 || (file_type == 0o040000 && entry.is_dir()) {
        return Ok(());
    }
    let description = if file_type == 0o120000 {
        "symbolic link"
    } else {
        "non-regular file"
    };
    Err(BaegunError::bad_args(format!(
        "CBZ entry '{name}' is a {description}; only regular files and directories are allowed."
    )))
}

fn normalized_relative_path(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => {
                return Err(BaegunError::bad_args(format!(
                    "Unsafe CBZ entry path '{}'.",
                    path.display()
                )))
            }
        }
    }
    if parts.is_empty() {
        return Err(BaegunError::bad_args("CBZ entry has an empty path."));
    }
    Ok(parts.join("/"))
}

fn should_ignore(path: &str) -> bool {
    let components = path.split('/').collect::<Vec<_>>();
    components.iter().any(|component| {
        component.eq_ignore_ascii_case("__MACOSX")
            || component.eq_ignore_ascii_case(".DS_Store")
            || component.eq_ignore_ascii_case("Thumbs.db")
            || component.starts_with("._")
    })
}

fn has_image_extension(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("jpg")
                || extension.eq_ignore_ascii_case("jpeg")
                || extension.eq_ignore_ascii_case("png")
        })
}

fn read_entry(
    archive: &mut ZipArchive<File>,
    descriptor: &EntryDescriptor,
    total_expanded: &mut u64,
) -> Result<Vec<u8>> {
    let entry = archive
        .by_index(descriptor.index)
        .map_err(|error| map_named_entry_error(&descriptor.path, error))?;
    let mut entry = entry.take(observed_read_limit(descriptor, *total_expanded));
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; READ_BUFFER_BYTES];
    let mut entry_expanded = 0_u64;
    loop {
        let read = entry.read(&mut buffer).map_err(|error| {
            BaegunError::bad_args(format!(
                "Failed reading CBZ entry '{}' or validating its CRC: {error}",
                descriptor.path
            ))
        })?;
        if read == 0 {
            break;
        }
        entry_expanded = entry_expanded.checked_add(read as u64).ok_or_else(|| {
            BaegunError::bad_args(format!(
                "CBZ entry '{}' expanded size overflowed.",
                descriptor.path
            ))
        })?;
        *total_expanded = total_expanded
            .checked_add(read as u64)
            .ok_or_else(|| BaegunError::bad_args("CBZ total expanded size overflowed."))?;
        validate_observed_limits(
            &descriptor.path,
            entry_expanded,
            *total_expanded,
            descriptor.compressed_size,
        )?;
        if descriptor.kind == EntryKind::ComicInfo && entry_expanded > MAX_COMIC_INFO_BYTES {
            return Err(BaegunError::bad_args(format!(
                "ComicInfo.xml exceeds the {MAX_COMIC_INFO_BYTES}-byte limit while reading."
            )));
        }
        if descriptor.kind != EntryKind::Ignored {
            bytes.extend_from_slice(&buffer[..read]);
        }
    }
    if entry_expanded != descriptor.declared_size {
        return Err(BaegunError::bad_args(format!(
            "CBZ entry '{}' expanded to {entry_expanded} bytes, but its ZIP metadata declares {} bytes.",
            descriptor.path, descriptor.declared_size
        )));
    }
    Ok(bytes)
}

fn observed_read_limit(descriptor: &EntryDescriptor, total_expanded: u64) -> u64 {
    let ratio_limit = descriptor
        .compressed_size
        .saturating_mul(MAX_COMPRESSION_RATIO);
    let total_remaining = MAX_TOTAL_EXPANDED_BYTES.saturating_sub(total_expanded);
    let kind_limit = if descriptor.kind == EntryKind::ComicInfo {
        MAX_COMIC_INFO_BYTES
    } else {
        MAX_ENTRY_BYTES
    };
    kind_limit
        .min(MAX_ENTRY_BYTES)
        .min(total_remaining)
        .min(ratio_limit)
        .saturating_add(1)
}

fn validate_observed_limits(
    path: &str,
    entry_expanded: u64,
    total_expanded: u64,
    compressed_size: u64,
) -> Result<()> {
    if entry_expanded > MAX_ENTRY_BYTES {
        return Err(BaegunError::bad_args(format!(
            "CBZ entry '{path}' exceeds the {MAX_ENTRY_BYTES}-byte expanded limit while reading."
        )));
    }
    if total_expanded > MAX_TOTAL_EXPANDED_BYTES {
        return Err(BaegunError::bad_args(format!(
            "CBZ entries exceed the {MAX_TOTAL_EXPANDED_BYTES}-byte total expanded limit while reading."
        )));
    }
    if exceeds_compression_ratio(entry_expanded, compressed_size) {
        return Err(BaegunError::bad_args(format!(
            "CBZ entry '{path}' exceeds the {MAX_COMPRESSION_RATIO}:1 compression-ratio limit while reading."
        )));
    }
    Ok(())
}

fn exceeds_compression_ratio(expanded_size: u64, compressed_size: u64) -> bool {
    expanded_size > 0
        && (compressed_size == 0
            || u128::from(expanded_size)
                > u128::from(compressed_size) * u128::from(MAX_COMPRESSION_RATIO))
}

fn hash_source(file: &mut File) -> Result<String> {
    let mut reader = BufReader::with_capacity(READ_BUFFER_BYTES, file);
    let mut buffer = [0_u8; READ_BUFFER_BYTES];
    let mut hasher = Sha256::new();
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            BaegunError::internal(format!("Failed hashing CBZ source bytes: {error}"))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn map_entry_error(index: usize, error: ZipError) -> BaegunError {
    let message = error.to_string();
    if message.contains("Password required") || message.contains("password") {
        BaegunError::bad_args(format!(
            "CBZ entry #{index} is encrypted; encrypted archives are not supported."
        ))
    } else {
        BaegunError::bad_args(format!("Failed inspecting CBZ entry #{index}: {error}"))
    }
}

fn map_named_entry_error(name: &str, error: ZipError) -> BaegunError {
    let message = error.to_string();
    if message.contains("Password required") || message.contains("password") {
        BaegunError::bad_args(format!(
            "CBZ entry '{name}' is encrypted; encrypted archives are not supported."
        ))
    } else {
        BaegunError::bad_args(format!("Failed opening CBZ entry '{name}': {error}"))
    }
}

fn inspect_image(
    bytes: &[u8],
) -> std::result::Result<(&'static str, &'static str, u32, u32), String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let (width, height) = inspect_png(bytes)?;
        return Ok(("image/png", "png", width, height));
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        let (width, height) = inspect_jpeg(bytes)?;
        return Ok(("image/jpeg", "jpg", width, height));
    }
    Err("content is not JPEG or PNG".to_string())
}

fn inspect_png(bytes: &[u8]) -> std::result::Result<(u32, u32), String> {
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: MAX_DECODER_WORK_BYTES,
        },
    );
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    let mut reader = decoder
        .read_info()
        .map_err(|error| format!("PNG decoder rejected header: {error}"))?;
    if reader.info().animation_control.is_some() {
        return Err("animated PNG pages are not supported".to_string());
    }
    let width = reader.info().width;
    let height = reader.info().height;
    let decoded_bytes = reader.output_buffer_size();
    validate_image_limits(width, height, decoded_bytes)?;
    let mut decoded = vec![0_u8; decoded_bytes];
    reader
        .next_frame(&mut decoded)
        .map_err(|error| format!("PNG decoder rejected image data: {error}"))?;
    Ok((width, height))
}

fn inspect_jpeg(bytes: &[u8]) -> std::result::Result<(u32, u32), String> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    decoder.set_max_decoding_buffer_size(MAX_DECODED_IMAGE_BYTES);
    decoder
        .read_info()
        .map_err(|error| format!("JPEG decoder rejected header: {error}"))?;
    let info = decoder
        .info()
        .ok_or_else(|| "JPEG decoder returned no image information".to_string())?;
    let width = u32::from(info.width);
    let height = u32::from(info.height);
    let decoded_upper_bound = pixel_count(width, height)?
        .checked_mul(4)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| "JPEG decoded size overflows allocation limits".to_string())?;
    validate_image_limits(width, height, decoded_upper_bound)?;
    decoder
        .decode()
        .map_err(|error| format!("JPEG decoder rejected image data: {error}"))?;
    Ok((width, height))
}

fn validate_image_limits(
    width: u32,
    height: u32,
    decoded_bytes: usize,
) -> std::result::Result<(), String> {
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(format!(
            "image dimensions {width}x{height} exceed the {MAX_IMAGE_DIMENSION}-pixel per-axis limit"
        ));
    }
    let pixels = pixel_count(width, height)?;
    if pixels > MAX_IMAGE_PIXELS {
        return Err(format!(
            "image has {pixels} pixels; limit is {MAX_IMAGE_PIXELS}"
        ));
    }
    if decoded_bytes > MAX_DECODED_IMAGE_BYTES {
        return Err(format!(
            "decoded image requires {decoded_bytes} bytes; limit is {MAX_DECODED_IMAGE_BYTES} bytes"
        ));
    }
    Ok(())
}

fn pixel_count(width: u32, height: u32) -> std::result::Result<u64, String> {
    if width == 0 || height == 0 {
        return Err("image dimensions must be non-zero".to_string());
    }
    u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| "image pixel count overflowed".to_string())
}

fn parse_comic_info(bytes: &[u8]) -> Result<ComicInfo> {
    if bytes.len() as u64 > MAX_COMIC_INFO_BYTES {
        return Err(BaegunError::bad_args(format!(
            "ComicInfo.xml exceeds the {MAX_COMIC_INFO_BYTES}-byte limit."
        )));
    }

    let mut reader = Reader::from_reader(bytes);
    let mut element_stack = Vec::<String>::new();
    let mut values = std::collections::HashMap::<String, String>::new();
    let mut page_index = 0_usize;
    let mut pages = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
                if name.eq_ignore_ascii_case("Page") {
                    pages.push(parse_page_attributes(&start, reader.decoder(), page_index)?);
                    page_index += 1;
                }
                element_stack.push(name);
            }
            Ok(Event::Empty(start)) => {
                if start.name().as_ref().eq_ignore_ascii_case(b"Page") {
                    pages.push(parse_page_attributes(&start, reader.decoder(), page_index)?);
                    page_index += 1;
                }
            }
            Ok(Event::Text(text)) => {
                if let Some(element) = element_stack.last() {
                    let value = text.decode().map_err(|error| {
                        BaegunError::bad_args(format!("Invalid ComicInfo.xml text: {error}"))
                    })?;
                    values.entry(element.clone()).or_default().push_str(&value);
                }
            }
            Ok(Event::CData(text)) => {
                if let Some(element) = element_stack.last() {
                    let value = text.decode().map_err(|error| {
                        BaegunError::bad_args(format!("Invalid ComicInfo.xml CDATA: {error}"))
                    })?;
                    values.entry(element.clone()).or_default().push_str(&value);
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(element) = element_stack.last() {
                    let value = resolve_xml_reference(&reference)?;
                    values.entry(element.clone()).or_default().push_str(&value);
                }
            }
            Ok(Event::End(_)) => {
                element_stack.pop();
            }
            Ok(Event::DocType(_)) => {
                return Err(BaegunError::bad_args(
                    "ComicInfo.xml DTDs and external entities are not allowed.",
                ))
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(BaegunError::bad_args(format!(
                    "Invalid ComicInfo.xml: {error}"
                )))
            }
        }
    }

    let get = |name: &str| {
        values
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let right_to_left =
        get("Manga").is_some_and(|value| value.trim().eq_ignore_ascii_case("YesAndRightToLeft"));

    Ok(ComicInfo {
        metadata: BookMetadata {
            title: get("Title"),
            author: get("Writer"),
            language: get("LanguageISO"),
            publisher: get("Publisher"),
            description: get("Summary"),
            subjects: Vec::new(),
        },
        right_to_left,
        pages,
    })
}

fn resolve_xml_reference(reference: &quick_xml::events::BytesRef<'_>) -> Result<String> {
    if let Some(value) = reference.resolve_char_ref().map_err(|error| {
        BaegunError::bad_args(format!(
            "Invalid ComicInfo.xml character reference: {error}"
        ))
    })? {
        return Ok(value.to_string());
    }
    let name = reference.decode().map_err(|error| {
        BaegunError::bad_args(format!("Invalid ComicInfo.xml entity reference: {error}"))
    })?;
    match name.as_ref() {
        "amp" => Ok("&".to_string()),
        "lt" => Ok("<".to_string()),
        "gt" => Ok(">".to_string()),
        "apos" => Ok("'".to_string()),
        "quot" => Ok("\"".to_string()),
        _ => Err(BaegunError::bad_args(format!(
            "ComicInfo.xml entity '&{name};' is not allowed."
        ))),
    }
}

fn parse_page_attributes(
    start: &BytesStart<'_>,
    decoder: Decoder,
    fallback_index: usize,
) -> Result<ComicInfoPage> {
    let mut page_type = None;
    let mut image = None;
    for attribute in start.attributes().with_checks(true) {
        let attribute = attribute.map_err(|error| {
            BaegunError::bad_args(format!("Invalid ComicInfo.xml attribute: {error}"))
        })?;
        let key = String::from_utf8_lossy(attribute.key.as_ref());
        let value = attribute
            .decode_and_unescape_value(decoder)
            .map_err(|error| {
                BaegunError::bad_args(format!("Invalid ComicInfo.xml attribute value: {error}"))
            })?
            .into_owned();
        if key.eq_ignore_ascii_case("Type") {
            page_type = Some(value);
        } else if key.eq_ignore_ascii_case("Image") {
            image = value.parse::<usize>().ok();
        }
    }
    let mut deleted = false;
    let mut front_cover = false;
    if let Some(page_type) = page_type {
        for token in page_type.split(|character: char| {
            character.is_ascii_whitespace() || matches!(character, ',' | ';' | '|')
        }) {
            if token.eq_ignore_ascii_case("Deleted") {
                deleted = true;
            } else if token.eq_ignore_ascii_case("FrontCover") {
                front_cover = true;
            }
        }
    }
    Ok(ComicInfoPage {
        image_index: image.unwrap_or(fallback_index),
        deleted,
        front_cover,
    })
}

fn apply_comic_info_pages(
    pages: Vec<ComicPage>,
    metadata_pages: &[ComicInfoPage],
) -> (Vec<ComicPage>, usize) {
    let deleted = metadata_pages
        .iter()
        .filter(|page| page.deleted)
        .map(|page| page.image_index)
        .collect::<std::collections::HashSet<_>>();
    let cover_index = metadata_pages
        .iter()
        .find(|page| page.front_cover && !deleted.contains(&page.image_index))
        .map(|page| page.image_index);
    let mut retained = Vec::with_capacity(pages.len().saturating_sub(deleted.len()));
    let mut remapped_cover = None;
    for (index, page) in pages.into_iter().enumerate() {
        if deleted.contains(&index) {
            continue;
        }
        if cover_index == Some(index) {
            remapped_cover = Some(retained.len());
        }
        retained.push(page);
    }
    (retained, remapped_cover.unwrap_or(0))
}

fn render_book(
    cfg: &ConvertConfig,
    comic_info: ComicInfo,
    pages: Vec<ComicPage>,
    cover_page: usize,
    source_hash: String,
) -> RenderedBook {
    let ComicInfo {
        metadata,
        right_to_left,
        ..
    } = comic_info;
    let language = if cfg.language.trim().is_empty() || cfg.language.eq_ignore_ascii_case("en") {
        metadata.language.unwrap_or_else(|| "en".to_string())
    } else {
        cfg.language.clone()
    };
    let title = cfg
        .title
        .clone()
        .or(metadata.title)
        .unwrap_or_else(|| source_title(&cfg.input_pdf));
    let author = cfg.author.clone().or(metadata.author);
    let publisher = cfg.publisher.clone().or(metadata.publisher);
    let description = metadata.description;
    let mut chapters = Vec::with_capacity(pages.len());
    let mut images = Vec::with_capacity(pages.len());
    let mut cover_image = None;

    for (index, page) in pages.into_iter().enumerate() {
        let number = index + 1;
        let image_name = format!("page-{number:04}.{}", page.extension);
        let chapter_name = format!("page-{number:04}.xhtml");
        let page_title = format!("Page {number}");
        if index == cover_page {
            cover_image = Some(image_name.clone());
        }
        chapters.push(RenderedChapter {
            id: format!("page-{number:04}"),
            title: page_title.clone(),
            file_name: chapter_name,
            markdown: String::new(),
            xhtml: render_page_xhtml(&page_title, &language, &image_name, page.width, page.height),
        });
        images.push(ImageAsset {
            file_name: image_name,
            media_type: page.media_type.to_string(),
            bytes: page.bytes,
        });
    }

    RenderedBook {
        title,
        author,
        language,
        publisher,
        description,
        subjects: metadata.subjects,
        source_hash,
        chapters,
        images,
        cover_image,
        fixed_layout: true,
        page_progression_direction: if right_to_left {
            PageProgressionDirection::RightToLeft
        } else {
            PageProgressionDirection::LeftToRight
        },
    }
}

fn render_page_xhtml(
    title: &str,
    language: &str,
    image_name: &str,
    width: u32,
    height: u32,
) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html class=\"fixed-layout\" xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"{}\">\n  <head>\n    <meta charset=\"utf-8\" />\n    <meta name=\"viewport\" content=\"width={width}, height={height}\" />\n    <title>{}</title>\n    <link rel=\"stylesheet\" type=\"text/css\" href=\"../styles/book.css\" />\n  </head>\n  <body class=\"comic-page\">\n    <div class=\"comic-frame\">\n      <img src=\"../images/{}\" alt=\"{}\" />\n    </div>\n  </body>\n</html>\n",
        xml_escape(language),
        xml_escape(title),
        xml_escape(image_name),
        xml_escape(title),
    )
}

fn source_title(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Comic")
        .to_owned()
}

fn natural_path_cmp(left: &str, right: &str) -> Ordering {
    let left_components = left.split('/').collect::<Vec<_>>();
    let right_components = right.split('/').collect::<Vec<_>>();
    for (left_component, right_component) in left_components.iter().zip(&right_components) {
        let ordering = natural_component_cmp(left_component, right_component);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left_components.len().cmp(&right_components.len())
}

fn natural_component_cmp(left: &str, right: &str) -> Ordering {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    let mut left_index = 0;
    let mut right_index = 0;

    while left_index < left_bytes.len() && right_index < right_bytes.len() {
        if left_bytes[left_index].is_ascii_digit() && right_bytes[right_index].is_ascii_digit() {
            let left_end = digit_end(left_bytes, left_index);
            let right_end = digit_end(right_bytes, right_index);
            let left_digits = &left[left_index..left_end];
            let right_digits = &right[right_index..right_end];
            let left_trimmed = left_digits.trim_start_matches('0');
            let right_trimmed = right_digits.trim_start_matches('0');
            let left_number = if left_trimmed.is_empty() {
                "0"
            } else {
                left_trimmed
            };
            let right_number = if right_trimmed.is_empty() {
                "0"
            } else {
                right_trimmed
            };
            let ordering = left_number
                .len()
                .cmp(&right_number.len())
                .then_with(|| left_number.cmp(right_number))
                .then_with(|| left_digits.len().cmp(&right_digits.len()));
            if ordering != Ordering::Equal {
                return ordering;
            }
            left_index = left_end;
            right_index = right_end;
            continue;
        }

        let ordering = left_bytes[left_index]
            .to_ascii_lowercase()
            .cmp(&right_bytes[right_index].to_ascii_lowercase());
        if ordering != Ordering::Equal {
            return ordering;
        }
        left_index += 1;
        right_index += 1;
    }

    left_bytes
        .len()
        .cmp(&right_bytes.len())
        .then_with(|| left.cmp(right))
}

fn digit_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    end
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
        apply_comic_info_pages, inspect_image, natural_path_cmp, parse_comic_info,
        validate_observed_limits, ComicPage, MAX_COMPRESSION_RATIO,
    };
    use std::cmp::Ordering;

    #[test]
    fn natural_sort_orders_numbers_and_nested_components() {
        let mut paths = vec![
            "chapter10/1.png",
            "chapter2/10.png",
            "chapter2/2.png",
            "chapter2/1.png",
        ];
        paths.sort_by(|left, right| natural_path_cmp(left, right));
        assert_eq!(
            paths,
            vec![
                "chapter2/1.png",
                "chapter2/2.png",
                "chapter2/10.png",
                "chapter10/1.png"
            ]
        );
    }

    #[test]
    fn natural_sort_has_deterministic_case_ties() {
        assert_eq!(natural_path_cmp("Page1.png", "page1.png"), Ordering::Less);
        assert_eq!(natural_path_cmp("page1.png", "page01.png"), Ordering::Less);
    }

    #[test]
    fn comic_info_parses_metadata_direction_and_cover() {
        let xml = br#"<ComicInfo><Title>A &amp; B</Title><Writer>A Writer</Writer><Publisher>A Press</Publisher><Summary><![CDATA[One < two]]></Summary><LanguageISO>ja</LanguageISO><Manga>YesAndRightToLeft</Manga><Pages><Page Image="0" Type="Story"/><Page Image="1" Type="FrontCover"/></Pages></ComicInfo>"#;
        let parsed = parse_comic_info(xml).expect("ComicInfo should parse");
        assert_eq!(parsed.metadata.title.as_deref(), Some("A & B"));
        assert_eq!(parsed.metadata.author.as_deref(), Some("A Writer"));
        assert_eq!(parsed.metadata.description.as_deref(), Some("One < two"));
        assert_eq!(parsed.metadata.language.as_deref(), Some("ja"));
        assert!(parsed.right_to_left);
        assert!(parsed.pages[1].front_cover);
    }

    #[test]
    fn comic_info_plain_manga_is_ltr_and_deleted_cover_is_remapped() {
        let xml = br#"<ComicInfo><Manga>Yes</Manga><Pages><Page Image="0" Type="Story, Deleted"/><Page Image="1" Type="Story"/><Page Image="2" Type="Story; FrontCover"/></Pages></ComicInfo>"#;
        let parsed = parse_comic_info(xml).expect("ComicInfo should parse");
        assert!(!parsed.right_to_left);
        assert!(parsed.pages[0].deleted);
        assert!(parsed.pages[2].front_cover);

        let pages = (0..3).map(dummy_page).collect();
        let (pages, cover) = apply_comic_info_pages(pages, &parsed.pages);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].bytes, vec![1]);
        assert_eq!(pages[1].bytes, vec![2]);
        assert_eq!(cover, 1);
    }

    #[test]
    fn comic_info_decodes_declared_windows_1252() {
        let mut xml =
            br#"<?xml version="1.0" encoding="windows-1252"?><ComicInfo><Title>Caf"#.to_vec();
        xml.push(0xe9);
        xml.extend_from_slice(b"</Title></ComicInfo>");
        let parsed = parse_comic_info(&xml).expect("declared legacy encoding should parse");
        assert_eq!(parsed.metadata.title.as_deref(), Some("Caf\u{e9}"));
    }

    #[test]
    fn comic_info_rejects_doctype() {
        let xml = br#"<!DOCTYPE ComicInfo SYSTEM "https://example.com/comic.dtd"><ComicInfo/>"#;
        let error = parse_comic_info(xml).expect_err("DTD must be rejected");
        assert!(error.message.contains("DTD"));
    }

    #[test]
    fn image_sniffing_does_not_trust_extension() {
        let malformed = b"not really a png";
        assert!(inspect_image(malformed).is_err());
    }

    #[test]
    fn signature_bearing_malformed_images_fail_decoder_validation() {
        let fake_jpeg = [
            0xff, 0xd8, 0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11,
            0x00, 0xff, 0xd9,
        ];
        assert!(inspect_image(&fake_jpeg).is_err());

        let mut invalid_png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&1_u32.to_be_bytes());
        ihdr.extend_from_slice(&1_u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
        append_png_chunk(&mut invalid_png, b"IHDR", &ihdr);
        append_png_chunk(&mut invalid_png, b"IDAT", b"not zlib data");
        append_png_chunk(&mut invalid_png, b"IEND", &[]);
        assert!(inspect_image(&invalid_png).is_err());
    }

    #[test]
    fn observed_compression_ratio_uses_exact_boundary() {
        validate_observed_limits("page", MAX_COMPRESSION_RATIO, MAX_COMPRESSION_RATIO, 1)
            .expect("exact ratio limit should be accepted");
        let error = validate_observed_limits(
            "page",
            MAX_COMPRESSION_RATIO + 1,
            MAX_COMPRESSION_RATIO + 1,
            1,
        )
        .expect_err("one byte over ratio limit should fail");
        assert!(error.message.contains("compression-ratio"));

        let error = validate_observed_limits("page", super::MAX_ENTRY_BYTES + 1, 1, u64::MAX)
            .expect_err("actual per-entry bytes must be bounded");
        assert!(error.message.contains("expanded limit"));
        let error =
            validate_observed_limits("page", 1, super::MAX_TOTAL_EXPANDED_BYTES + 1, u64::MAX)
                .expect_err("actual cumulative bytes must be bounded");
        assert!(error.message.contains("total expanded limit"));
    }

    fn dummy_page(index: u8) -> ComicPage {
        ComicPage {
            source_index: usize::from(index),
            source_path: format!("{index}.png"),
            media_type: "image/png",
            extension: "png",
            width: 1,
            height: 1,
            bytes: vec![index],
        }
    }

    fn append_png_chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        output.extend_from_slice(&(data.len() as u32).to_be_bytes());
        output.extend_from_slice(kind);
        output.extend_from_slice(data);
        let mut crc = crc32fast::Hasher::new();
        crc.update(kind);
        crc.update(data);
        output.extend_from_slice(&crc.finalize().to_be_bytes());
    }
}
