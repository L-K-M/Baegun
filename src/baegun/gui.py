from __future__ import annotations

import json
import logging
import os
import re
import subprocess
import sys
import threading
import types
from dataclasses import dataclass
from pathlib import Path
import tkinter as tk
from tkinter import filedialog, messagebox
from typing import TYPE_CHECKING, Any, Callable
from urllib.parse import unquote, urlparse

import customtkinter as ctk


def _ensure_tix_compat() -> None:
    try:
        from tkinter import tix as _tix  # noqa: F401
        return
    except Exception:
        pass

    if "tkinter.tix" in sys.modules:
        return

    tix_module = types.ModuleType("tkinter.tix")
    setattr(tix_module, "Tk", tk.Tk)
    sys.modules["tkinter.tix"] = tix_module
    tk.tix = tix_module  # type: ignore[attr-defined]


_ensure_tix_compat()

try:
    from tkinterdnd2 import DND_FILES, TkinterDnD

    DND_IMPORT_ERROR: str | None = None
except Exception as exc:  # pragma: no cover - import environment specific
    DND_FILES = "DND_Files"
    TkinterDnD = None  # type: ignore[assignment]
    DND_IMPORT_ERROR = str(exc)

if TkinterDnD is None:

    class _BaseAppNoDnD(ctk.CTk):
        pass

    AppBase = _BaseAppNoDnD

else:

    class _BaseAppWithDnD(ctk.CTk, TkinterDnD.DnDWrapper):
        pass

    AppBase = _BaseAppWithDnD

if TYPE_CHECKING:

    class DnDAppBase(ctk.CTk):
        pass

else:
    DnDAppBase = AppBase

from baegun.cli import convert_pdf_to_epub
from baegun.config import build_convert_config
from baegun.utils import ConfigError

LOGGER = logging.getLogger(__name__)

STATUS_ICONS = {
    "pending": "\u23f8",
    "running": "\u23f3",
    "done": "\u2705",
    "error": "\u274c",
}

LANGUAGE_OPTIONS = ["en", "ko", "ja", "zh", "fr", "de", "es"]
SETTINGS_PATH = Path.home() / ".baegun_gui_settings.json"


def _path_from_drop_token(token: str) -> Path | None:
    cleaned = token.strip()
    if not cleaned:
        return None

    if cleaned.startswith("{") and cleaned.endswith("}"):
        cleaned = cleaned[1:-1]

    if cleaned.startswith("file://"):
        parsed = urlparse(cleaned)
        decoded = unquote(parsed.path or "")
        if parsed.netloc and parsed.netloc != "localhost":
            decoded = f"//{parsed.netloc}{decoded}"
        if os.name == "nt" and re.match(r"^/[A-Za-z]:", decoded):
            decoded = decoded[1:]
        cleaned = decoded
    else:
        cleaned = unquote(cleaned)

    if (
        len(cleaned) >= 2
        and cleaned[0] in {'"', "'"}
        and cleaned[-1] == cleaned[0]
    ):
        cleaned = cleaned[1:-1]

    cleaned = cleaned.strip()
    if not cleaned:
        return None
    return Path(cleaned).expanduser()


def _parse_drop_data(data: str, *, tcl: tk.Misc | None = None) -> list[Path]:
    """Parse tkinterdnd2 payload, including file:// URLs and braced paths."""
    raw = data.strip()
    if not raw:
        return []

    tokens: list[str] = []
    if tcl is not None:
        try:
            tokens = [str(item) for item in tcl.tk.splitlist(raw)]
        except tk.TclError:
            tokens = []

    if not tokens:
        tokens = re.findall(r"\{[^}]+\}|\S+", raw)

    paths: list[Path] = []
    for token in tokens:
        path = _path_from_drop_token(token)
        if path is not None:
            paths.append(path)
    return paths


@dataclass(slots=True)
class RunSettings:
    output_dir: Path | None
    api_key: str | None
    comic_mode: bool
    include_images: bool
    infer_metadata: bool
    language: str


