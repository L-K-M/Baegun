use baegun_core::{
    convert_to_epub, detect_source_format, BaegunError, ConvertConfig, ConvertSummary, ErrorKind,
    SourceFormat, TableFormat,
};
use clap::{ArgAction, Args, Parser, Subcommand};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

#[derive(Debug, Parser)]
#[command(name = "baegun")]
#[command(version)]
#[command(about = "Convert PDF and CBZ books to EPUB")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Convert(ConvertArgs),
    #[command(name = "convert-batch")]
    ConvertBatch(ConvertBatchArgs),
}

#[derive(Debug, Args)]
struct ConvertArgs {
    input: PathBuf,

    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,

    #[command(flatten)]
    options: CommonConvertArgs,
}

#[derive(Debug, Args)]
struct ConvertBatchArgs {
    input_dir: PathBuf,

    #[arg(short = 'o', long = "output-dir")]
    output_dir: Option<PathBuf>,

    #[arg(long, action = ArgAction::SetTrue)]
    recursive: bool,

    #[command(flatten)]
    options: CommonConvertArgs,
}

#[derive(Debug, Args, Clone)]
struct CommonConvertArgs {
    #[arg(long = "api-key")]
    api_key: Option<String>,

    #[arg(long, default_value = "mistral-ocr-latest")]
    model: String,

    #[arg(long)]
    title: Option<String>,

    #[arg(long)]
    author: Option<String>,

    #[arg(long, default_value = "en")]
    language: String,

    #[arg(long)]
    publisher: Option<String>,

    #[arg(long, default_value = "html")]
    table_format: String,

    // These default-on toggles take an explicit boolean value (`--include-images false`);
    // a plain `bool` flag would get `ArgAction::SetTrue` and could never be disabled.
    #[arg(long = "extract-header", default_value_t = true, action = ArgAction::Set, value_name = "BOOL")]
    extract_header: bool,

    #[arg(long = "extract-footer", default_value_t = true, action = ArgAction::Set, value_name = "BOOL")]
    extract_footer: bool,

    #[arg(long = "include-images", default_value_t = true, action = ArgAction::Set, value_name = "BOOL")]
    include_images: bool,

    #[arg(long = "cache-dir", default_value = ".baegun-cache")]
    cache_dir: PathBuf,

    #[arg(long = "no-cache", action = ArgAction::SetTrue)]
    no_cache: bool,

    #[arg(long, action = ArgAction::SetTrue)]
    validate: bool,

    #[arg(long = "epubcheck-bin", default_value = "epubcheck")]
    epubcheck_bin: String,

    #[arg(long = "debug-dir")]
    debug_dir: Option<PathBuf>,

    #[arg(long = "keep-remote-file", action = ArgAction::SetTrue)]
    keep_remote_file: bool,

    #[arg(long = "fail-on-warn", action = ArgAction::SetTrue)]
    fail_on_warn: bool,

    #[arg(long = "quiet", action = ArgAction::SetTrue)]
    quiet: bool,

    #[arg(long = "verbose", action = ArgAction::SetTrue)]
    verbose: bool,

    #[arg(long = "comic", action = ArgAction::SetTrue, help = "PDF-only comic mode: render each OCR page as a full-bleed image")]
    comic: bool,

    #[arg(long = "delete-source", action = ArgAction::SetTrue, help = "Delete the source book after successful conversion")]
    delete_source: bool,
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Convert(args) => run_convert(args),
        Commands::ConvertBatch(args) => run_convert_batch(args),
    };

    match result {
        Ok(()) => {}
        Err(error) => {
            eprintln!("Error: {}", error.message);
            process::exit(error.kind.exit_code());
        }
    }
}

