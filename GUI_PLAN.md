# Baegun GUI — Implementation Plan

## Overview

Build a modern macOS-style desktop GUI for Baegun using **CustomTkinter** with drag-and-drop support via **`tkinterdnd2-universal`**. The GUI wraps the existing `baegun.cli.convert_pdf_to_epub` pipeline. No changes to existing core modules are needed; the GUI is purely an additional frontend.

---

## New File

Create **one new file**: `src/baegun/gui.py`

Add an optional console entrypoint to `pyproject.toml`:

```toml
[project.scripts]
baegun = "baegun.cli:app"
baegun-gui = "baegun.gui:main"
```

Add GUI dependencies to `pyproject.toml`:

```toml
[project.optional-dependencies]
gui = [
  "customtkinter>=5.2",
  "tkinterdnd2-universal>=0.3",
]
dev = [
  "pytest>=8.2",
  "pytest-cov>=5.0",
]
```

Install with: `pip install -e ".[gui]"`

---

## Visual Design

### Aesthetic Goals
- macOS-native dark mode feel (dark grey backgrounds, accent blues)
- Rounded corners everywhere (CustomTkinter default style)
- Clean two-column layout: left panel = settings, right panel = log/progress
- Subtle drag highlight on the drop zone

### Color Palette (CTk appearance)
- Appearance mode: `dark` (set via `ctk.set_appearance_mode("dark")`)
- Color theme: `blue` (set via `ctk.set_default_color_theme("blue")`)

### Window
- Title: `Baegun — PDF to EPUB Converter`
- Minimum size: 820 × 600
- Resizable: yes

---

## Layout

The main window is now a **single-column layout** — no persistent log panel.

```
┌──────────────────────────────────────────┐
│  Baegun — PDF to EPUB Converter          │
├──────────────────────────────────────────┤
│  DROP ZONE / QUEUE                       │
│  ┌─────────────────────────────────────┐ │
│  │  (empty): ☁ Drop PDFs here          │ │
│  │           or click to browse        │ │
│  ├─────────────────────────────────────┤ │
│  │  (filled): • book1.pdf       ✅     │ │
│  │            • book2.pdf       ⏳     │ │
│  │            • comic.pdf       ⏸      │ │
│  │            + Drop more PDFs here    │ │
│  └─────────────────────────────────────┘ │
│  [Clear Completed]                       │
├──────────────────────────────────────────┤
│  SETTINGS                                │
│  API Key: [_______________________] 👁   │
│  Output dir: [_________________] [...]   │
│  ☑ Comic Mode  ☑ Include Images          │
│  ☑ Infer Metadata   Language: [en ▾]     │
├──────────────────────────────────────────┤
│  [      Convert All      ]               │
│  [ Open Output Folder ]                  │
└──────────────────────────────────────────┘
```

**Drop zone / queue panel behaviour:**
- The panel is **always a valid drop target**, whether empty or populated
- **Empty state**: shows a cloud/arrow icon with "Drop PDFs here or click to browse" centred in a dashed-border frame
- **Populated state**: switches to a `CTkScrollableFrame` showing the file list with per-file status icons (⏸ pending, ⏳ running, ✅ done, ❌ failed). A faint dashed border and a "+ Drop more…" hint at the bottom signals it is still droppable
- Dropping more files **appends** to the list; duplicates are silently skipped
- "Clear Completed" button below the panel removes done/error entries; pending entries can be cleared the same way

---

## Drag and Drop

Use `tkinterdnd2-universal`. The app window class must mix in the `TkinterDnD.DnDWrapper`:

```python
import customtkinter as ctk
from tkinterdnd2 import TkinterDnD, DND_FILES

class BaegunApp(ctk.CTk, TkinterDnD.DnDWrapper):
    def __init__(self):
        super().__init__()
        self.TkdndVersion = TkinterDnD._require(self)
        self.queue: list[dict] = []  # [{"path": Path, "status": "pending"|"running"|"done"|"error"}]
        # ... setup UI
```

Register the drop zone frame:

```python
drop_frame.drop_target_register(DND_FILES)
drop_frame.dnd_bind("<<Drop>>", self._on_drop)
drop_frame.dnd_bind("<<DragEnter>>", self._on_drag_enter)
drop_frame.dnd_bind("<<DragLeave>>", self._on_drag_leave)
```

`_on_drop` must handle **multiple files** dropped at once. `tkinterdnd2` delivers them as a single space-separated string in `event.data`. Paths containing spaces are wrapped in `{}` braces.

