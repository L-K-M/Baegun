use baegun_core::{
    convert_pdf_to_epub, convert_pdf_to_epub_with_progress, ConvertConfig, ConvertStage, ErrorKind,
    TableFormat,
};
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::PathBuf;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};
use zip::ZipArchive;

struct TestWorkspace {
    root: PathBuf,
}

impl TestWorkspace {
    fn new(name: &str) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "baegun-core-integration-{name}-{}-{timestamp}",
            process::id()
        ));
        fs::create_dir_all(&root).expect("temporary workspace should be created");
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn converts_cached_ocr_fixture_to_epub() {
    let workspace = TestWorkspace::new("cached-conversion");
    let input_pdf = workspace.path("input/book.pdf");
    let output_epub = workspace.path("output/book.epub");
    let cache_dir = workspace.path("cache");

    if let Some(parent) = input_pdf.parent() {
        fs::create_dir_all(parent).expect("input directory should be created");
    }

    let pdf_bytes = b"%PDF-1.4\n% Baegun test fixture\n";
    fs::write(&input_pdf, pdf_bytes).expect("input PDF fixture should be written");

    let cfg = fixture_config(input_pdf.clone(), output_epub.clone(), cache_dir.clone());
    seed_cache_from_fixture(&cfg, pdf_bytes);

    let summary = convert_pdf_to_epub(&cfg).expect("cached fixture conversion should succeed");

    assert!(summary.cache_hit);
    assert_eq!(summary.pages_processed, 2);
    assert_eq!(summary.chapters, 2);
    assert_eq!(summary.images, 1);
    assert_eq!(summary.output_path, output_epub);

    let mut archive = ZipArchive::new(File::open(&summary.output_path).expect("epub should exist"))
        .expect("epub archive should be readable");
    let entry_names = zip_entry_names(&mut archive);

    assert!(entry_names.contains(&"mimetype".to_string()));
    assert!(entry_names.contains(&"META-INF/container.xml".to_string()));
    assert!(entry_names.contains(&"OEBPS/content.opf".to_string()));
    assert!(entry_names.contains(&"OEBPS/nav.xhtml".to_string()));
    assert!(entry_names.contains(&"OEBPS/text/cover.xhtml".to_string()));
    assert!(entry_names.contains(&"OEBPS/styles/book.css".to_string()));
    assert!(entry_names
        .iter()
        .any(|name| name.starts_with("OEBPS/text/chapter-001-")));
    assert!(entry_names
        .iter()
        .any(|name| name.starts_with("OEBPS/text/chapter-002-")));
    assert!(entry_names
        .iter()
        .any(|name| name.starts_with("OEBPS/images/")));

    let chapter_one_path = entry_names
        .iter()
        .find(|name| name.starts_with("OEBPS/text/chapter-001-"))
        .expect("first chapter entry should exist");
    let chapter_one = read_zip_entry(&mut archive, chapter_one_path);

    assert!(chapter_one.contains("<table>"));
    assert!(chapter_one.contains("../images/"));
    assert!(!chapter_one.contains("[table-main.html](table-main.html)"));

    let content_opf = read_zip_entry(&mut archive, "OEBPS/content.opf");
    assert!(content_opf.contains("properties=\"cover-image\""));
    assert!(content_opf.contains("href=\"images/img-cover.png\""));

    let cover = read_zip_entry(&mut archive, "OEBPS/text/cover.xhtml");
    assert!(cover.contains("../images/img-cover.png"));
}

#[test]
fn disabled_body_images_still_uses_first_page_image_as_cover() {
    let workspace = TestWorkspace::new("cover-without-body-images");
    let input_pdf = workspace.path("input/book.pdf");
    let output_epub = workspace.path("output/book.epub");
    let cache_dir = workspace.path("cache");

    if let Some(parent) = input_pdf.parent() {
        fs::create_dir_all(parent).expect("input directory should be created");
    }

    let pdf_bytes = b"%PDF-1.4\n% Baegun cover fixture\n";
    fs::write(&input_pdf, pdf_bytes).expect("input PDF fixture should be written");

    let mut cfg = fixture_config(input_pdf.clone(), output_epub, cache_dir);
    cfg.include_images = false;
    seed_cache_from_fixture(&cfg, pdf_bytes);

    let summary = convert_pdf_to_epub(&cfg).expect("cached fixture conversion should succeed");
    assert_eq!(summary.images, 1);

    let mut archive = ZipArchive::new(File::open(&summary.output_path).expect("epub should exist"))
        .expect("epub archive should be readable");
    let chapter_one_path = zip_entry_names(&mut archive)
        .into_iter()
        .find(|name| name.starts_with("OEBPS/text/chapter-001-"))
        .expect("first chapter entry should exist");
    let chapter_one = read_zip_entry(&mut archive, &chapter_one_path);
    assert!(!chapter_one.contains("../images/"));

    let content_opf = read_zip_entry(&mut archive, "OEBPS/content.opf");
    assert!(content_opf.contains("properties=\"cover-image\""));
    assert!(content_opf.contains("href=\"images/img-cover.png\""));
}