fn run_convert(args: ConvertArgs) -> Result<(), BaegunError> {
    let output_epub = args
        .output
        .unwrap_or_else(|| args.input.with_extension("epub"));

    let api_key = resolve_api_key(args.options.api_key.clone());

    let table_format = args
        .options
        .table_format
        .parse::<TableFormat>()
        .map_err(BaegunError::bad_args)?;

    let delete_source = args.options.delete_source;
    let input_path = args.input.clone();
    let cfg = build_config(
        args.input,
        output_epub,
        &args.options,
        api_key,
        table_format,
    );
    run_single_conversion(&cfg)?;

    if delete_source {
        delete_source_file(&input_path, cfg.quiet)?;
    }

    Ok(())
}

fn run_convert_batch(args: ConvertBatchArgs) -> Result<(), BaegunError> {
    if !args.input_dir.exists() {
        return Err(BaegunError::bad_args(format!(
            "Input directory does not exist: {}",
            args.input_dir.display()
        )));
    }

    if !args.input_dir.is_dir() {
        return Err(BaegunError::bad_args(format!(
            "Input path is not a directory: {}",
            args.input_dir.display()
        )));
    }

    let output_dir = args
        .output_dir
        .clone()
        .unwrap_or_else(|| args.input_dir.clone());
    fs::create_dir_all(&output_dir).map_err(|error| {
        BaegunError::internal(format!(
            "Failed creating output directory '{}': {error}",
            output_dir.display()
        ))
    })?;

    let api_key = resolve_api_key(args.options.api_key.clone());

    let table_format = args
        .options
        .table_format
        .parse::<TableFormat>()
        .map_err(BaegunError::bad_args)?;

    let book_files = collect_book_files(&args.input_dir, args.recursive)?;
    if book_files.is_empty() {
        return Err(BaegunError::bad_args(format!(
            "No PDF or CBZ files found in '{}'.",
            args.input_dir.display()
        )));
    }

    if !args.options.quiet {
        println!(
            "Found {} book file(s) in '{}'{}.",
            book_files.len(),
            args.input_dir.display(),
            if args.recursive { " (recursive)" } else { "" }
        );
    }

    let mut successes = 0_usize;
    let mut failures: Vec<(PathBuf, BaegunError)> = Vec::new();
    let mut total_pages = 0_usize;
    let mut total_chapters = 0_usize;
    let mut total_images = 0_usize;
    let mut cache_hits = 0_usize;
    let mut sources_deleted = 0_usize;
    let mut used_outputs = HashSet::new();

    for input in book_files {
        let output_epub = unique_batch_output_path(
            derive_batch_output_path(&input, &args.input_dir, &output_dir),
            &mut used_outputs,
        );
        if let Some(parent) = output_epub.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                BaegunError::internal(format!(
                    "Failed creating output directory '{}': {error}",
                    parent.display()
                ))
            })?;
        }

        let cfg = build_config(
            input.clone(),
            output_epub,
            &args.options,
            api_key.clone(),
            table_format,
        );

        match run_single_conversion(&cfg) {
            Ok(summary) => {
                successes += 1;
                total_pages += summary.pages_processed;
                total_chapters += summary.chapters;
                total_images += summary.images;
                if summary.cache_hit {
                    cache_hits += 1;
                }
                if args.options.delete_source {
                    if let Err(error) = delete_source_file(&input, args.options.quiet) {
                        if !args.options.quiet {
                            eprintln!(
                                "Warning: converted '{}' but failed to delete source: {}",
                                input.display(),
                                error.message
                            );
                        }
                    } else {
                        sources_deleted += 1;
                    }
                }
            }
            Err(error) => {
                if !args.options.quiet {
                    eprintln!("Failed '{}': {}", input.display(), error.message);
                }
                failures.push((input, error));
            }
        }
    }

    if !args.options.quiet {
        println!();
        println!(
            "Batch complete: {} succeeded, {} failed.",
            successes,
            failures.len()
        );
        if successes > 0 {
            println!(
                "Totals: {} pages, {} chapters, {} images, {} PDF OCR cache hit(s).",
                total_pages, total_chapters, total_images, cache_hits
            );
        }
        if sources_deleted > 0 {
            println!("Deleted {} source file(s).", sources_deleted);
        }
        if !failures.is_empty() {
            println!();
            println!("Failures:");
            for (path, error) in &failures {
                println!("  {}: {}", path.display(), error.message);
            }
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(build_batch_failure_error(successes, &failures))
    }
}

