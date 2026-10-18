"""
FLP Organizer - GUI (v1.3.0)
============================

Adds:
 - Batch mode (up to 30 files)
 - Sub-sort options (by type, length, color)
 - Post-process options (auto-rename tracks, auto-color, remove empty)
 - Multi-language UI (EN/DE/ES/FR/IT/RU)
"""
from __future__ import annotations
import json
import os
import sys
import webbrowser
import threading
import tkinter as tk
from pathlib import Path
from tkinter import filedialog, messagebox, ttk
from typing import Optional

try:
    from tkinterdnd2 import DND_FILES, TkinterDnD
    DND_AVAILABLE = True
except ImportError:
    DND_AVAILABLE = False
    TkinterDnD = None  # type: ignore

import flp_core
import translations as tr
from translations import t, LANGUAGES, DEFAULT_LANG


APP_NAME = "FLP Organizer"
APP_VERSION = "1.6.1"
AUTHOR = "Matt Danieli"
PAYPAL_URL = "https://paypal.me/mattdanieli"
BATCH_LIMIT = 30

CONFIG_FILE = Path.home() / ".flp_organizer_config.json"
DISCLAIMER_FLAG_FILE = Path.home() / ".flp_organizer_disclaimer_accepted"

# --- Palette -----------------------------------------------------------------
BG          = "#141416"
BG_PANEL    = "#1f1f24"
BG_INSET    = "#0f0f11"
BG_ROW      = "#1a1a1d"
BG_ROW_ALT  = "#1e1e22"
BORDER      = "#2e2e35"

FG          = "#eeeef0"
FG_DIM      = "#9a9aa2"
FG_MUTED    = "#5f5f66"

ACCENT      = "#ff7a00"
ACCENT_HOV  = "#ff9124"
ACCENT_DIS  = "#5a3a1a"
ACCENT_FG   = "#1a1a1d"
ACCENT_DIM  = "#8a6340"

OK_GREEN    = "#4ade80"
WARN_AMBER  = "#fbbf24"
ERROR_RED   = "#f87171"


# --- Resource loader ---------------------------------------------------------

def _resource_path(rel_path: str) -> Path:
    if hasattr(sys, "_MEIPASS"):
        base = Path(sys._MEIPASS)  # type: ignore
    else:
        base = Path(__file__).resolve().parent.parent
    return base / rel_path


# --- Config helpers ----------------------------------------------------------

def load_config() -> dict:
    try:
        if CONFIG_FILE.exists():
            return json.loads(CONFIG_FILE.read_text(encoding="utf-8"))
    except Exception:
        pass
    return {}


def save_config(cfg: dict) -> None:
    try:
        CONFIG_FILE.write_text(json.dumps(cfg, indent=2), encoding="utf-8")
    except Exception:
        pass


# --- Disclaimer --------------------------------------------------------------