#[test]
fn pdf_info_metadata_populates_epub_opf() {
    let workspace = TestWorkspace::new("pdf-info-metadata");
    let input_pdf = workspace.path("input/book.pdf");
    let output_epub = workspace.path("output/book.epub");
    let cache_dir = workspace.path("cache");

    if let Some(parent) = input_pdf.parent() {
        fs::create_dir_all(parent).expect("input directory should be created");
    }

    let pdf_bytes = br#"%PDF-1.4
1 0 obj
<< /Title (Metadata Title) /Author (Metadata Author) /Publisher (Baegun Press) /Subject (Metadata description) /Keywords (conversion, epub) /Lang (fr) >>
endobj
"#;
    fs::write(&input_pdf, pdf_bytes).expect("input PDF fixture should be written");

    let cfg = fixture_config(input_pdf.clone(), output_epub, cache_dir);
    seed_cache_json(
        &cfg,
        pdf_bytes,
        r##"{
  "model": "mistral-ocr-latest",
  "pages": [
    {
      "index": 0,
      "markdown": "![Cover](img-cover.png)",
      "images": [
        {
          "id": "img-cover.png",
          "image_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO2V4x8AAAAASUVORK5CYII="
        }
      ]
    },
    {
      "index": 1,
      "markdown": "# Chapter One\n\nReadable content."
    }
  ]
}"##,
    );

    let summary = convert_pdf_to_epub(&cfg).expect("cached fixture conversion should succeed");
    let mut archive = ZipArchive::new(File::open(&summary.output_path).expect("epub should exist"))
        .expect("epub archive should be readable");
    let content_opf = read_zip_entry(&mut archive, "OEBPS/content.opf");

    assert!(content_opf.contains("<dc:title>Metadata Title</dc:title>"));
    assert!(content_opf.contains("<dc:creator>Metadata Author</dc:creator>"));
    assert!(content_opf.contains("<dc:language>fr</dc:language>"));
    assert!(content_opf.contains("<dc:publisher>Baegun Press</dc:publisher>"));
    assert!(content_opf.contains("<dc:description>Metadata description</dc:description>"));
    assert!(content_opf.contains("<dc:subject>conversion</dc:subject>"));
    assert!(content_opf.contains("<dc:subject>epub</dc:subject>"));
}

#[test]
fn progress_callback_reports_expected_stage_sequence() {
    let workspace = TestWorkspace::new("progress-sequence");
    let input_pdf = workspace.path("input/book.pdf");
    let output_epub = workspace.path("output/book.epub");
    let cache_dir = workspace.path("cache");

    if let Some(parent) = input_pdf.parent() {
        fs::create_dir_all(parent).expect("input directory should be created");
    }

    let pdf_bytes = b"%PDF-1.4\n% Baegun progress fixture\n";
    fs::write(&input_pdf, pdf_bytes).expect("input PDF fixture should be written");

    let cfg = fixture_config(input_pdf.clone(), output_epub.clone(), cache_dir.clone());
    seed_cache_from_fixture(&cfg, pdf_bytes);

    let mut stages = Vec::<ConvertStage>::new();
    let summary = convert_pdf_to_epub_with_progress(&cfg, |progress| {
        stages.push(progress.stage);
    })
    .expect("conversion with progress callback should succeed");

    assert!(summary.cache_hit);
    assert_eq!(stages.first(), Some(&ConvertStage::ReadingInput));
    assert!(stages.contains(&ConvertStage::Ocr));
    assert!(stages.contains(&ConvertStage::Normalize));
    assert!(stages.contains(&ConvertStage::PackageEpub));
    assert!(!stages.contains(&ConvertStage::Validate));
    assert_eq!(stages.last(), Some(&ConvertStage::Complete));
}