class ConversionDialog(ctk.CTkToplevel):
    def __init__(self, parent: ctk.CTk, total: int):
        super().__init__(parent)
        self.title("Converting...")
        self.resizable(False, False)
        self.grab_set()
        self.protocol("WM_DELETE_WINDOW", lambda: None)

        self.total = max(total, 1)
        self.started_count = 0

        self.status_label = ctk.CTkLabel(self, text=f"Converting {total} PDF(s)...")
        self.status_label.pack(padx=20, pady=(16, 4), anchor="w")

        self.file_label = ctk.CTkLabel(self, text="", text_color="gray70")
        self.file_label.pack(padx=20, pady=(0, 8), anchor="w")

        self.progress_bar = ctk.CTkProgressBar(self, width=380)
        self.progress_bar.set(0)
        self.progress_bar.pack(padx=20, pady=(0, 12))

        self.log_box = ctk.CTkTextbox(self, width=420, height=220, state="disabled")
        self.log_box.pack(padx=20, pady=(0, 12))

        self.actions_row = ctk.CTkFrame(self, fg_color="transparent")
        self.actions_row.pack(fill="x", padx=20, pady=(0, 16))

        self.open_output_btn: ctk.CTkButton | None = None

        self.close_btn = ctk.CTkButton(self.actions_row, text="Close", state="disabled", command=self.destroy)
        self.close_btn.pack(side="right")

    def log(self, message: str) -> None:
        self.log_box.configure(state="normal")
        self.log_box.insert("end", message + "\n")
        self.log_box.see("end")
        self.log_box.configure(state="disabled")

    def advance(self, filename: str) -> None:
        self.started_count += 1
        self.file_label.configure(text=f"{filename} ({self.started_count} of {self.total})")
        self.progress_bar.set(self.started_count / self.total)

    def finish(self, open_output_callback: Callable[[], None] | None = None) -> None:
        self.status_label.configure(text="\U0001f3c1 All conversions complete.")
        self.file_label.configure(text="")
        self.progress_bar.set(1.0)

        if open_output_callback is not None:
            if self.open_output_btn is None:
                button = ctk.CTkButton(
                    self.actions_row,
                    text="Open Output Folder",
                    command=open_output_callback,
                )
                button.pack(side="left")
                self.open_output_btn = button
            else:
                self.open_output_btn.configure(command=open_output_callback)

        self.close_btn.configure(state="normal")
        self.protocol("WM_DELETE_WINDOW", self.destroy)