Parsing logic:

```python
import re

def _parse_drop_data(data: str) -> list[Path]:
    """
    tkinterdnd2 encodes multiple paths as a space-separated list.
    Paths with spaces are wrapped in {curly braces}.
    e.g.: '/simple/path.pdf {/path with spaces/file.pdf} /another.pdf'
    """
    paths = []
    # Extract braced paths first, then split remainder on spaces
    for token in re.findall(r'\{[^}]+\}|\S+', data):
        p = Path(token.strip('{}'))
        if p.suffix.lower() == '.pdf' and p.is_file():
            paths.append(p)
    return paths
```

`_on_drop` should:
1. Call `_parse_drop_data(event.data)` to get a list of `Path` objects
2. Filter to `.pdf` files only; log a warning for any non-PDF entries
3. **Append** each new path to `self.queue` as `{"path": p, "status": "pending"}` (skip duplicates already in queue)
4. Call `_refresh_drop_panel()` to re-render the unified panel

### Unified panel rendering (`_refresh_drop_panel`)

The drop zone and queue share a single `CTkFrame` (call it `self.drop_panel`). This function completely re-renders its contents each time the queue changes:

```python
def _refresh_drop_panel(self):
    # Clear all children
    for widget in self.drop_panel.winfo_children():
        widget.destroy()

    if not self.queue:
        # Empty state: centred prompt
        lbl = ctk.CTkLabel(
            self.drop_panel,
            text="\u2601  Drop PDFs here\nor click to browse",
            justify="center", text_color="gray60",
        )
        lbl.pack(expand=True)
    else:
        # Populated state: scrollable file list
        scroll = ctk.CTkScrollableFrame(self.drop_panel)
        scroll.pack(fill="both", expand=True, padx=4, pady=4)
        for item in self.queue:
            icon = {"pending": "\u23f8", "running": "\u23f3", "done": "\u2705", "error": "\u274c"}[item["status"]]
            row = ctk.CTkLabel(scroll, text=f"{icon}  {item['path'].name}", anchor="w")
            row.pack(fill="x", padx=6, pady=1)
        # Persistent drop hint at the bottom
        hint = ctk.CTkLabel(scroll, text="+ Drop more PDFs here", text_color="gray50", anchor="w")
        hint.pack(fill="x", padx=6, pady=(4, 2))
```

The `drop_target_register` and `dnd_bind` calls are placed on `self.drop_panel` once during `__init__` and do **not** need to be re-registered on each refresh — the bindings persist.

---

## Conversion Threading

**Never run the conversion on the main thread.** Process the entire queue sequentially in a single background thread:

```python
import threading
from baegun.config import build_convert_config
from baegun.cli import convert_pdf_to_epub

def _run_queue(self):
    """Runs all pending queue items sequentially in a background thread."""
    self.convert_btn.configure(state="disabled")
    output_dir = Path(self.output_dir_entry.get()) if self.output_dir_entry.get() else None

    for item in self.queue:
        if item["status"] != "pending":
            continue

        item["status"] = "running"
        self._refresh_queue_widget()
        self._log(f"\n▶ Converting: {item['path'].name}")

        try:
            output = (output_dir / item["path"].stem).with_suffix(".epub") if output_dir else None
            cfg = build_convert_config(
                input_pdf=item["path"],
                output=output,
                output_from_metadata=output is None,  # use inferred title if no explicit output dir
                api_key=self.api_key_entry.get() or None,
                comic_mode=self.comic_mode_var.get(),
                include_images=self.include_images_var.get(),
                infer_metadata=self.infer_metadata_var.get(),
                language=self.language_var.get(),
                # all other options at their defaults
                model="mistral-ocr-latest",
                title=None, author=None, publisher=None,
                table_format="html",
                extract_header=True, extract_footer=True,
                cache_dir=Path(".baegun-cache"), no_cache=False,
                validate=False, epubcheck_bin="epubcheck",
                debug_dir=None, keep_remote_file=False,
                metadata_model="mistral-small-latest",
                metadata_max_pages=3, metadata_max_chars=12000,
                fail_on_warn=False, quiet=True, verbose=False,
            )
            result = convert_pdf_to_epub(cfg)
            item["status"] = "done"
            self._log(f"✅ Done: {result}")
        except Exception as e:
            item["status"] = "error"
            self._log(f"❌ Error ({item['path'].name}): {e}")
        finally:
            self._refresh_queue_widget()

    self.convert_btn.configure(state="normal")
    self._log("\n🏁 All conversions complete.")

def _on_convert_click(self):
    pending = [i for i in self.queue if i["status"] == "pending"]
    if not pending:
        self._log("⚠ No pending PDFs in queue.")
        return
    t = threading.Thread(target=self._run_queue, daemon=True)
    t.start()
```