fn build_config(
    input_pdf: PathBuf,
    output_epub: PathBuf,
    options: &CommonConvertArgs,
    api_key: Option<String>,
    table_format: TableFormat,
) -> ConvertConfig {
    ConvertConfig {
        input_pdf,
        output_epub,
        api_key,
        model: options.model.clone(),
        title: options.title.clone(),
        author: options.author.clone(),
        language: options.language.clone(),
        publisher: options.publisher.clone(),
        table_format,
        extract_header: options.extract_header,
        extract_footer: options.extract_footer,
        include_images: options.include_images,
        comic_mode: options.comic,
        cache_dir: options.cache_dir.clone(),
        no_cache: options.no_cache,
        validate: options.validate,
        epubcheck_bin: options.epubcheck_bin.clone(),
        keep_remote_file: options.keep_remote_file,
        fail_on_warn: options.fail_on_warn,
        debug_dir: options.debug_dir.clone(),
        quiet: options.quiet,
        verbose: options.verbose,
    }
}

fn resolve_api_key(arg_value: Option<String>) -> Option<String> {
    arg_value
        .or_else(|| env::var("MISTRAL_API_KEY").ok())
        .filter(|value| !value.trim().is_empty())
}

fn delete_source_file(path: &Path, quiet: bool) -> Result<(), BaegunError> {
    fs::remove_file(path).map_err(|error| {
        BaegunError::internal(format!(
            "Failed to delete source file '{}': {error}",
            path.display()
        ))
    })?;
    if !quiet {
        println!("Deleted source: {}", path.display());
    }
    Ok(())
}

fn run_single_conversion(cfg: &ConvertConfig) -> Result<ConvertSummary, BaegunError> {
    if !cfg.quiet {
        println!("Converting '{}' ...", cfg.input_pdf.display());
    }

    let source_format = detect_source_format(&cfg.input_pdf)?;
    let summary = convert_to_epub(cfg)?;

    if !cfg.quiet {
        println!("Done: {}", summary.output_path.display());
        match source_format {
            SourceFormat::Pdf => println!(
                "Pages: {}, chapters: {}, images: {}, OCR cache: {}",
                summary.pages_processed,
                summary.chapters,
                summary.images,
                if summary.cache_hit { "hit" } else { "miss" }
            ),
            SourceFormat::Cbz => println!(
                "Pages: {}, fixed-layout pages: {}, images: {}",
                summary.pages_processed, summary.chapters, summary.images
            ),
        }
        if let Some(validation) = summary.validation.as_ref() {
            println!(
                "Validation: passed={}, warnings={}, errors={}",
                validation.passed, validation.warnings, validation.errors
            );
        }
    }

    Ok(summary)
}

fn collect_book_files(input_dir: &Path, recursive: bool) -> Result<Vec<PathBuf>, BaegunError> {
    let mut files = Vec::new();
    collect_book_files_inner(input_dir, recursive, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_book_files_inner(
    directory: &Path,
    recursive: bool,
    files: &mut Vec<PathBuf>,
) -> Result<(), BaegunError> {
    let entries = fs::read_dir(directory).map_err(|error| {
        BaegunError::internal(format!(
            "Failed reading directory '{}': {error}",
            directory.display()
        ))
    })?;

    for entry_result in entries {
        let entry = entry_result.map_err(|error| {
            BaegunError::internal(format!(
                "Failed reading directory entry in '{}': {error}",
                directory.display()
            ))
        })?;
        let path = entry.path();

        if path.is_dir() {
            if recursive {
                collect_book_files_inner(&path, true, files)?;
            }
            continue;
        }

        if is_book_file(&path) {
            files.push(path);
        }
    }

    Ok(())
}

fn is_book_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("pdf") || value.eq_ignore_ascii_case("cbz"))
        .unwrap_or(false)
}

