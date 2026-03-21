use crate::errors::{BaegunError, Result};
use crate::models::RenderedBook;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

pub fn write_epub(book: &RenderedBook, output_path: &Path) -> Result<()> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            BaegunError::epub(format!(
                "Failed creating output directory '{}': {error}",
                parent.display()
            ))
        })?;
    }

    let file = File::create(output_path).map_err(|error| {
        BaegunError::epub(format!(
            "Failed creating EPUB output '{}': {error}",
            output_path.display()
        ))
    })?;

    let mut zip = ZipWriter::new(file);
    let stored = FileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = FileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("mimetype", stored)
        .map_err(zip_error("mimetype"))?;
    zip.write_all(b"application/epub+zip")
        .map_err(io_error("mimetype"))?;

    zip.start_file("META-INF/container.xml", deflated)
        .map_err(zip_error("META-INF/container.xml"))?;
    zip.write_all(container_xml().as_bytes())
        .map_err(io_error("META-INF/container.xml"))?;

    zip.start_file("OEBPS/styles/book.css", deflated)
        .map_err(zip_error("OEBPS/styles/book.css"))?;
    zip.write_all(book_css().as_bytes())
        .map_err(io_error("OEBPS/styles/book.css"))?;

    zip.start_file("OEBPS/nav.xhtml", deflated)
        .map_err(zip_error("OEBPS/nav.xhtml"))?;
    zip.write_all(build_nav_xhtml(book).as_bytes())
        .map_err(io_error("OEBPS/nav.xhtml"))?;

    zip.start_file("OEBPS/content.opf", deflated)
        .map_err(zip_error("OEBPS/content.opf"))?;
    zip.write_all(build_content_opf(book).as_bytes())
        .map_err(io_error("OEBPS/content.opf"))?;

    for chapter in &book.chapters {
        let zip_path = format!("OEBPS/text/{}", chapter.file_name);
        zip.start_file(&zip_path, deflated)
            .map_err(zip_error(&zip_path))?;
        zip.write_all(chapter.xhtml.as_bytes())
            .map_err(io_error(&zip_path))?;
    }

    for image in &book.images {
        let zip_path = format!("OEBPS/images/{}", image.file_name);
        zip.start_file(&zip_path, deflated)
            .map_err(zip_error(&zip_path))?;
        zip.write_all(&image.bytes).map_err(io_error(&zip_path))?;
    }

    zip.finish()
        .map_err(|error| BaegunError::epub(format!("Failed finalizing EPUB zip stream: {error}")))?;

    Ok(())
}

fn container_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#
}

fn build_nav_xhtml(book: &RenderedBook) -> String {
    let mut items = String::new();
    for chapter in &book.chapters {
        items.push_str(&format!(
            "      <li><a href=\"text/{}\">{}</a></li>\n",
            chapter.file_name,
            xml_escape(&chapter.title)
        ));
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{}\">\n  <head>\n    <meta charset=\"utf-8\" />\n    <title>Contents</title>\n    <link rel=\"stylesheet\" type=\"text/css\" href=\"styles/book.css\" />\n  </head>\n  <body>\n    <nav epub:type=\"toc\" id=\"toc\">\n      <h1>Contents</h1>\n      <ol>\n{}      </ol>\n    </nav>\n  </body>\n</html>\n",
        xml_escape(&book.language),
        items
    )
}

fn build_content_opf(book: &RenderedBook) -> String {
    let mut manifest = String::from(
        "    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n    <item id=\"css\" href=\"styles/book.css\" media-type=\"text/css\"/>\n",
    );
    let mut spine = String::new();

    for chapter in &book.chapters {
        manifest.push_str(&format!(
            "    <item id=\"{}\" href=\"text/{}\" media-type=\"application/xhtml+xml\"/>\n",
            chapter.id,
            chapter.file_name
        ));
        spine.push_str(&format!("    <itemref idref=\"{}\"/>\n", chapter.id));
    }

    for (index, image) in book.images.iter().enumerate() {
        manifest.push_str(&format!(
            "    <item id=\"img-{:04}\" href=\"images/{}\" media-type=\"{}\"/>\n",
            index + 1,
            image.file_name,
            image.media_type
        ));
    }

    let author_meta = book
        .author
        .as_ref()
        .map(|author| format!("    <dc:creator>{}</dc:creator>\n", xml_escape(author)))
        .unwrap_or_default();
    let publisher_meta = book
        .publisher
        .as_ref()
        .map(|publisher| format!("    <dc:publisher>{}</dc:publisher>\n", xml_escape(publisher)))
        .unwrap_or_default();

    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" unique-identifier=\"bookid\" version=\"3.0\">\n  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n    <dc:identifier id=\"bookid\">urn:sha256:{}</dc:identifier>\n    <dc:title>{}</dc:title>\n{}    <dc:language>{}</dc:language>\n{}  </metadata>\n  <manifest>\n{}  </manifest>\n  <spine>\n{}  </spine>\n</package>\n",
        xml_escape(&book.source_hash),
        xml_escape(&book.title),
        author_meta,
        xml_escape(&book.language),
        publisher_meta,
        manifest,
        spine,
    )
}

fn book_css() -> &'static str {
    r#"body {
  font-family: Georgia, "Times New Roman", serif;
  line-height: 1.5;
  margin: 0;
  padding: 1rem;
}

h1, h2, h3, h4, h5, h6 {
  line-height: 1.2;
  margin-top: 1.3em;
  margin-bottom: 0.6em;
}

h1 { font-size: 1.8rem; }
h2 { font-size: 1.5rem; }
h3 { font-size: 1.25rem; }

img {
  max-width: 100%;
  height: auto;
}

table {
  border-collapse: collapse;
  width: 100%;
  margin: 1rem 0;
}

th, td {
  border: 1px solid #666;
  padding: 0.35rem 0.45rem;
  vertical-align: top;
}

aside.callout {
  border-left: 3px solid #666;
  background: #f5f5f5;
  padding: 0.6rem 0.8rem;
  margin: 0.9rem 0;
}
"#
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn zip_error(path: &str) -> impl Fn(zip::result::ZipError) -> BaegunError + '_ {
    move |error| BaegunError::epub(format!("Failed writing zip entry '{path}': {error}"))
}

fn io_error(path: &str) -> impl Fn(std::io::Error) -> BaegunError + '_ {
    move |error| BaegunError::epub(format!("Failed writing '{path}': {error}"))
}
