use crate::errors::{BaegunError, Result};
use crate::models::ValidationResult;
use std::path::Path;
use std::process::Command;

pub fn run_epubcheck(epubcheck_bin: &str, epub_path: &Path, fail_on_warn: bool) -> Result<ValidationResult> {
    let output = Command::new(epubcheck_bin)
        .arg(epub_path)
        .output()
        .map_err(|error| {
            BaegunError::validation(format!(
                "Failed launching epubcheck binary '{}': {error}",
                epubcheck_bin
            ))
        })?;

    let mut raw_output = String::new();
    raw_output.push_str(&String::from_utf8_lossy(&output.stdout));
    if !output.stderr.is_empty() {
        if !raw_output.is_empty() {
            raw_output.push('\n');
        }
        raw_output.push_str(&String::from_utf8_lossy(&output.stderr));
    }

    let warnings = raw_output.matches("WARNING").count();
    let errors = raw_output.matches("ERROR").count();
    let passed = output.status.success() && !(fail_on_warn && warnings > 0);

    let result = ValidationResult {
        warnings,
        errors,
        passed,
        raw_output: raw_output.clone(),
    };

    if !passed {
        let mut message = format!(
            "epubcheck reported validation issues (errors: {}, warnings: {})",
            errors, warnings
        );
        if fail_on_warn && warnings > 0 && errors == 0 {
            message.push_str("; fail-on-warn is enabled");
        }
        if !raw_output.trim().is_empty() {
            message.push_str(&format!("\n{raw_output}"));
        }
        return Err(BaegunError::validation(message));
    }

    Ok(result)
}