**All UI updates from `_run_queue`** (status icons, dialog progress) must be dispatched via `self.after(0, ...)` since the conversion runs in a background thread.

---

## Modal Progress Dialog

When the user clicks **Convert All**, open a `CTkToplevel` modal dialog **before** starting the background thread. The dialog is non-closable during conversion (override `WM_DELETE_WINDOW` to a no-op) and auto-closes (or shows a "Done — Close" button) when all conversions finish.

### Dialog layout

```
┌───────────────────────────────────────┐
│  Converting 3 PDFs…                   │
│  book2.pdf (2 of 3)                   │
│  ███████████░░░░░░░░░░░░░░░░░░░  67%  │
│  ┌──────────────────────────────────┐ │
│  │ ▶ Converting: book1.pdf          │ │
│  │ ✅ Done: /output/book1.epub      │ │
│  │ ▶ Converting: book2.pdf          │ │
│  │ …                                │ │
│  └──────────────────────────────────┘ │
│              [  Close  ]  (disabled)  │
└───────────────────────────────────────┘
```

### Implementation

```python
import customtkinter as ctk

class ConversionDialog(ctk.CTkToplevel):
    def __init__(self, parent, total: int):
        super().__init__(parent)
        self.title("Converting…")
        self.resizable(False, False)
        self.grab_set()           # make modal
        self.protocol("WM_DELETE_WINDOW", lambda: None)  # block close during conversion
        self.total = total
        self.done_count = 0

        self.status_label = ctk.CTkLabel(self, text=f"Converting {total} PDF(s)…")
        self.status_label.pack(padx=20, pady=(16, 4))

        self.file_label = ctk.CTkLabel(self, text="", text_color="gray60")
        self.file_label.pack(padx=20, pady=(0, 8))

        self.progress_bar = ctk.CTkProgressBar(self, width=360)
        self.progress_bar.set(0)
        self.progress_bar.pack(padx=20, pady=(0, 12))

        self.log_box = ctk.CTkTextbox(self, width=380, height=200, state="disabled")
        self.log_box.pack(padx=20, pady=(0, 12))

        self.close_btn = ctk.CTkButton(self, text="Close", state="disabled", command=self.destroy)
        self.close_btn.pack(pady=(0, 16))

    def log(self, message: str) -> None:
        """Thread-safe: must be called via self.after() from background thread."""
        self.log_box.configure(state="normal")
        self.log_box.insert("end", message + "\n")
        self.log_box.see("end")
        self.log_box.configure(state="disabled")

    def advance(self, filename: str) -> None:
        """Call when a new file starts converting (thread-safe via after())."""
        self.done_count += 1
        self.file_label.configure(text=f"{filename}  ({self.done_count} of {self.total})")
        self.progress_bar.set(self.done_count / self.total)

    def finish(self) -> None:
        """Call when all conversions are done (thread-safe via after())."""
        self.status_label.configure(text="🏁 All done!")
        self.file_label.configure(text="")
        self.progress_bar.set(1.0)
        self.close_btn.configure(state="normal")
        self.protocol("WM_DELETE_WINDOW", self.destroy)  # re-enable close
```

### Wiring into `_run_queue`

