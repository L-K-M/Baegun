use baegun_core::{convert_pdf_to_epub, BaegunError, ConvertConfig, ErrorKind, TableFormat};
use clap::{ArgAction, Args, Parser, Subcommand};
use std::env;
use std::path::PathBuf;
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
}

#[derive(Debug, Args)]
struct ConvertArgs {
    input_pdf: PathBuf,

    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,

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

    let api_key = args
        .api_key
        .or_else(|| env::var("MISTRAL_API_KEY").ok())
        .filter(|value| !value.trim().is_empty());

    if api_key.is_none() {
        return Err(BaegunError::new(
            ErrorKind::BadArgs,
            "Missing API key. Pass --api-key or set MISTRAL_API_KEY.",
        ));
    }

    let table_format = args
        .table_format
        .parse::<TableFormat>()
        .map_err(BaegunError::bad_args)?;

    let cfg = ConvertConfig {
        input_pdf: args.input_pdf,
        output_epub,
        api_key,
        model: args.model,
        title: args.title,
        author: args.author,
        language: args.language,
        publisher: args.publisher,
        table_format,
        extract_header: args.extract_header,
        extract_footer: args.extract_footer,
        include_images: args.include_images,
        cache_dir: args.cache_dir,
        no_cache: args.no_cache,
        validate: args.validate,
        epubcheck_bin: args.epubcheck_bin,
        keep_remote_file: args.keep_remote_file,
        fail_on_warn: args.fail_on_warn,
        debug_dir: args.debug_dir,
        quiet: args.quiet,
        verbose: args.verbose,
    };

    if !cfg.quiet {
        println!("Converting '{}' ...", cfg.input_pdf.display());
    }

    let summary = convert_pdf_to_epub(&cfg)?;

    if !cfg.quiet {
        println!("Done: {}", summary.output_path.display());
        println!(
            "Pages: {}, chapters: {}, images: {}, cache: {}",
            summary.pages_processed,
            summary.chapters,
            summary.images,
            if summary.cache_hit { "hit" } else { "miss" }
        );
        if let Some(validation) = summary.validation {
            println!(
                "Validation: passed={}, warnings={}, errors={}",
                validation.passed, validation.warnings, validation.errors
            );
        }
    }

    Ok(())
}
