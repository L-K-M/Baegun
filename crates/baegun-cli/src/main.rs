use baegun_core::{
    convert_pdf_to_epub, BaegunError, ConvertConfig, ConvertSummary, ErrorKind, TableFormat,
};
use clap::{ArgAction, Args, Parser, Subcommand};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

#[derive(Debug, Parser)]
#[command(name = "baegun")]
#[command(about = "Convert PDFs to EPUBs with Mistral OCR")]
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
    input_pdf: PathBuf,

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

    #[arg(long = "extract-header", default_value_t = true)]
    extract_header: bool,

    #[arg(long = "extract-footer", default_value_t = true)]
    extract_footer: bool,

    #[arg(long = "include-images", default_value_t = true)]
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
        .unwrap_or_else(|| args.input_pdf.with_extension("epub"));

    let api_key = resolve_api_key(args.options.api_key.clone());
    if api_key.is_none() {
        return Err(BaegunError::new(
            ErrorKind::BadArgs,
            "Missing API key. Pass --api-key or set MISTRAL_API_KEY.",
        ));
    }

    let table_format = args
        .options
        .table_format
        .parse::<TableFormat>()
        .map_err(BaegunError::bad_args)?;

    let cfg = build_config(args.input_pdf, output_epub, &args.options, api_key, table_format);
    run_single_conversion(&cfg)?;

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
    if api_key.is_none() {
        return Err(BaegunError::new(
            ErrorKind::BadArgs,
            "Missing API key. Pass --api-key or set MISTRAL_API_KEY.",
        ));
    }

    let table_format = args
        .options
        .table_format
        .parse::<TableFormat>()
        .map_err(BaegunError::bad_args)?;

    let pdf_files = collect_pdf_files(&args.input_dir, args.recursive)?;
    if pdf_files.is_empty() {
        return Err(BaegunError::bad_args(format!(
            "No PDF files found in '{}'.",
            args.input_dir.display()
        )));
    }

    if !args.options.quiet {
        println!(
            "Found {} PDF file(s) in '{}'{}.",
            pdf_files.len(),
            args.input_dir.display(),
            if args.recursive { " (recursive)" } else { "" }
        );
    }

    let mut successes = 0_usize;
    let mut failures: Vec<(PathBuf, BaegunError)> = Vec::new();

    for input_pdf in pdf_files {
        let output_epub = derive_batch_output_path(&input_pdf, &args.input_dir, &output_dir);
        if let Some(parent) = output_epub.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                BaegunError::internal(format!(
                    "Failed creating output directory '{}': {error}",
                    parent.display()
                ))
            })?;
        }

        let cfg = build_config(
            input_pdf.clone(),
            output_epub,
            &args.options,
            api_key.clone(),
            table_format,
        );

        match run_single_conversion(&cfg) {
            Ok(_) => {
                successes += 1;
            }
            Err(error) => {
                if !args.options.quiet {
                    eprintln!("Failed '{}': {}", input_pdf.display(), error.message);
                }
                failures.push((input_pdf, error));
            }
        }
    }

    if !args.options.quiet {
        println!(
            "Batch complete: {} succeeded, {} failed.",
            successes,
            failures.len()
        );
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

fn run_single_conversion(cfg: &ConvertConfig) -> Result<ConvertSummary, BaegunError> {
    if !cfg.quiet {
        println!("Converting '{}' ...", cfg.input_pdf.display());
    }

    let summary = convert_pdf_to_epub(cfg)?;

    if !cfg.quiet {
        println!("Done: {}", summary.output_path.display());
        println!(
            "Pages: {}, chapters: {}, images: {}, cache: {}",
            summary.pages_processed,
            summary.chapters,
            summary.images,
            if summary.cache_hit { "hit" } else { "miss" }
        );
        if let Some(validation) = summary.validation.as_ref() {
            println!(
                "Validation: passed={}, warnings={}, errors={}",
                validation.passed, validation.warnings, validation.errors
            );
        }
    }

    Ok(summary)
}

fn collect_pdf_files(input_dir: &Path, recursive: bool) -> Result<Vec<PathBuf>, BaegunError> {
    let mut files = Vec::new();
    collect_pdf_files_inner(input_dir, recursive, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_pdf_files_inner(
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
                collect_pdf_files_inner(&path, true, files)?;
            }
            continue;
        }

        if is_pdf_file(&path) {
            files.push(path);
        }
    }

    Ok(())
}

fn is_pdf_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

fn derive_batch_output_path(input_pdf: &Path, input_dir: &Path, output_dir: &Path) -> PathBuf {
    let relative_path = input_pdf
        .strip_prefix(input_dir)
        .ok()
        .unwrap_or(input_pdf);
    let mut output_epub = output_dir.join(relative_path);
    output_epub.set_extension("epub");
    output_epub
}

fn build_batch_failure_error(successes: usize, failures: &[(PathBuf, BaegunError)]) -> BaegunError {
    let kind = failures
        .first()
        .map(|(_, error)| error.kind)
        .unwrap_or(ErrorKind::Internal);

    let mut message = format!(
        "Batch conversion completed with {} failure(s) and {} success(es).",
        failures.len(),
        successes
    );

    for (path, error) in failures.iter().take(5) {
        message.push_str(&format!("\n- {}: {}", path.display(), error.message));
    }

    if failures.len() > 5 {
        message.push_str(&format!(
            "\n- ... and {} more failure(s).",
            failures.len() - 5
        ));
    }

    BaegunError::new(kind, message)
}

#[cfg(test)]
mod tests {
    use super::{derive_batch_output_path, is_pdf_file};
    use std::path::{Path, PathBuf};

    #[test]
    fn pdf_detection_is_case_insensitive() {
        assert!(is_pdf_file(Path::new("sample.pdf")));
        assert!(is_pdf_file(Path::new("sample.PDF")));
        assert!(!is_pdf_file(Path::new("sample.txt")));
    }

    #[test]
    fn batch_output_preserves_relative_path_structure() {
        let input_dir = PathBuf::from("input");
        let output_dir = PathBuf::from("output");
        let input_pdf = input_dir.join("nested").join("book.pdf");

        let output = derive_batch_output_path(&input_pdf, &input_dir, &output_dir);
        assert_eq!(output, output_dir.join("nested").join("book.epub"));
    }
}