```python
def _on_convert_click(self):
    pending = [i for i in self.queue if i["status"] == "pending"]
    if not pending:
        return
    dialog = ConversionDialog(self, total=len(pending))
    t = threading.Thread(target=self._run_queue, args=(dialog,), daemon=True)
    t.start()

def _run_queue(self, dialog: ConversionDialog):
    output_dir = Path(self.output_dir_entry.get()) if self.output_dir_entry.get() else None
    for item in self.queue:
        if item["status"] != "pending":
            continue
        item["status"] = "running"
        self.after(0, self._refresh_drop_panel)
        self.after(0, dialog.advance, item["path"].name)
        self.after(0, dialog.log, f"▶ Converting: {item['path'].name}")
        try:
            output = (output_dir / item["path"].stem).with_suffix(".epub") if output_dir else None
            cfg = build_convert_config(
                input_pdf=item["path"],
                output=output,
                output_from_metadata=output is None,
                api_key=self.api_key_entry.get() or None,
                comic_mode=self.comic_mode_var.get(),
                include_images=self.include_images_var.get(),
                infer_metadata=self.infer_metadata_var.get(),
                language=self.language_var.get(),
                model="mistral-ocr-latest",
                title=None, author=None, publisher=None,
                table_format="html",
                extract_header=True, extract_footer=True,
                cache_dir=Path(".baegun-cache"), no_cache=False,
                validate=False, epubcheck_bin="epubcheck",
                debug_dir=None, keep_remote_file=False,
                metadata_model="mistral-small-latest",
                metadata_max_pages=3, metadata_max_chars=12000,
                fail_on_warn=False, quiet=True, verbose=False,
            )
            result = convert_pdf_to_epub(cfg)
            item["status"] = "done"
            self.after(0, dialog.log, f"✅ Done: {result}")
        except Exception as e:
            item["status"] = "error"
            self.after(0, dialog.log, f"❌ Error: {e}")
        self.after(0, self._refresh_drop_panel)
    self.after(0, dialog.finish)
```

---

## Settings Persistence

Use Python's `json` module to save/restore user settings between sessions.

- Save location: `~/.baegun_gui_settings.json`
- Fields to persist: `api_key`, `last_output_dir`, `comic_mode`, `include_images`, `infer_metadata`, `language`
- **Do not persist the queue** — the queue is session-only

Load on startup, save on every convert or on window close.

---

## API Key Handling

- Show/hide toggle button on the API key field (`●●●●` vs plain text)
- When `comic_mode` is checked, grey out the API key field (not required)
- On error `ConfigError` (missing API key), show a red inline label below the field

---

## Entrypoint

At bottom of `gui.py`:

```python
def main():
    ctk.set_appearance_mode("dark")
    ctk.set_default_color_theme("blue")
    app = BaegunApp()
    app.mainloop()

if __name__ == "__main__":
    main()
```

---

## Implementation Steps (ordered)

1. **Install dependencies**: `pip install customtkinter tkinterdnd2-universal` (or add to `pyproject.toml [gui]` extras and reinstall)
2. **Create `src/baegun/gui.py`** with the `BaegunApp` class skeleton (window, layout frames)
3. **Implement the unified drop panel** with `_refresh_drop_panel()` (morphs between empty prompt and file list)
4. **Implement `ConversionDialog`** (`CTkToplevel`): progress bar, scrollable log, Close button; block close during conversion via `WM_DELETE_WINDOW`
5. **Implement the settings panel** (entries, switches, combobox, output dir picker)
6. **Wire up the Convert All button**: open `ConversionDialog`, start `_run_queue` in a background thread, pass `dialog` as argument
7. **Ensure all background→UI updates use `self.after(0, ...)`**
8. **Add "Clear Completed" button** below the drop panel
9. **Add settings persistence** (load on `__init__`, save on close/convert)
10. **Add `baegun-gui` entrypoint** to `pyproject.toml` and reinstall with `pip install -e ".[gui]"`
11. **Test** by dropping a single PDF, then multiple PDFs, then a mix of comic and regular PDFs

---

## Files to Modify

| File | Change |
|---|---|
| `pyproject.toml` | Add `[gui]` optional deps, add `baegun-gui` script entrypoint |
| `src/baegun/gui.py` | **Create new** — entire GUI lives here |

**No other source files need to be changed.**

---

## Key Import Map

The GUI uses these existing Baegun APIs directly:

```python
from baegun.config import build_convert_config
from baegun.cli import convert_pdf_to_epub
from baegun.utils import BaegunError, ConfigError
```

`build_convert_config` signature (already implemented):
```python
build_convert_config(
    input_pdf: Path,
    output: Path | None,
    api_key: str | None,
    model: str,
    title: str | None,
    author: str | None,
    language: str,
    publisher: str | None,
    table_format: str,
    extract_header: bool,
    extract_footer: bool,
    include_images: bool,
    comic_mode: bool,
    cache_dir: Path,
    no_cache: bool,
    validate: bool,
    epubcheck_bin: str,
    debug_dir: Path | None,
    keep_remote_file: bool,
    infer_metadata: bool,
    metadata_model: str,
    metadata_max_pages: int,
    metadata_max_chars: int,
    output_from_metadata: bool,
    fail_on_warn: bool,
    quiet: bool,
    verbose: bool,
) -> ConvertConfig
```