#[test]
fn cache_miss_without_api_key_returns_bad_args_error() {
    let workspace = TestWorkspace::new("missing-api-key");
    let input_pdf = workspace.path("input/book.pdf");
    let output_epub = workspace.path("output/book.epub");
    let cache_dir = workspace.path("cache");

    if let Some(parent) = input_pdf.parent() {
        fs::create_dir_all(parent).expect("input directory should be created");
    }

    fs::write(&input_pdf, b"%PDF-1.4\n% Missing key test\n")
        .expect("input PDF fixture should be written");

    let mut cfg = fixture_config(input_pdf, output_epub, cache_dir);
    cfg.no_cache = true;
    cfg.api_key = None;

    let error = convert_pdf_to_epub(&cfg).expect_err("missing API key should fail");
    assert_eq!(error.kind, ErrorKind::BadArgs);
    assert!(error.message.contains("Missing API key"));
}

#[test]
fn rejects_exact_same_input_and_output_without_changing_source() {
    let workspace = TestWorkspace::new("same-path");
    let input_pdf = workspace.path("book.pdf");
    let pdf_bytes = b"%PDF-1.4\n% Exact identity fixture\n";
    fs::write(&input_pdf, pdf_bytes).expect("input PDF fixture should be written");

    assert_same_file_rejected(
        input_pdf.clone(),
        input_pdf,
        workspace.path("cache"),
        pdf_bytes,
    );
}

#[test]
fn rejects_noncanonical_input_alias_without_changing_source() {
    let workspace = TestWorkspace::new("noncanonical-alias");
    let input_dir = workspace.path("input");
    fs::create_dir_all(&input_dir).expect("input directory should be created");
    let input_pdf = input_dir.join("book.pdf");
    let output_alias = input_dir.join("..").join("input").join("book.pdf");
    let pdf_bytes = b"%PDF-1.4\n% Noncanonical identity fixture\n";
    fs::write(&input_pdf, pdf_bytes).expect("input PDF fixture should be written");

    assert_same_file_rejected(input_pdf, output_alias, workspace.path("cache"), pdf_bytes);
}

#[cfg(any(unix, windows))]
#[test]
fn rejects_hard_link_output_without_changing_source() {
    let workspace = TestWorkspace::new("hard-link-alias");
    let input_pdf = workspace.path("book.pdf");
    let output_alias = workspace.path("book.epub");
    let pdf_bytes = b"%PDF-1.4\n% Hard link identity fixture\n";
    fs::write(&input_pdf, pdf_bytes).expect("input PDF fixture should be written");
    fs::hard_link(&input_pdf, &output_alias).expect("hard link should be created");

    assert_same_file_rejected(input_pdf, output_alias, workspace.path("cache"), pdf_bytes);
}

#[cfg(unix)]
#[test]
fn rejects_symlink_output_without_changing_source() {
    use std::os::unix::fs::symlink;

    let workspace = TestWorkspace::new("symlink-alias");
    let input_pdf = workspace.path("book.pdf");
    let output_alias = workspace.path("book.epub");
    let pdf_bytes = b"%PDF-1.4\n% Symlink identity fixture\n";
    fs::write(&input_pdf, pdf_bytes).expect("input PDF fixture should be written");
    symlink(&input_pdf, &output_alias).expect("symlink should be created");

    assert_same_file_rejected(input_pdf, output_alias, workspace.path("cache"), pdf_bytes);
}