def show_disclaimer_if_needed(root, lang: str) -> bool:
    """Returns True if accepted (or already previously accepted)."""
    if DISCLAIMER_FLAG_FILE.exists():
        return True

    dlg = tk.Toplevel(root)
    dlg.title(f"{APP_NAME} — {t('disclaimer_title', lang)}")
    dlg.configure(bg=BG)
    dlg.transient(root)
    dlg.grab_set()
    dlg.resizable(True, True)
    dlg.minsize(480, 380)

    w, h = 640, 520
    root.update_idletasks()
    sw = root.winfo_screenwidth()
    sh = root.winfo_screenheight()
    x = max(0, (sw - w) // 2)
    y = max(0, (sh - h) // 2)
    dlg.geometry(f"{w}x{h}+{x}+{y}")

    result = {"accepted": False}

    def accept():
        try:
            DISCLAIMER_FLAG_FILE.write_text("accepted\n")
        except Exception:
            pass
        result["accepted"] = True
        dlg.destroy()

    def decline():
        result["accepted"] = False
        dlg.destroy()

    # Buttons FIRST (reserved bottom area)
    btn_bar = tk.Frame(dlg, bg=BG, height=60)
    btn_bar.pack(side="bottom", fill="x", padx=24, pady=(8, 18))
    btn_bar.pack_propagate(False)

    ttk.Button(btn_bar, text=t("btn_decline", lang), style="Secondary.TButton",
               command=decline).pack(side="right", padx=(10, 0))
    ttk.Button(btn_bar, text=t("btn_agree", lang), style="Accent.TButton",
               command=accept).pack(side="right")

    tk.Label(dlg, text=t("disclaimer_title", lang),
             bg=BG, fg=ACCENT, font=("Segoe UI", 16, "bold"),
             ).pack(side="top", anchor="w", padx=24, pady=(20, 8))

    txt_frame = tk.Frame(dlg, bg=BG)
    txt_frame.pack(side="top", fill="both", expand=True, padx=24, pady=(0, 10))
    txt = tk.Text(txt_frame, bg=BG_PANEL, fg=FG, font=("Segoe UI", 9),
                  wrap="word", borderwidth=0, highlightthickness=1,
                  highlightbackground=BORDER, padx=12, pady=10)
    txt.insert("1.0", t("disclaimer_text", lang))
    txt.configure(state="disabled")
    txt.pack(fill="both", expand=True)

    dlg.protocol("WM_DELETE_WINDOW", decline)
    dlg.wait_window()
    return result["accepted"]


# --- Main application --------------------------------------------------------

class FlpOrganizerApp:
    def __init__(self, root) -> None:
        self.root = root
        self.root.title(APP_NAME)
        self.root.geometry("920x780")
        self.root.minsize(720, 620)
        self.root.configure(bg=BG)

        # Load user config
        self.config = load_config()
        self.lang = self.config.get("language", DEFAULT_LANG)
        if self.lang not in LANGUAGES:
            self.lang = DEFAULT_LANG

        self._load_icon()

        # State
        self.mode = "single"                    # "single" or "batch"
        self.current_path: Path | None = None   # for single
        self.batch_paths: list[Path] = []       # for batch
        self.current_result: flp_core.AnalysisResult | None = None
        self.current_validation: flp_core.ValidationReport | None = None
        self._user_overrode_validation: bool = False

        # Tkinter vars
        self.sort_mode_var = tk.StringVar(value="alpha")
        self.sub_type_var = tk.BooleanVar(value=False)
        self.sub_length_var = tk.BooleanVar(value=False)
        self.sub_color_var = tk.BooleanVar(value=False)
        self.opt_rename_var = tk.BooleanVar(value=False)
        self.opt_color_var = tk.BooleanVar(value=False)
        self.opt_remove_empty_var = tk.BooleanVar(value=False)
        self.lang_var = tk.StringVar(value=LANGUAGES[self.lang])
        self.batch_output_dir: Path | None = None

        self._logo_photo = None
        self._window_icon_img = None

        self._setup_style()
        self._build_ui()

        self.root.after(200, self._run_disclaimer)

        if DND_AVAILABLE:
            self.drop_area.drop_target_register(DND_FILES)
            self.drop_area.dnd_bind("<<Drop>>", self._on_drop)

    # ---------- resources ----------
    def _load_icon(self) -> None:
        ico_path = _resource_path("docs/icon.ico")
        png_path = _resource_path("docs/icon_64.png")
        try:
            if sys.platform.startswith("win") and ico_path.exists():
                self.root.iconbitmap(default=str(ico_path))
            elif png_path.exists():
                img = tk.PhotoImage(file=str(png_path))
                self.root.iconphoto(True, img)
                self._window_icon_img = img
        except Exception:
            pass

    def _run_disclaimer(self) -> None:
        accepted = show_disclaimer_if_needed(self.root, self.lang)
        if not accepted:
            self.root.destroy()

    # ---------- style ----------
    def _setup_style(self) -> None:
        style = ttk.Style()
        try:
            style.theme_use("clam")
        except tk.TclError:
            pass

        style.configure("TFrame", background=BG)
        style.configure("Panel.TFrame", background=BG_PANEL)

        style.configure("TLabel", background=BG, foreground=FG,
                        font=("Segoe UI", 10))
        style.configure("Panel.TLabel", background=BG_PANEL, foreground=FG,
                        font=("Segoe UI", 10))
        style.configure("Title.TLabel", background=BG, foreground=FG,
                        font=("Segoe UI", 18, "bold"))
        style.configure("TitleAccent.TLabel", background=BG, foreground=ACCENT,
                        font=("Segoe UI", 18, "bold"))
        style.configure("Subtitle.TLabel", background=BG, foreground=FG_DIM,
                        font=("Segoe UI", 10))
        style.configure("Caption.TLabel", background=BG_PANEL,
                        foreground=FG_MUTED, font=("Segoe UI", 9))
        style.configure("Section.TLabel", background=BG_PANEL, foreground=FG,
                        font=("Segoe UI", 9, "bold"))
        style.configure("Status.TLabel", background=BG, foreground=FG_DIM,
                        font=("Segoe UI", 9))
        style.configure("StatusOK.TLabel", background=BG, foreground=OK_GREEN,
                        font=("Segoe UI", 9))
        style.configure("StatusWarn.TLabel", background=BG, foreground=WARN_AMBER,
                        font=("Segoe UI", 9))
        style.configure("StatusErr.TLabel", background=BG, foreground=ERROR_RED,
                        font=("Segoe UI", 9))
        style.configure("Footer.TLabel", background=BG, foreground=FG_MUTED,
                        font=("Segoe UI", 9))
        style.configure("FooterHeart.TLabel", background=BG, foreground=ACCENT,
                        font=("Segoe UI", 10))
        style.configure("LangLabel.TLabel", background=BG, foreground=FG_DIM,
                        font=("Segoe UI", 9))

        # Accent button
        style.configure("Accent.TButton",
                        background=ACCENT, foreground=ACCENT_FG,
                        font=("Segoe UI", 10, "bold"),
                        borderwidth=0, focusthickness=0, padding=(22, 10))
        style.map("Accent.TButton",
                  background=[("active", ACCENT_HOV),
                              ("pressed", ACCENT_HOV),
                              ("disabled", ACCENT_DIS)],
                  foreground=[("disabled", ACCENT_DIM)])

        # Secondary button
        style.configure("Secondary.TButton",
                        background=BG_PANEL, foreground=FG,
                        font=("Segoe UI", 10),
                        borderwidth=1, focusthickness=0, padding=(16, 8))
        style.map("Secondary.TButton",
                  background=[("active", BG_INSET),
                              ("pressed", BG_INSET),
                              ("disabled", BG_PANEL)],
                  foreground=[("disabled", FG_MUTED)])

        # Donate button
        style.configure("Donate.TButton",
                        background=BG, foreground=ACCENT,
                        font=("Segoe UI", 9, "bold"),
                        borderwidth=1, focusthickness=0, padding=(12, 6))
        style.map("Donate.TButton",
                  background=[("active", BG_PANEL), ("pressed", BG_PANEL)],
                  foreground=[("active", ACCENT_HOV)])

        # Tiny browse button
        style.configure("Mini.TButton",
                        background=BG_PANEL, foreground=FG,
                        font=("Segoe UI", 9),
                        borderwidth=1, focusthickness=0, padding=(10, 4))
        style.map("Mini.TButton",
                  background=[("active", BG_INSET)])

        # Radiobuttons (track order toggle)
        style.configure("Toggle.TRadiobutton",
                        background=BG_PANEL, foreground=FG,
                        font=("Segoe UI", 10, "bold"),
                        indicatorsize=0, focusthickness=0,
                        padding=(12, 8))
        style.map("Toggle.TRadiobutton",
                  background=[("selected", ACCENT), ("active", BG_INSET)],
                  foreground=[("selected", ACCENT_FG)])

        # Mode tabs (radiobuttons used as segmented buttons)
        style.configure("ModeTab.TRadiobutton",
                        background=BG_INSET, foreground=FG_DIM,
                        font=("Segoe UI", 10, "bold"),
                        indicatorsize=0, focusthickness=0,
                        padding=(22, 10))
        style.map("ModeTab.TRadiobutton",
                  background=[("selected", ACCENT), ("active", BG_PANEL)],
                  foreground=[("selected", ACCENT_FG), ("active", FG)])

        # Checkbutton
        style.configure("Opt.TCheckbutton",
                        background=BG_PANEL, foreground=FG,
                        font=("Segoe UI", 9),
                        focusthickness=0,
                        padding=(4, 3))
        style.map("Opt.TCheckbutton",
                  background=[("active", BG_PANEL)])

        # Treeview
        style.configure("Modern.Treeview",
                        background=BG_ROW, foreground=FG,
                        fieldbackground=BG_ROW, borderwidth=0,
                        font=("Segoe UI", 9), rowheight=24)
        style.configure("Modern.Treeview.Heading",
                        background=BG_INSET, foreground=FG_DIM,
                        font=("Segoe UI", 9, "bold"),
                        borderwidth=0, relief="flat", padding=(10, 6))
        style.map("Modern.Treeview",
                  background=[("selected", ACCENT)],
                  foreground=[("selected", ACCENT_FG)])

        # Progressbar
        style.configure("Accent.Horizontal.TProgressbar",
                        troughcolor=BG_INSET, background=ACCENT,
                        borderwidth=0, thickness=8)

        # LabelFrame
        style.configure("Card.TLabelframe",
                        background=BG_PANEL, foreground=FG_DIM,
                        borderwidth=1, relief="solid",
                        bordercolor=BORDER)
        style.configure("Card.TLabelframe.Label",
                        background=BG_PANEL, foreground=FG,
                        font=("Segoe UI", 10, "bold"))

        # Combobox
        style.configure("Lang.TCombobox",
                        fieldbackground=BG_PANEL, background=BG_PANEL,
                        foreground=FG, bordercolor=BORDER,
                        arrowcolor=FG_DIM, selectforeground=FG,
                        selectbackground=BG_PANEL,
                        padding=(6, 4))

    # ---------- build UI ----------
    def _build_ui(self) -> None:
        outer = ttk.Frame(self.root, style="TFrame", padding=(20, 14))
        outer.pack(fill="both", expand=True)

        outer.grid_columnconfigure(0, weight=1)
        # Rows: 0 topbar (language), 1 header, 2 mode_tabs, 3 drop,
        #       4 controls card, 5 info, 6 tree (expand), 7 progress,
        #       8 bottom buttons, 9 footer, 10 footer disclaimer
        outer.grid_rowconfigure(6, weight=1)

        # ------- Top bar with language selector (row 0) -------
        topbar = ttk.Frame(outer, style="TFrame")
        topbar.grid(row=0, column=0, sticky="ew", pady=(0, 4))
        topbar.grid_columnconfigure(0, weight=1)   # pushes lang_frame to the right

        # Language selector on the right
        lang_frame = ttk.Frame(topbar, style="TFrame")
        lang_frame.grid(row=0, column=1, sticky="e")
        self.lang_label = ttk.Label(lang_frame, text=t("language_label", self.lang),
                                     style="LangLabel.TLabel")
        self.lang_label.pack(side="left", padx=(0, 6))
        self.lang_combo = ttk.Combobox(
            lang_frame, textvariable=self.lang_var,
            values=list(LANGUAGES.values()),
            state="readonly", width=12, style="Lang.TCombobox"
        )
        self.lang_combo.pack(side="left")
        self.lang_combo.bind("<<ComboboxSelected>>", self._on_lang_changed)

        # ------- Header (centered, row 1) -------
        header = ttk.Frame(outer, style="TFrame")
        header.grid(row=1, column=0, sticky="ew", pady=(0, 10))
        header.grid_columnconfigure(0, weight=1)

        title_row = ttk.Frame(header, style="TFrame")
        title_row.grid(row=0, column=0)
        try:
            icon_png = _resource_path("docs/icon_32.png")
            if icon_png.exists():
                self._logo_photo = tk.PhotoImage(file=str(icon_png))
                tk.Label(title_row, image=self._logo_photo, bg=BG).pack(
                    side="left", padx=(0, 10)
                )
        except Exception:
            pass
        ttk.Label(title_row, text="FLP",
                  style="TitleAccent.TLabel").pack(side="left")
        ttk.Label(title_row, text=" Organizer",
                  style="Title.TLabel").pack(side="left")
        ttk.Label(title_row, text=f"  v{APP_VERSION}",
                  style="Subtitle.TLabel").pack(side="left", padx=(6, 0))

        self.subtitle_label = ttk.Label(
            header, text=t("app_subtitle", self.lang),
            style="Subtitle.TLabel", wraplength=820, justify="center"
        )
        self.subtitle_label.grid(row=1, column=0, pady=(6, 0))

        # ------- Mode tabs (Single / Batch) -------
        mode_bar = ttk.Frame(outer, style="TFrame")
        mode_bar.grid(row=2, column=0, sticky="ew", pady=(4, 10))
        mode_bar.grid_columnconfigure(0, weight=1)
        mode_bar.grid_columnconfigure(3, weight=1)

        self.mode_var = tk.StringVar(value="single")
        self.single_tab_btn = ttk.Radiobutton(
            mode_bar, text=t("mode_single", self.lang),
            variable=self.mode_var, value="single",
            style="ModeTab.TRadiobutton",
            command=self._on_mode_changed,
        )
        self.single_tab_btn.grid(row=0, column=1, padx=(0, 4))

        self.batch_tab_btn = ttk.Radiobutton(
            mode_bar, text=t("mode_batch", self.lang),
            variable=self.mode_var, value="batch",
            style="ModeTab.TRadiobutton",
            command=self._on_mode_changed,
        )
        self.batch_tab_btn.grid(row=0, column=2, padx=(4, 0))

        # ------- Drop area -------
        self.drop_area = tk.Frame(
            outer, bg=BG_PANEL, height=80,
            highlightbackground=BORDER, highlightthickness=1, cursor="hand2",
        )
        self.drop_area.grid(row=3, column=0, sticky="ew", pady=(0, 10))
        self.drop_area.grid_propagate(False)
        self.drop_area.bind("<Button-1>", lambda e: self._pick_files())
        self.drop_area.bind("<Enter>", lambda e: self._drop_hover(True))
        self.drop_area.bind("<Leave>", lambda e: self._drop_hover(False))

        self.drop_label = tk.Label(
            self.drop_area, text=t("drop_prompt", self.lang),
            bg=BG_PANEL, fg=FG_DIM, font=("Segoe UI", 11, "bold"),
        )
        self.drop_label.place(relx=0.5, rely=0.5, anchor="center")
        self.drop_label.bind("<Button-1>", lambda e: self._pick_files())

        # ------- Controls card -------
        controls_card = ttk.Labelframe(
            outer, text="", style="Card.TLabelframe", padding=14
        )
        controls_card.grid(row=4, column=0, sticky="ew", pady=(0, 10))
        controls_card.grid_columnconfigure(0, weight=1)
        controls_card.grid_columnconfigure(1, weight=1)

        # Left column: Track order + sub-sort
        left_col = ttk.Frame(controls_card, style="Panel.TFrame")
        left_col.grid(row=0, column=0, sticky="nsew", padx=(0, 10))

        self.track_order_label = ttk.Label(
            left_col, text=t("track_order", self.lang), style="Section.TLabel"
        )
        self.track_order_label.pack(anchor="w", pady=(0, 6))

        radio_row = ttk.Frame(left_col, style="Panel.TFrame")
        radio_row.pack(fill="x")
        self.sort_alpha_btn = ttk.Radiobutton(
            radio_row, text=f"  {t('sort_alpha', self.lang)}  ",
            variable=self.sort_mode_var, value="alpha",
            style="Toggle.TRadiobutton",
            command=self._on_sort_changed,
        )
        self.sort_alpha_btn.pack(side="left", padx=(0, 6))
        self.sort_first_btn = ttk.Radiobutton(
            radio_row, text=f"  {t('sort_first', self.lang)}  ",
            variable=self.sort_mode_var, value="first",
            style="Toggle.TRadiobutton",
            command=self._on_sort_changed,
        )
        self.sort_first_btn.pack(side="left")

        self.sort_caption = ttk.Label(
            left_col, text=t("caption_alpha", self.lang),
            style="Caption.TLabel", wraplength=380, justify="left"
        )
        self.sort_caption.pack(anchor="w", pady=(10, 8), fill="x")

        self.subsort_label = ttk.Label(
            left_col, text=t("subsort_label", self.lang), style="Section.TLabel"
        )
        self.subsort_label.pack(anchor="w", pady=(4, 4))

        self.sub_type_cb = ttk.Checkbutton(
            left_col, text=t("sub_by_type", self.lang),
            variable=self.sub_type_var, style="Opt.TCheckbutton",
            command=self._on_sort_changed,
        )
        self.sub_type_cb.pack(anchor="w")
        self.sub_length_cb = ttk.Checkbutton(
            left_col, text=t("sub_by_length", self.lang),
            variable=self.sub_length_var, style="Opt.TCheckbutton",
            command=self._on_sort_changed,
        )
        self.sub_length_cb.pack(anchor="w")
        self.sub_color_cb = ttk.Checkbutton(
            left_col, text=t("sub_by_color", self.lang),
            variable=self.sub_color_var, style="Opt.TCheckbutton",
            command=self._on_sort_changed,
        )
        self.sub_color_cb.pack(anchor="w")
        # sub_color is now enabled in v1.4.0

        # Right column: Post-process options + batch output dir
        right_col = ttk.Frame(controls_card, style="Panel.TFrame")
        right_col.grid(row=0, column=1, sticky="nsew", padx=(10, 0))

        self.postprocess_label = ttk.Label(
            right_col, text=t("postprocess_label", self.lang),
            style="Section.TLabel"
        )
        self.postprocess_label.pack(anchor="w", pady=(0, 6))

        self.opt_rename_cb = ttk.Checkbutton(
            right_col, text=t("opt_rename_tracks", self.lang),
            variable=self.opt_rename_var, style="Opt.TCheckbutton",
            command=self._on_sort_changed,
        )
        self.opt_rename_cb.pack(anchor="w")
        self.opt_color_cb = ttk.Checkbutton(
            right_col, text=t("opt_color_tracks", self.lang),
            variable=self.opt_color_var, style="Opt.TCheckbutton",
            command=self._on_sort_changed,
        )
        self.opt_color_cb.pack(anchor="w")
        # opt_color enabled in v1.5.0

        self.opt_remove_empty_cb = ttk.Checkbutton(
            right_col, text=t("opt_remove_empty", self.lang),
            variable=self.opt_remove_empty_var, style="Opt.TCheckbutton",
        )
        self.opt_remove_empty_cb.pack(anchor="w")
        self.opt_remove_empty_cb.state(["disabled"])   # coming soon

        # Batch output dir row (only visible in batch mode)
        self.batch_output_frame = ttk.Frame(right_col, style="Panel.TFrame")
        # don't pack initially — single mode is default

        self.batch_output_label = ttk.Label(
            self.batch_output_frame,
            text=t("batch_output_folder", self.lang), style="Caption.TLabel"
        )
        self.batch_output_label.pack(anchor="w", pady=(10, 2))

        out_row = ttk.Frame(self.batch_output_frame, style="Panel.TFrame")
        out_row.pack(fill="x")
        self.batch_output_display = ttk.Label(
            out_row, text=t("batch_default_folder", self.lang),
            style="Caption.TLabel"
        )
        self.batch_output_display.pack(side="left", fill="x", expand=True)
        self.batch_browse_btn = ttk.Button(
            out_row, text=t("batch_browse", self.lang),
            style="Mini.TButton", command=self._pick_batch_output_dir,
        )
        self.batch_browse_btn.pack(side="right", padx=(8, 0))

        # ------- Info bar -------
        self.info_label = ttk.Label(outer, text=t("no_file", self.lang),
                                     style="Subtitle.TLabel")
        self.info_label.grid(row=5, column=0, sticky="w", pady=(0, 6))

        # ------- Tree preview -------
        tree_wrap = tk.Frame(outer, bg=BG_PANEL,
                             highlightbackground=BORDER, highlightthickness=1)
        tree_wrap.grid(row=6, column=0, sticky="nsew", pady=(0, 10))
        tree_wrap.grid_rowconfigure(0, weight=1)
        tree_wrap.grid_columnconfigure(0, weight=1)

        columns = ("tracks", "count", "name")
        self.tree = ttk.Treeview(
            tree_wrap, columns=columns, show="headings",
            style="Modern.Treeview"
        )
        self._apply_tree_headings()
        self.tree.column("tracks", width=110, anchor="center", stretch=False)
        self.tree.column("count",  width=80,  anchor="center", stretch=False)
        self.tree.column("name",   width=500, anchor="w")
        self.tree.tag_configure("odd",  background=BG_ROW)
        self.tree.tag_configure("even", background=BG_ROW_ALT)

        vscroll = ttk.Scrollbar(tree_wrap, orient="vertical",
                                command=self.tree.yview)
        self.tree.configure(yscrollcommand=vscroll.set)
        self.tree.grid(row=0, column=0, sticky="nsew")
        vscroll.grid(row=0, column=1, sticky="ns")

        # ------- Progress -------
        self.progress = ttk.Progressbar(
            outer, mode="determinate", maximum=100,
            style="Accent.Horizontal.TProgressbar"
        )

        # ------- Bottom button bar -------
        bottom = ttk.Frame(outer, style="TFrame")
        bottom.grid(row=8, column=0, sticky="ew", pady=(0, 4))
        bottom.grid_columnconfigure(0, weight=1)

        self.status_label = ttk.Label(bottom, text="", style="Status.TLabel")
        self.status_label.grid(row=0, column=0, sticky="w")

        btn_row = ttk.Frame(bottom, style="TFrame")
        btn_row.grid(row=0, column=1, sticky="e")

        self.clear_btn = ttk.Button(
            btn_row, text=t("btn_clear", self.lang),
            style="Secondary.TButton", command=self._clear, state="disabled",
        )
        self.clear_btn.pack(side="left", padx=(0, 10))

        self.apply_btn = ttk.Button(
            btn_row, text=t("btn_apply", self.lang),
            style="Accent.TButton", command=self._apply, state="disabled",
        )
        self.apply_btn.pack(side="left")

        # ------- Footer -------
        footer = ttk.Frame(outer, style="TFrame")
        footer.grid(row=9, column=0, sticky="ew", pady=(12, 0))
        footer.grid_columnconfigure(0, weight=1)
        footer.grid_columnconfigure(2, weight=1)

        footer_center = ttk.Frame(footer, style="TFrame")
        footer_center.grid(row=0, column=1)

        self.footer_made = ttk.Label(footer_center, text=t("footer_made_with", self.lang),
                                      style="Footer.TLabel")
        self.footer_made.pack(side="left")
        ttk.Label(footer_center, text="🧡", style="FooterHeart.TLabel").pack(side="left")
        self.footer_by = ttk.Label(footer_center, text=t("footer_by", self.lang, name=AUTHOR),
                                    style="Footer.TLabel")
        self.footer_by.pack(side="left", padx=(0, 12))

        self.donate_btn = ttk.Button(footer_center, text=t("btn_donate", self.lang),
                                      style="Donate.TButton",
                                      command=self._open_donation)
        self.donate_btn.pack(side="left")

        self.footer_disclaimer_label = ttk.Label(
            outer, text=t("footer_disclaimer", self.lang),
            style="Caption.TLabel"
        )
        self.footer_disclaimer_label.grid(row=10, column=0, pady=(6, 0))

        # Set initial language value in combobox
        self.lang_combo.set(LANGUAGES[self.lang])

    def _apply_tree_headings(self) -> None:
        self.tree.heading("tracks", text=t("col_track", self.lang))
        self.tree.heading("count",  text=t("col_clips", self.lang))
        self.tree.heading("name",   text=t("col_name", self.lang))

    # ---------- translations refresh ----------
    def _retranslate_ui(self) -> None:
        """Refresh every user-visible string in the current language."""
        L = self.lang
        self.subtitle_label.configure(text=t("app_subtitle", L))
        self.lang_label.configure(text=t("language_label", L))

        # Mode tabs
        self.single_tab_btn.configure(text=t("mode_single", L))
        self.batch_tab_btn.configure(text=t("mode_batch", L))

        # Drop label depends on mode
        self._refresh_drop_label()

        # Track order section
        self.track_order_label.configure(text=t("track_order", L))
        self.sort_alpha_btn.configure(text=f"  {t('sort_alpha', L)}  ")
        self.sort_first_btn.configure(text=f"  {t('sort_first', L)}  ")
        self.sort_caption.configure(text=self._sort_caption_text())

        # Sub-sort
        self.subsort_label.configure(text=t("subsort_label", L))
        self.sub_type_cb.configure(text=t("sub_by_type", L))
        self.sub_length_cb.configure(text=t("sub_by_length", L))
        self.sub_color_cb.configure(text=t("sub_by_color", L))

        # Post-process
        self.postprocess_label.configure(text=t("postprocess_label", L))
        self.opt_rename_cb.configure(text=t("opt_rename_tracks", L))
        self.opt_color_cb.configure(text=t("opt_color_tracks", L))
        self.opt_remove_empty_cb.configure(text=t("opt_remove_empty", L))

        # Batch fields
        self.batch_output_label.configure(text=t("batch_output_folder", L))
        self.batch_browse_btn.configure(text=t("batch_browse", L))
        if self.batch_output_dir is None:
            self.batch_output_display.configure(text=t("batch_default_folder", L))

        # Tree headings
        self._apply_tree_headings()

        # Info
        if self.current_path is None and not self.batch_paths:
            self.info_label.configure(text=t("no_file", L))

        # Buttons
        self.clear_btn.configure(text=t("btn_clear", L))
        self.apply_btn.configure(text=t("btn_apply", L))
        self.donate_btn.configure(text=t("btn_donate", L))

        # Footer
        self.footer_made.configure(text=t("footer_made_with", L))
        self.footer_by.configure(text=t("footer_by", L, name=AUTHOR))
        self.footer_disclaimer_label.configure(text=t("footer_disclaimer", L))

    def _refresh_drop_label(self) -> None:
        L = self.lang
        if self.mode == "batch":
            if self.batch_paths:
                self.drop_label.configure(text=t("batch_loaded", L, count=len(self.batch_paths)))
            else:
                self.drop_label.configure(text=t("drop_prompt_batch", L))
        else:
            if self.current_path:
                self.drop_label.configure(text=t("drop_loaded", L, name=self.current_path.name))
            else:
                self.drop_label.configure(text=t("drop_prompt", L))

    def _sort_caption_text(self) -> str:
        return (t("caption_first", self.lang)
                if self.sort_mode_var.get() == "first"
                else t("caption_alpha", self.lang))

    # ---------- helpers ----------
    def _drop_hover(self, hovering: bool) -> None:
        color = ACCENT if hovering else BORDER
        try:
            self.drop_area.configure(highlightbackground=color)
        except tk.TclError:
            pass

    def _set_status(self, text: str, kind: str = "dim") -> None:
        styles = {"dim": "Status.TLabel", "ok": "StatusOK.TLabel",
                  "warn": "StatusWarn.TLabel", "err": "StatusErr.TLabel"}
        self.status_label.configure(text=text, style=styles.get(kind, "Status.TLabel"))

    def _update_compat_banner(self) -> None:
        """Show compatibility report results in a popup the first time, and
        keep a status hint visible afterwards. Errors disable Apply&Save until
        the user explicitly confirms via the popup."""
        v = self.current_validation
        if v is None or v.overall_severity == flp_core.SEVERITY_OK:
            self._user_overrode_validation = False
            return

        # Build the message body
        L = self.lang
        lines = []
        for issue in v.issues:
            symbol = "❌" if issue.severity == flp_core.SEVERITY_ERROR else "⚠"
            lines.append(f"{symbol}  {issue.message}")
            if issue.details:
                lines.append(f"     {issue.details}")
            lines.append("")
        body = "\n".join(lines).rstrip()

        if v.has_errors:
            # Errors → show ERROR-level dialog with "Proceed anyway?" choice
            title = t("compat_errors_title", L)
            prompt = t("compat_errors_prompt", L)
            full = f"{prompt}\n\n{body}\n\n{t('compat_errors_question', L)}"
            answer = messagebox.askyesno(APP_NAME, full, icon="warning")
            self._user_overrode_validation = bool(answer)
            if answer:
                # User chose to proceed — re-enable apply via _on_analyze_done
                # rerun (it'll see _user_overrode_validation = True)
                if self.current_result is not None:
                    has_changes = len(self.current_result._patches) > 0
                    self.apply_btn.configure(state=("normal" if has_changes else "disabled"))
                self._set_status(t("compat_proceeding_warning", L), kind="warn")
            else:
                self._set_status(t("compat_blocked", L), kind="err")
        elif v.has_warnings:
            # Warnings → show INFO-level dialog (one click to dismiss)
            title = t("compat_warnings_title", L)
            prompt = t("compat_warnings_prompt", L)
            messagebox.showinfo(APP_NAME, f"{prompt}\n\n{body}")
            self._set_status(t("compat_warnings_status", L), kind="warn")
            self._user_overrode_validation = False

    def _open_donation(self) -> None:
        try:
            webbrowser.open(PAYPAL_URL)
        except Exception:
            messagebox.showinfo(APP_NAME,
                                f"Please open this URL in your browser:\n\n{PAYPAL_URL}")

    def _get_sub_sort(self) -> list[str]:
        result = []
        if self.sub_type_var.get():
            result.append(flp_core.SUB_BY_TYPE)
        if self.sub_length_var.get():
            result.append(flp_core.SUB_BY_LENGTH)
        if self.sub_color_var.get():
            result.append(flp_core.SUB_BY_COLOR)
        return result

    def _current_sort_mode(self) -> str:
        return (flp_core.SORT_BY_FIRST_APPEARANCE
                if self.sort_mode_var.get() == "first"
                else flp_core.SORT_ALPHABETICAL)

    # ---------- event handlers ----------
    def _on_lang_changed(self, _event=None) -> None:
        # Find code from display name
        display = self.lang_var.get()
        new_lang = next((code for code, name in LANGUAGES.items() if name == display),
                        DEFAULT_LANG)
        if new_lang != self.lang:
            self.lang = new_lang
            self.config["language"] = new_lang
            save_config(self.config)
            self._retranslate_ui()

    def _on_mode_changed(self) -> None:
        self.mode = self.mode_var.get()
        if self.mode == "batch":
            self.batch_output_frame.pack(fill="x", pady=(8, 0))
        else:
            self.batch_output_frame.pack_forget()
        # Clear state when switching modes
        self._clear(keep_mode=True)

    def _on_sort_changed(self) -> None:
        self.sort_caption.configure(text=self._sort_caption_text())
        # Recompute only in single mode (batch mode doesn't show preview)
        if self.mode == "single" and self.current_path is not None:
            threading.Thread(target=self._analyze_worker,
                             args=(self.current_path,), daemon=True).start()
            self.info_label.configure(text=t("recomputing", self.lang))

    # ---------- file picking ----------
    def _on_drop(self, event) -> None:
        paths = self.root.tk.splitlist(event.data)
        valid = [Path(p) for p in paths if p.lower().endswith(".flp")]
        if not valid:
            return
        if self.mode == "batch":
            self._load_batch(valid)
        else:
            self._load_single(valid[0])

    def _pick_files(self) -> None:
        if self.mode == "batch":
            names = filedialog.askopenfilenames(
                title="Select up to 30 .flp files",
                filetypes=[("FL Studio projects", "*.flp")],
            )
            if names:
                self._load_batch([Path(n) for n in names])
        else:
            fname = filedialog.askopenfilename(
                title="Select an FL Studio project file",
                filetypes=[("FL Studio projects", "*.flp"), ("All files", "*.*")],
            )
            if fname:
                self._load_single(Path(fname))

    def _pick_batch_output_dir(self) -> None:
        d = filedialog.askdirectory(title="Choose output folder for batch")
        if d:
            self.batch_output_dir = Path(d)
            self.batch_output_display.configure(text=str(self.batch_output_dir))
        else:
            self.batch_output_dir = None
            self.batch_output_display.configure(text=t("batch_default_folder", self.lang))

    # ---------- single-file flow ----------
    def _load_single(self, path: Path) -> None:
        if not path.exists() or path.suffix.lower() != ".flp":
            messagebox.showerror(APP_NAME, f"Not a valid .flp file:\n{path}")
            return
        self.current_path = path
        self.info_label.configure(text=t("loading", self.lang, name=path.name))
        self._set_status("")
        self.tree.delete(*self.tree.get_children())
        self.apply_btn.configure(state="disabled")
        self.clear_btn.configure(state="disabled")
        self._refresh_drop_label()
        threading.Thread(target=self._analyze_worker, args=(path,), daemon=True).start()

    def _analyze_worker(self, path: Path) -> None:
        try:
            # Validate compatibility first — read-only, never modifies
            validation = flp_core.validate_compatibility(path)
            # Even if validation has errors, we still try to analyze (so the
            # user sees a preview), but the GUI will refuse Apply&Save until
            # they confirm.
            result = flp_core.analyze(
                path, sort_mode=self._current_sort_mode(),
                sub_sort=self._get_sub_sort(),
                apply_auto_color=bool(self.opt_color_var.get()),
                apply_auto_rename=bool(self.opt_rename_var.get()),
            )
        except Exception as e:
            self.root.after(0, lambda: self._on_analyze_error(e))
            return
        self.root.after(0, lambda: self._on_analyze_done(result, validation))

    def _on_analyze_error(self, e: Exception) -> None:
        self.info_label.configure(text=t("failed_read", self.lang))
        messagebox.showerror(APP_NAME, f"{t('failed_read', self.lang)}\n\n{e}")
        self.current_path = None
        self._refresh_drop_label()

    def _on_analyze_done(self, result: flp_core.AnalysisResult,
                         validation: Optional[flp_core.ValidationReport] = None) -> None:
        self.current_result = result
        self.current_validation = validation
        self.tree.delete(*self.tree.get_children())
        assert self.current_path is not None

        # Update compatibility banner if applicable
        self._update_compat_banner()

        info = (f"{self.current_path.name}    •    FL {result.fl_version}    •    "
                f"{result.total_clips} clips    •    "
                f"{len(result.groups)} groups    •    "
                f"{result.total_tracks_needed} tracks    •    "
                f"{len(result._patches)} clips move")
        self.info_label.configure(text=info)

        for i, g in enumerate(result.groups):
            track_txt = (f"#{g.first_track}" if g.lanes_used == 1
                         else f"#{g.first_track}–{g.first_track + g.lanes_used - 1}")
            tag = "even" if i % 2 == 0 else "odd"
            self.tree.insert("", "end",
                             values=(track_txt, g.clip_count, g.name),
                             tags=(tag,))

        if result.warnings:
            self._set_status("⚠  " + result.warnings[0], kind="warn")
        else:
            self._set_status(t("ready", self.lang), kind="ok")

        has_changes = len(result._patches) > 0
        # Block apply if validation found ERROR-level issues
        block_for_validation = bool(
            validation and validation.has_errors
            and not getattr(self, "_user_overrode_validation", False)
        )
        if block_for_validation:
            self.apply_btn.configure(state="disabled")
        else:
            self.apply_btn.configure(state=("normal" if has_changes else "disabled"))
        self.clear_btn.configure(state="normal")
        if not has_changes:
            self._set_status(t("nothing_to_change", self.lang), kind="dim")

    # ---------- batch flow ----------
    def _load_batch(self, paths: list[Path]) -> None:
        valid = [p for p in paths if p.exists() and p.suffix.lower() == ".flp"]
        if not valid:
            messagebox.showerror(APP_NAME, "No valid .flp files in selection.")
            return
        truncated = False
        if len(valid) > BATCH_LIMIT:
            valid = valid[:BATCH_LIMIT]
            truncated = True
        self.batch_paths = valid
        self.tree.delete(*self.tree.get_children())
        # Populate tree with file list
        for i, p in enumerate(valid):
            tag = "even" if i % 2 == 0 else "odd"
            self.tree.insert("", "end",
                             values=("—", "—", p.name), tags=(tag,))
        self._refresh_drop_label()
        self.info_label.configure(text=f"{len(valid)} files loaded")
        self._set_status(
            t("batch_limit_warn", self.lang) if truncated else "",
            kind="warn" if truncated else "dim"
        )
        self.apply_btn.configure(state="normal")
        self.clear_btn.configure(state="normal")

    # ---------- apply ----------
    def _apply(self) -> None:
        if self.mode == "batch":
            self._apply_batch()
        else:
            self._apply_single()

    def _apply_single(self) -> None:
        if not (self.current_result and self.current_path):
            return
        default_name = self.current_path.stem + "_organized.flp"
        out = filedialog.asksaveasfilename(
            title=t("dlg_save_title", self.lang),
            defaultextension=".flp",
            initialfile=default_name,
            initialdir=str(self.current_path.parent),
            filetypes=[("FL Studio projects", "*.flp")],
        )
        if not out:
            return
        out_path = Path(out)
        try:
            if out_path.resolve() == self.current_path.resolve():
                messagebox.showerror(APP_NAME, t("dlg_err_overwrite", self.lang))
                return
        except Exception:
            pass

        self.apply_btn.configure(state="disabled")
        self.clear_btn.configure(state="disabled")
        self.progress.grid(row=7, column=0, sticky="ew", pady=(0, 6))
        self.progress["value"] = 0
        self._set_status(t("writing", self.lang), kind="dim")
        threading.Thread(target=self._apply_worker_single,
                         args=(out_path,), daemon=True).start()

    def _apply_worker_single(self, out_path: Path) -> None:
        assert self.current_result is not None
        try:
            def prog(done: int, tot: int) -> None:
                self.root.after(0, lambda: self.progress.configure(value=done * 100 / tot))
            flp_core.apply_plan(self.current_result, out_path, progress=prog)
        except Exception as e:
            self.root.after(0, lambda: self._on_apply_error(e))
            return
        self.root.after(0, lambda: self._on_apply_single_done(out_path))

    def _on_apply_error(self, e: Exception) -> None:
        self.progress.grid_remove()
        self._set_status(t("write_failed", self.lang), kind="err")
        self.apply_btn.configure(state="normal")
        self.clear_btn.configure(state="normal")
        messagebox.showerror(APP_NAME, f"{t('write_failed', self.lang)}\n\n{e}")

    def _on_apply_single_done(self, out_path: Path) -> None:
        self.progress.grid_remove()
        self._set_status(t("saved", self.lang, name=out_path.name), kind="ok")
        self.apply_btn.configure(state="normal")
        self.clear_btn.configure(state="normal")
        if messagebox.askyesno(APP_NAME, t("dlg_save_ok", self.lang, path=out_path)):
            self._open_folder(out_path.parent)

    def _apply_batch(self) -> None:
        if not self.batch_paths:
            return
        self.apply_btn.configure(state="disabled")
        self.clear_btn.configure(state="disabled")
        self.progress.grid(row=7, column=0, sticky="ew", pady=(0, 6))
        self.progress["value"] = 0
        threading.Thread(target=self._apply_worker_batch, daemon=True).start()

    def _apply_worker_batch(self) -> None:
        total = len(self.batch_paths)
        ok = 0
        errors: list[tuple[str, str]] = []
        sort_mode = self._current_sort_mode()
        sub_sort = self._get_sub_sort()
        out_dir = self.batch_output_dir
        auto_color = bool(self.opt_color_var.get())
        auto_rename = bool(self.opt_rename_var.get())

        for i, in_path in enumerate(self.batch_paths, 1):
            self.root.after(
                0, lambda i=i, total=total, name=in_path.name:
                self._set_status(t("batch_processing", self.lang,
                                     i=i, n=total, name=name), kind="dim")
            )
            try:
                result = flp_core.analyze(in_path, sort_mode=sort_mode,
                                           sub_sort=sub_sort,
                                           apply_auto_color=auto_color,
                                           apply_auto_rename=auto_rename)
                if out_dir is not None:
                    out_path = out_dir / (in_path.stem + "_organized.flp")
                else:
                    out_path = in_path.with_name(in_path.stem + "_organized.flp")
                flp_core.apply_plan(result, out_path)
                ok += 1
            except Exception as e:
                errors.append((in_path.name, str(e)))

            self.root.after(0, lambda i=i, total=total:
                            self.progress.configure(value=i * 100 / total))

        self.root.after(0, lambda: self._on_apply_batch_done(ok, total, errors))

    def _on_apply_batch_done(self, ok: int, total: int,
                              errors: list[tuple[str, str]]) -> None:
        self.progress.grid_remove()
        self._set_status(t("batch_done", self.lang, ok=ok, total=total), kind="ok")
        self.apply_btn.configure(state="normal")
        self.clear_btn.configure(state="normal")
        if errors:
            msg = "Errors:\n\n" + "\n".join(f"• {n}: {e}" for n, e in errors[:10])
            messagebox.showwarning(APP_NAME, msg)

    def _open_folder(self, folder: Path) -> None:
        try:
            if sys.platform.startswith("win"):
                os.startfile(str(folder))
            elif sys.platform == "darwin":
                os.system(f'open "{folder}"')
            else:
                os.system(f'xdg-open "{folder}"')
        except Exception:
            pass

    def _clear(self, keep_mode: bool = False) -> None:
        self.current_path = None
        self.current_result = None
        self.batch_paths = []
        self.tree.delete(*self.tree.get_children())
        self.info_label.configure(text=t("no_file", self.lang))
        self._set_status("")
        self.apply_btn.configure(state="disabled")
        self.clear_btn.configure(state="disabled")
        self._refresh_drop_label()


def main() -> None:
    if DND_AVAILABLE:
        root = TkinterDnD.Tk()
    else:
        root = tk.Tk()
    app = FlpOrganizerApp(root)
    root.mainloop()


if __name__ == "__main__":
    main()
