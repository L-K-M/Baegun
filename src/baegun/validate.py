from __future__ import annotations

import re
import subprocess
from pathlib import Path

from baegun.models import ValidationResult
from baegun.utils import ValidationFailedError


def run_epubcheck(epub_path: Path, bin_path: str = "epubcheck", *, fail_on_warn: bool = False) -> ValidationResult:
    try:
        result = subprocess.run(
            [bin_path, str(epub_path)],
            check=False,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError as exc:
        raise ValidationFailedError(f"epubcheck binary not found: {bin_path}") from exc

    combined_output = "\n".join(part for part in [result.stdout, result.stderr] if part).strip()

    errors = len(re.findall(r"\berror\b", combined_output, flags=re.IGNORECASE))
    warnings = len(re.findall(r"\bwarning\b", combined_output, flags=re.IGNORECASE))

    ok = result.returncode == 0 and errors == 0 and (warnings == 0 or not fail_on_warn)
    return ValidationResult(ok=ok, errors=errors, warnings=warnings, output=combined_output)