fn derive_batch_output_path(input_pdf: &Path, input_dir: &Path, output_dir: &Path) -> PathBuf {
    let relative_path = input_pdf.strip_prefix(input_dir).ok().unwrap_or(input_pdf);
    let mut output_epub = output_dir.join(relative_path);
    output_epub.set_extension("epub");
    output_epub
}

fn unique_batch_output_path(candidate: PathBuf, used: &mut HashSet<String>) -> PathBuf {
    if used.insert(case_insensitive_path_key(&candidate)) {
        return candidate;
    }

    let parent = candidate.parent().unwrap_or_else(|| Path::new(""));
    let stem = candidate
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    let mut counter = 2_usize;
    loop {
        let suffixed = parent.join(format!("{stem}-{counter}.epub"));
        if used.insert(case_insensitive_path_key(&suffixed)) {
            return suffixed;
        }
        counter += 1;
    }
}

fn case_insensitive_path_key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

fn build_batch_failure_error(successes: usize, failures: &[(PathBuf, BaegunError)]) -> BaegunError {
    let kind = failures
        .first()
        .map(|(_, error)| error.kind)
        .unwrap_or(ErrorKind::Internal);

    let message = format!(
        "Batch conversion completed with {} failure(s) and {} success(es).",
        failures.len(),
        successes
    );

    BaegunError::new(kind, message)
}

#[cfg(test)]
mod tests {
    use super::{derive_batch_output_path, is_book_file, unique_batch_output_path, Cli, Commands};
    use clap::Parser;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    #[test]
    fn default_on_toggles_accept_explicit_boolean_values() {
        let cli = Cli::try_parse_from([
            "baegun",
            "convert",
            "in.pdf",
            "--include-images",
            "false",
            "--extract-header",
            "false",
            "--extract-footer",
            "true",
        ])
        .expect("documented boolean syntax should parse");

        let Commands::Convert(args) = cli.command else {
            panic!("expected convert subcommand");
        };
        assert!(!args.options.include_images);
        assert!(!args.options.extract_header);
        assert!(args.options.extract_footer);
    }

    #[test]
    fn default_on_toggles_default_to_true() {
        let cli = Cli::try_parse_from(["baegun", "convert", "in.pdf"])
            .expect("plain convert invocation should parse");

        let Commands::Convert(args) = cli.command else {
            panic!("expected convert subcommand");
        };
        assert!(args.options.include_images);
        assert!(args.options.extract_header);
        assert!(args.options.extract_footer);
    }

    #[test]
    fn book_detection_is_case_insensitive() {
        assert!(is_book_file(Path::new("sample.pdf")));
        assert!(is_book_file(Path::new("sample.PDF")));
        assert!(is_book_file(Path::new("sample.cbz")));
        assert!(is_book_file(Path::new("sample.CBZ")));
        assert!(!is_book_file(Path::new("sample.txt")));
    }

    #[test]
    fn batch_output_preserves_relative_path_structure() {
        let input_dir = PathBuf::from("input");
        let output_dir = PathBuf::from("output");
        let input_pdf = input_dir.join("nested").join("book.pdf");

        let output = derive_batch_output_path(&input_pdf, &input_dir, &output_dir);
        assert_eq!(output, output_dir.join("nested").join("book.epub"));
    }

    #[test]
    fn batch_output_suffixes_case_insensitive_pdf_cbz_name_collisions() {
        let mut used = HashSet::new();
        let input_dir = PathBuf::from("input");
        let output_dir = PathBuf::from("output");
        let pdf = derive_batch_output_path(&input_dir.join("Book.pdf"), &input_dir, &output_dir);
        let cbz = derive_batch_output_path(&input_dir.join("book.cbz"), &input_dir, &output_dir);
        let first = unique_batch_output_path(pdf, &mut used);
        let second = unique_batch_output_path(cbz, &mut used);
        assert_eq!(first, PathBuf::from("output/Book.epub"));
        assert_eq!(second, PathBuf::from("output/book-2.epub"));
    }
}