class BaegunApp(DnDAppBase):
    def __init__(self):
        super().__init__()
        self._dnd_enabled = False
        self._dnd_error: str | None = None
        force_no_dnd = os.getenv("BAEGUN_GUI_NO_DND", "").strip().lower() in {"1", "true", "yes", "on"}

        if force_no_dnd:
            self._dnd_error = "Disabled by BAEGUN_GUI_NO_DND"
            self.TkdndVersion = None
        elif TkinterDnD is None:
            self._dnd_error = DND_IMPORT_ERROR
            self.TkdndVersion = None
        else:
            try:
                self.TkdndVersion = TkinterDnD._require(self)
            except Exception as exc:
                self.TkdndVersion = None
                self._dnd_error = str(exc)
                LOGGER.info("Drag-and-drop disabled: %s", exc)
            else:
                self._dnd_enabled = True

        self.title("Baegun \u2014 PDF to EPUB Converter")
        self.minsize(820, 600)
        self.geometry("920x700")
        self.protocol("WM_DELETE_WINDOW", self._on_close)

        self.queue: list[dict[str, Any]] = []
        self._queue_lock = threading.Lock()
        self._is_converting = False
        self._api_key_hidden = True
        self._last_output_dir: Path | None = None

        self.api_key_var = ctk.StringVar(value="")
        self.output_dir_var = ctk.StringVar(value="")
        self.comic_mode_var = ctk.BooleanVar(value=False)
        self.include_images_var = ctk.BooleanVar(value=True)
        self.infer_metadata_var = ctk.BooleanVar(value=True)
        self.language_var = ctk.StringVar(value="en")
        self.notice_var = ctk.StringVar(value="")

        self._drop_fg_idle = ("#1A1D22", "#1A1D22")
        self._drop_fg_hover = ("#203046", "#203046")
        self._drop_border_idle = ("#2F3C4A", "#2F3C4A")
        self._drop_border_hover = ("#4A86C6", "#4A86C6")

        self._build_ui()
        self._load_settings()
        self._sync_api_key_state()
        self._refresh_drop_panel()
        if not self._dnd_enabled:
            self._set_notice("Drag-and-drop unavailable; use click to browse.")

    def _build_ui(self) -> None:
        self.grid_columnconfigure(0, weight=0)
        self.grid_columnconfigure(1, weight=1)
        self.grid_columnconfigure(2, weight=0)
        self.grid_rowconfigure(0, weight=1)

        self.drop_panel = ctk.CTkFrame(
            self,
            corner_radius=12,
            border_width=2,
            border_color=self._drop_border_idle,
            fg_color=self._drop_fg_idle,
            height=220,
        )
        self.drop_panel.grid(row=0, column=0, columnspan=3, sticky="nsew", padx=14, pady=(14, 8))
        self.drop_panel.grid_propagate(False)

        self._register_drop_target(self)
        self._register_drop_target(self.drop_panel)
        self.drop_panel.bind("<Button-1>", self._on_browse_click)

        self.notice_label = ctk.CTkLabel(self, textvariable=self.notice_var, text_color="gray70")
        self.notice_label.grid(row=1, column=0, columnspan=3, sticky="w", padx=14, pady=(0, 8))

        api_key_label = ctk.CTkLabel(self, text="API Key")
        api_key_label.grid(row=2, column=0, sticky="w", padx=(14, 8), pady=(0, 6))

        self.api_key_entry = ctk.CTkEntry(self, textvariable=self.api_key_var, show="*")
        self.api_key_entry.grid(row=2, column=1, sticky="ew", pady=(0, 6))

        self.api_key_toggle_btn = ctk.CTkButton(
            self,
            width=42,
            text="\U0001f441",
            command=self._toggle_api_key_visibility,
        )
        self.api_key_toggle_btn.grid(row=2, column=2, sticky="e", padx=(8, 14), pady=(0, 6))

        self.api_error_label = ctk.CTkLabel(self, text="", text_color="#ff6b6b")
        self.api_error_label.grid(row=3, column=1, columnspan=2, sticky="w", pady=(0, 6), padx=(0, 14))

        output_label = ctk.CTkLabel(self, text="Output dir")
        output_label.grid(row=4, column=0, sticky="w", padx=(14, 8), pady=(0, 8))

        self.output_dir_entry = ctk.CTkEntry(self, textvariable=self.output_dir_var)
        self.output_dir_entry.grid(row=4, column=1, sticky="ew", pady=(0, 8))

        self.output_dir_btn = ctk.CTkButton(self, width=42, text="...", command=self._choose_output_dir)
        self.output_dir_btn.grid(row=4, column=2, sticky="e", padx=(8, 14), pady=(0, 8))

        self.comic_mode_switch = ctk.CTkSwitch(
            self,
            text="Comic Mode",
            variable=self.comic_mode_var,
            command=self._sync_api_key_state,
        )
        self.comic_mode_switch.grid(row=5, column=0, sticky="w", padx=14, pady=(0, 8))

        self.include_images_switch = ctk.CTkSwitch(
            self,
            text="Include Images",
            variable=self.include_images_var,
        )
        self.include_images_switch.grid(row=5, column=1, sticky="w", padx=(0, 8), pady=(0, 8))

        self.infer_metadata_switch = ctk.CTkSwitch(
            self,
            text="Infer Metadata",
            variable=self.infer_metadata_var,
        )
        self.infer_metadata_switch.grid(row=5, column=2, sticky="w", padx=(0, 14), pady=(0, 8))

        language_label = ctk.CTkLabel(self, text="Language")
        language_label.grid(row=6, column=0, sticky="w", padx=(14, 8), pady=(0, 12))

        self.language_combo = ctk.CTkComboBox(
            self,
            values=LANGUAGE_OPTIONS,
            variable=self.language_var,
            state="readonly",
            width=160,
        )
        self.language_combo.grid(row=6, column=1, sticky="w", pady=(0, 12))

        self.convert_btn = ctk.CTkButton(
            self,
            text="Convert All",
            height=42,
            command=self._on_convert_click,
        )
        self.convert_btn.grid(row=7, column=0, columnspan=3, sticky="ew", padx=14, pady=(0, 14))

    def _register_drop_target(self, widget: Any) -> None:
        if not self._dnd_enabled:
            return

        register = getattr(widget, "drop_target_register", None)
        bind = getattr(widget, "dnd_bind", None)
        if not callable(register) or not callable(bind):
            return

        try:
            register(DND_FILES)
            bind("<<Drop>>", self._on_drop)
            bind("<<DragEnter>>", self._on_drag_enter)
            bind("<<DragLeave>>", self._on_drag_leave)
        except Exception as exc:  # pragma: no cover - platform specific
            self._dnd_enabled = False
            self._dnd_error = str(exc)
            LOGGER.info("Drag-and-drop disabled while binding: %s", exc)

    def _register_drop_targets_for_scroll(self, scroll: ctk.CTkScrollableFrame) -> None:
        self._register_drop_target(scroll)
        canvas = getattr(scroll, "_parent_canvas", None)
        if canvas is not None:
            self._register_drop_target(canvas)
        for child in scroll.winfo_children():
            self._register_drop_target(child)

    def _refresh_drop_panel(self) -> None:
        for widget in self.drop_panel.winfo_children():
            widget.destroy()

        with self._queue_lock:
            queue_snapshot = [dict(item) for item in self.queue]

        if not queue_snapshot:
            empty_text = "\u2601  Drop PDFs here\nor click to browse"
            if not self._dnd_enabled:
                empty_text = "\u2601  Click to browse PDFs"

            prompt = ctk.CTkLabel(
                self.drop_panel,
                text=empty_text,
                text_color="gray70",
                justify="center",
                font=ctk.CTkFont(size=18, weight="bold"),
            )
            prompt.pack(expand=True)
            self._register_drop_target(prompt)
            self._bind_browse_click(prompt)
            return

        scroll = ctk.CTkScrollableFrame(self.drop_panel, fg_color="transparent")
        scroll.pack(fill="both", expand=True, padx=5, pady=5)
        self._bind_browse_click(scroll)
        self._register_drop_target(scroll)

        for item in queue_snapshot:
            icon = STATUS_ICONS.get(item["status"], "?")
            row = ctk.CTkLabel(scroll, text=f"{icon}  {item['path'].name}", anchor="w")
            row.pack(fill="x", padx=6, pady=2)
            self._register_drop_target(row)
            self._bind_browse_click(row)

        hint_text = "+ Drop more PDFs here" if self._dnd_enabled else "+ Click to add more PDFs"
        hint = ctk.CTkLabel(scroll, text=hint_text, text_color="gray55", anchor="w")
        hint.pack(fill="x", padx=6, pady=(6, 2))
        self._register_drop_target(hint)
        self._bind_browse_click(hint)
        self._register_drop_targets_for_scroll(scroll)

    def _bind_browse_click(self, widget: Any) -> None:
        widget.bind("<Button-1>", self._on_browse_click, add="+")

    def _on_drag_enter(self, event: Any) -> str:
        self.drop_panel.configure(border_color=self._drop_border_hover, fg_color=self._drop_fg_hover)
        return "copy"

    def _on_drag_leave(self, event: Any) -> str:
        self.drop_panel.configure(border_color=self._drop_border_idle, fg_color=self._drop_fg_idle)
        return "copy"

    def _on_drop(self, event: Any) -> str:
        self.drop_panel.configure(border_color=self._drop_border_idle, fg_color=self._drop_fg_idle)
        data = event.data if isinstance(event.data, str) else str(event.data)
        dropped = _parse_drop_data(data, tcl=self)
        if not dropped:
            self._set_notice("No readable files in drop payload.", is_error=True)
        self._enqueue_paths(dropped)
        return "copy"

    def _on_browse_click(self, _event: Any = None) -> str:
        selected = filedialog.askopenfilenames(
            title="Choose PDF files",
            filetypes=[("PDF files", "*.pdf"), ("All files", "*.*")],
        )
        if selected:
            self._enqueue_paths([Path(p) for p in selected])
        return "break"

    def _enqueue_paths(self, paths: list[Path]) -> None:
        if not paths:
            return

        with self._queue_lock:
            existing = {str(item["path"]) for item in self.queue}
            added = 0
            non_pdf = 0

            for original in paths:
                candidate = original.expanduser()
                if not candidate.is_file():
                    continue
                if candidate.suffix.lower() != ".pdf":
                    non_pdf += 1
                    continue

                resolved = candidate.resolve()
                key = str(resolved)
                if key in existing:
                    continue

                self.queue.append({"path": resolved, "status": "pending"})
                existing.add(key)
                added += 1

        if non_pdf:
            LOGGER.warning("Skipped %s non-PDF entries.", non_pdf)
            self._set_notice(f"Skipped {non_pdf} non-PDF item(s).", is_error=True)
        elif added:
            self._set_notice(f"Added {added} PDF(s) to queue.")

        if added:
            self._refresh_drop_panel()

    def _set_notice(self, message: str, *, is_error: bool = False) -> None:
        self.notice_var.set(message)
        color = "#ff6b6b" if is_error else "gray70"
        self.notice_label.configure(text_color=color)

    def _toggle_api_key_visibility(self) -> None:
        self._api_key_hidden = not self._api_key_hidden
        self.api_key_entry.configure(show="*" if self._api_key_hidden else "")

    def _sync_api_key_state(self) -> None:
        comic_mode = bool(self.comic_mode_var.get())
        state = "disabled" if comic_mode else "normal"
        self.api_key_entry.configure(state=state)
        self.api_key_toggle_btn.configure(state=state)
        if comic_mode:
            self._set_api_error("")

    def _set_api_error(self, message: str) -> None:
        self.api_error_label.configure(text=message)

    def _choose_output_dir(self) -> None:
        selected = filedialog.askdirectory(title="Choose output directory")
        if selected:
            self.output_dir_var.set(selected)
            self._last_output_dir = Path(selected).expanduser()
            self._set_notice(f"Output directory set to: {selected}")

    def _build_run_settings(self) -> RunSettings:
        output_dir_text = self.output_dir_var.get().strip()
        output_dir: Path | None = None
        if output_dir_text:
            output_dir = Path(output_dir_text).expanduser().resolve()
            output_dir.mkdir(parents=True, exist_ok=True)

        language = self.language_var.get().strip() or "en"
        return RunSettings(
            output_dir=output_dir,
            api_key=self.api_key_var.get().strip() or None,
            comic_mode=bool(self.comic_mode_var.get()),
            include_images=bool(self.include_images_var.get()),
            infer_metadata=bool(self.infer_metadata_var.get()),
            language=language,
        )

    def _on_convert_click(self) -> None:
        with self._queue_lock:
            pending_count = sum(1 for item in self.queue if item["status"] == "pending")

        if pending_count == 0:
            self._set_notice("No pending PDFs in queue.", is_error=True)
            return

        try:
            settings = self._build_run_settings()
        except OSError as exc:
            self._set_notice(f"Invalid output directory: {exc}", is_error=True)
            return

        self._set_api_error("")
        self._save_settings()

        self._is_converting = True
        self.convert_btn.configure(state="disabled")
        self._set_settings_enabled(False)
        self._set_notice("Converting queue...")

        dialog = ConversionDialog(self, total=pending_count)
        worker = threading.Thread(target=self._run_queue, args=(dialog, settings), daemon=True)
        worker.start()

    def _set_settings_enabled(self, enabled: bool) -> None:
        state = "normal" if enabled else "disabled"
        self.output_dir_entry.configure(state=state)
        self.output_dir_btn.configure(state=state)
        self.comic_mode_switch.configure(state=state)
        self.include_images_switch.configure(state=state)
        self.infer_metadata_switch.configure(state=state)
        self.language_combo.configure(state="readonly" if enabled else "disabled")

        if enabled:
            self._sync_api_key_state()
        else:
            self.api_key_entry.configure(state="disabled")
            self.api_key_toggle_btn.configure(state="disabled")

    def _run_queue(self, dialog: ConversionDialog, settings: RunSettings) -> None:
        with self._queue_lock:
            pending_items = [item for item in self.queue if item["status"] == "pending"]

        stop_after_config_error = False

        for item in pending_items:
            if stop_after_config_error:
                break

            with self._queue_lock:
                item["status"] = "running"

            name = item["path"].name
            self.after(0, self._refresh_drop_panel)
            self.after(0, dialog.advance, name)
            self.after(0, dialog.log, f"\u25b6 Converting: {name}")

            try:
                output = None
                if settings.output_dir is not None:
                    output = (settings.output_dir / item["path"].stem).with_suffix(".epub")

                cfg = build_convert_config(
                    input_pdf=item["path"],
                    output=output,
                    output_from_metadata=output is None,
                    api_key=settings.api_key,
                    comic_mode=settings.comic_mode,
                    include_images=settings.include_images,
                    infer_metadata=settings.infer_metadata,
                    language=settings.language,
                    model="mistral-ocr-latest",
                    title=None,
                    author=None,
                    publisher=None,
                    table_format="html",
                    extract_header=True,
                    extract_footer=True,
                    cache_dir=Path(".baegun-cache"),
                    no_cache=False,
                    validate=False,
                    epubcheck_bin="epubcheck",
                    debug_dir=None,
                    keep_remote_file=False,
                    metadata_model="mistral-small-latest",
                    metadata_max_pages=3,
                    metadata_max_chars=12000,
                    fail_on_warn=False,
                    quiet=True,
                    verbose=False,
                )
                result = convert_pdf_to_epub(cfg)
            except ConfigError as exc:
                message = str(exc)
                with self._queue_lock:
                    item["status"] = "error"
                self.after(0, dialog.log, f"\u274c Error ({name}): {message}")
                if "Missing API key" in message:
                    self.after(0, self._set_api_error, message)
                    stop_after_config_error = True
            except Exception as exc:  # pragma: no cover - defensive GUI path
                with self._queue_lock:
                    item["status"] = "error"
                self.after(0, dialog.log, f"\u274c Error ({name}): {exc}")
            else:
                with self._queue_lock:
                    item["status"] = "done"
                self._last_output_dir = result.parent
                self.after(0, dialog.log, f"\u2705 Done: {result}")
            finally:
                self.after(0, self._refresh_drop_panel)

        self.after(0, self._finish_queue_run, dialog)

    def _finish_queue_run(self, dialog: ConversionDialog) -> None:
        self._is_converting = False
        self.convert_btn.configure(state="normal")
        self._set_settings_enabled(True)

        with self._queue_lock:
            has_success = any(item["status"] == "done" for item in self.queue)

        dialog.finish(open_output_callback=self._open_output_folder if has_success else None)
        self._set_notice("All conversions complete.")
        self._save_settings()

    def _open_output_folder(self) -> None:
        output_dir_text = self.output_dir_var.get().strip()
        if output_dir_text:
            target = Path(output_dir_text).expanduser()
        elif self._last_output_dir is not None:
            target = self._last_output_dir
        elif self.queue:
            with self._queue_lock:
                target = self.queue[0]["path"].parent
        else:
            target = Path.cwd()

        target = target.resolve()
        if not target.exists():
            messagebox.showwarning("Output folder", f"Folder does not exist: {target}")
            return

        try:
            if sys.platform == "darwin":
                subprocess.Popen(["open", str(target)])
            elif os.name == "nt":
                os.startfile(str(target))  # type: ignore[attr-defined]
            else:
                subprocess.Popen(["xdg-open", str(target)])
        except Exception as exc:  # pragma: no cover - platform specific
            messagebox.showerror("Output folder", f"Could not open folder: {exc}")

    def _settings_payload(self) -> dict[str, Any]:
        output_dir = self.output_dir_var.get().strip()
        if not output_dir and self._last_output_dir is not None:
            output_dir = str(self._last_output_dir)

        return {
            "api_key": self.api_key_var.get().strip(),
            "last_output_dir": output_dir,
            "comic_mode": bool(self.comic_mode_var.get()),
            "include_images": bool(self.include_images_var.get()),
            "infer_metadata": bool(self.infer_metadata_var.get()),
            "language": self.language_var.get().strip() or "en",
        }

    def _load_settings(self) -> None:
        if not SETTINGS_PATH.exists():
            return

        try:
            payload = json.loads(SETTINGS_PATH.read_text(encoding="utf-8"))
        except (OSError, ValueError) as exc:
            LOGGER.warning("Could not load GUI settings: %s", exc)
            return

        self.api_key_var.set(str(payload.get("api_key", "")))

        last_output_dir = str(payload.get("last_output_dir", "")).strip()
        if last_output_dir:
            self.output_dir_var.set(last_output_dir)
            self._last_output_dir = Path(last_output_dir).expanduser()

        self.comic_mode_var.set(bool(payload.get("comic_mode", False)))
        self.include_images_var.set(bool(payload.get("include_images", True)))
        self.infer_metadata_var.set(bool(payload.get("infer_metadata", True)))

        language = str(payload.get("language", "en")).strip() or "en"
        self.language_var.set(language if language in LANGUAGE_OPTIONS else "en")

    def _save_settings(self) -> None:
        payload = self._settings_payload()
        try:
            SETTINGS_PATH.write_text(json.dumps(payload, indent=2), encoding="utf-8")
        except OSError as exc:  # pragma: no cover - local fs failure
            LOGGER.warning("Could not save GUI settings: %s", exc)

    def _on_close(self) -> None:
        if self._is_converting:
            messagebox.showinfo("Conversion in progress", "Please wait for conversion to finish before closing.")
            return
        self._save_settings()
        self.destroy()


def main() -> None:
    ctk.set_appearance_mode("dark")
    ctk.set_default_color_theme("blue")
    app = BaegunApp()
    app.mainloop()


if __name__ == "__main__":
    main()