#[test]
fn successful_conversion_atomically_replaces_existing_destination() {
    let workspace = TestWorkspace::new("replace-existing");
    let input_pdf = workspace.path("input/book.pdf");
    let output_epub = workspace.path("output/book.epub");
    let cache_dir = workspace.path("cache");
    fs::create_dir_all(input_pdf.parent().expect("input should have a parent"))
        .expect("input directory should be created");
    fs::create_dir_all(output_epub.parent().expect("output should have a parent"))
        .expect("output directory should be created");

    let pdf_bytes = b"%PDF-1.4\n% Atomic replacement fixture\n";
    let old_destination = b"existing destination bytes";
    fs::write(&input_pdf, pdf_bytes).expect("input PDF fixture should be written");
    fs::write(&output_epub, old_destination).expect("existing destination should be written");

    let cfg = fixture_config(input_pdf, output_epub.clone(), cache_dir);
    seed_cache_from_fixture(&cfg, pdf_bytes);
    convert_pdf_to_epub(&cfg).expect("cached fixture conversion should succeed");

    assert_ne!(
        fs::read(&output_epub).expect("replacement EPUB should be readable"),
        old_destination
    );
    ZipArchive::new(File::open(output_epub).expect("replacement EPUB should exist"))
        .expect("replacement EPUB should be a readable zip archive");
}

fn assert_same_file_rejected(
    input_pdf: PathBuf,
    output_epub: PathBuf,
    cache_dir: PathBuf,
    expected_source_bytes: &[u8],
) {
    let mut cfg = fixture_config(input_pdf.clone(), output_epub.clone(), cache_dir);
    cfg.no_cache = true;
    cfg.api_key = None;

    let error = convert_pdf_to_epub(&cfg)
        .expect_err("filesystem-identical input and output should be rejected");
    assert_eq!(error.kind, ErrorKind::BadArgs);
    assert!(error.message.contains("same filesystem file"));
    assert_eq!(
        fs::read(&input_pdf).expect("source should remain readable"),
        expected_source_bytes
    );
    assert_eq!(
        fs::read(&output_epub).expect("output alias should remain readable"),
        expected_source_bytes
    );
}

fn fixture_config(input_pdf: PathBuf, output_epub: PathBuf, cache_dir: PathBuf) -> ConvertConfig {
    ConvertConfig {
        input_pdf,
        output_epub,
        api_key: None,
        model: "mistral-ocr-latest".to_string(),
        title: None,
        author: None,
        language: "en".to_string(),
        publisher: None,
        table_format: TableFormat::Html,
        extract_header: true,
        extract_footer: true,
        include_images: true,
        comic_mode: false,
        cache_dir,
        no_cache: false,
        validate: false,
        epubcheck_bin: "epubcheck".to_string(),
        keep_remote_file: false,
        fail_on_warn: false,
        debug_dir: None,
        quiet: true,
        verbose: false,
    }
}

fn seed_cache_from_fixture(cfg: &ConvertConfig, pdf_bytes: &[u8]) {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("ocr_payload_sample.json");
    let fixture_json = fs::read_to_string(&fixture_path).unwrap_or_else(|error| {
        panic!(
            "failed reading fixture '{}': {error}",
            fixture_path.display()
        )
    });

    seed_cache_json(cfg, pdf_bytes, &fixture_json);
}

fn seed_cache_json(cfg: &ConvertConfig, pdf_bytes: &[u8], fixture_json: &str) {
    let cache_key = compute_cache_key(cfg, pdf_bytes);
    let cache_path = cfg.cache_dir.join(format!("{cache_key}.ocr.json"));

    fs::create_dir_all(&cfg.cache_dir).expect("cache directory should be created");
    fs::write(&cache_path, fixture_json).unwrap_or_else(|error| {
        panic!(
            "failed writing cache fixture '{}': {error}",
            cache_path.display()
        )
    });
}

fn compute_cache_key(cfg: &ConvertConfig, pdf_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(pdf_bytes);
    hasher.update(cfg.model.as_bytes());
    hasher.update(cfg.table_format.as_str().as_bytes());
    hasher.update(if cfg.extract_header { b"1" } else { b"0" });
    hasher.update(if cfg.extract_footer { b"1" } else { b"0" });
    hasher.update(b"1");
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    format!("{:x}", hasher.finalize())
}

fn zip_entry_names(archive: &mut ZipArchive<File>) -> Vec<String> {
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .expect("zip entry should be addressable by index");
        names.push(file.name().to_string());
    }
    names
}

fn read_zip_entry(archive: &mut ZipArchive<File>, path: &str) -> String {
    let mut file = archive
        .by_name(path)
        .unwrap_or_else(|error| panic!("missing zip entry '{path}': {error}"));
    let mut content = String::new();
    file.read_to_string(&mut content)
        .unwrap_or_else(|error| panic!("failed reading zip entry '{path}': {error}"));
    content
}
