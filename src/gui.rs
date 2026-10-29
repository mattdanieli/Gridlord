//! Interfaccia grafica (inglese, cinese, tedesco, spagnolo, francese, italiano, russo).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontFamily, FontId, Frame, Layout, Margin, Pos2, Rect,
    RichText, Sense, Stroke, TextStyle, Vec2,
};
use egui_extras::{Column, TableBuilder};
use serde::{Deserialize, Serialize};

use crate::engine::check::{self, Issue, Level};
use crate::engine::flp::FlpError;
use crate::engine::organize::{self, Options, Plan, SortMode, SubSort, Warning};
use crate::engine::project::{self, ClipKind, Project};
use crate::engine::system;
use crate::engine::write::{self, SaveError, Saved, Target};
use crate::i18n;

pub const APP_NAME: &str = "Gridlord";
pub const APP_SUBTITLE: &str = "Playlist Organizer for FL Studio";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

// --------------------------------------------------------------------------
// Lingua
// --------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum Lang {
    #[default]
    En,
    Zh,
    De,
    Es,
    Fr,
    It,
    Ru,
}

impl Lang {
    /// Ordine del menu: l'inglese (lingua di partenza) e poi le altre.
    const ALL: [Lang; 7] = [
        Lang::En,
        Lang::Zh,
        Lang::De,
        Lang::Es,
        Lang::Fr,
        Lang::It,
        Lang::Ru,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Zh => "中文",
            Lang::De => "Deutsch",
            Lang::Es => "Español",
            Lang::Fr => "Français",
            Lang::It => "Italiano",
            Lang::Ru => "Русский",
        }
    }

    fn code(self) -> &'static str {
        match self {
            Lang::En => "EN",
            Lang::Zh => "中文",
            Lang::De => "DE",
            Lang::Es => "ES",
            Lang::Fr => "FR",
            Lang::It => "IT",
            Lang::Ru => "RU",
        }
    }

    /// Sceglie il testo nella lingua attiva. Inglese e italiano sono scritti nel codice,
    /// le altre lingue stanno in i18n.rs (con l'inglese come chiave e come riserva).
    fn t(self, en: &'static str, it: &'static str) -> &'static str {
        match self {
            Lang::En => en,
            Lang::It => it,
            _ => i18n::get(self, en).unwrap_or(en),
        }
    }

    /// Come `t`, poi sostituisce {0}, {1}, ... con i valori.
    fn f(self, en: &'static str, it: &'static str, args: &[&dyn std::fmt::Display]) -> String {
        let mut s = self.t(en, it).to_string();
        for (i, a) in args.iter().enumerate() {
            s = s.replace(&format!("{{{i}}}"), &a.to_string());
        }
        s
    }
}

fn kind_label(l: Lang, k: ClipKind) -> &'static str {
    match k {
        ClipKind::Audio => l.t("Audio", "Audio"),
        ClipKind::Pattern => l.t("Pattern", "Pattern"),
        ClipKind::Automation => l.t("Automation", "Automazione"),
    }
}

fn error_text(l: Lang, e: &FlpError) -> String {
    match e {
        FlpError::Io(m) => l.f("Unable to read the file: {0}", "Impossibile leggere il file: {0}", &[m]),
        FlpError::NotFlp => l.t("This is not an FL Studio project (.flp).", "Questo non è un progetto di FL Studio (.flp).").into(),
        FlpError::Truncated => l
            .t(
                "The project file is incomplete. Save it again in FL Studio and retry.",
                "Il file del progetto è incompleto. Salvalo di nuovo in FL Studio e riprova.",
            )
            .into(),
        FlpError::Corrupt { .. } => l
            .t(
                "Part of this project could not be read. It was left untouched to keep it safe.",
                "Una parte di questo progetto non è leggibile. Non è stato toccato, per sicurezza.",
            )
            .into(),
        FlpError::VerifyFailed(_) => l
            .t(
                "The safety check on the result failed, so nothing was saved. Your project is untouched.",
                "Il controllo di sicurezza sul risultato non è andato a buon fine, quindi non è stato salvato nulla. Il progetto è intatto.",
            )
            .into(),
    }
}

fn save_error_text(l: Lang, e: &SaveError) -> String {
    match e {
        SaveError::ChangedOnDisk => l
            .t(
                "The project was changed after it was loaded (maybe saved again in FL Studio). It has been reloaded: check the preview and save again.",
                "Il progetto è stato modificato dopo il caricamento (forse salvato di nuovo in FL Studio). È stato ricaricato: controlla l'anteprima e salva di nuovo.",
            )
            .into(),
        SaveError::Flp(e) => error_text(l, e),
    }
}

fn issue_text(l: Lang, i: &Issue) -> String {
    match i {
        Issue::UntestedVersion(v) => l.f(
            "Saved with FL Studio {0}, a version not tested yet. It will most likely work: check the result in FL Studio.",
            "Salvato con FL Studio {0}, una versione non ancora provata. Molto probabilmente funziona: controlla il risultato in FL Studio.",
            &[v],
        ),
        Issue::TooOld(v) => l.f(
            "Saved with FL Studio {0}. Projects from versions older than FL Studio 20.7 are not supported. Open and save it in a newer version first.",
            "Salvato con FL Studio {0}. I progetti delle versioni precedenti a FL Studio 20.7 non sono supportati. Aprilo e salvalo prima in una versione più recente.",
            &[v],
        ),
        Issue::NoPlaylist => l.t("No playlist was found in this project.", "In questo progetto non c'è nessuna playlist.").into(),
        Issue::NoClips => l.t("The playlist is empty: there is nothing to organize.", "La playlist è vuota: non c'è niente da organizzare.").into(),
        Issue::UnknownItemSize => l
            .t(
                "The playlist uses a layout this version does not recognize. Its clips are left untouched.",
                "La playlist usa una struttura che questa versione non riconosce. Le sue clip non vengono toccate.",
            )
            .into(),
        Issue::UnusualTrackData => l
            .t(
                "Some playlist tracks have an unusual layout. Track names and mute states may need a check.",
                "Alcune tracce della playlist hanno una struttura insolita. Nomi e stato muto delle tracce potrebbero richiedere un controllo.",
            )
            .into(),
        Issue::TrackCount(n) => l.f(
            "The playlist has {0} tracks instead of 500. Check the result in FL Studio.",
            "La playlist ha {0} tracce invece di 500. Controlla il risultato in FL Studio.",
            &[n],
        ),
        Issue::TrailingData => l
            .t("The file has extra data at the end. It is kept as it is.", "Il file ha dati in più alla fine. Vengono mantenuti così come sono.")
            .into(),
        Issue::MultipleArrangements(n) => l.f(
            "{0} arrangements: each one is organized separately.",
            "{0} arrangement: ognuno viene organizzato separatamente.",
            &[n],
        ),
    }
}

fn warning_text(l: Lang, w: &Warning) -> String {
    match w {
        Warning::TooManyTracks { needed, .. } => l.f(
            "This layout needs {0} tracks but the playlist has 500. Clips that do not fit stay where they are.",
            "Questo ordine richiede {0} tracce ma la playlist ne ha 500. Le clip che non ci stanno restano dove sono.",
            &[needed],
        ),
    }
}

// --------------------------------------------------------------------------
// Colori
// --------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Palette {
    bg: Color32,
    panel: Color32,
    card: Color32,
    card_hi: Color32,
    border: Color32,
    text: Color32,
    weak: Color32,
    accent: Color32,
    on_accent: Color32,
    green: Color32,
    red: Color32,
    yellow: Color32,
    blue: Color32,
    gray: Color32,
    lane: Color32,
    lane_alt: Color32,
}

const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x14, 0x16, 0x1b),
    panel: Color32::from_rgb(0x1a, 0x1d, 0x23),
    card: Color32::from_rgb(0x1f, 0x23, 0x2b),
    card_hi: Color32::from_rgb(0x2a, 0x2f, 0x39),
    border: Color32::from_rgb(0x30, 0x35, 0x40),
    text: Color32::from_rgb(0xe8, 0xea, 0xf0),
    weak: Color32::from_rgb(0x98, 0x9f, 0xb0),
    accent: Color32::from_rgb(0xff, 0x8a, 0x2b),
    on_accent: Color32::from_rgb(0x1a, 0x10, 0x05),
    green: Color32::from_rgb(0x3e, 0xcf, 0x8e),
    red: Color32::from_rgb(0xff, 0x5f, 0x6d),
    yellow: Color32::from_rgb(0xf2, 0xc1, 0x4e),
    blue: Color32::from_rgb(0x5a, 0xaa, 0xff),
    gray: Color32::from_rgb(0x80, 0x87, 0x96),
    lane: Color32::from_rgb(0x1b, 0x1e, 0x25),
    lane_alt: Color32::from_rgb(0x21, 0x25, 0x2d),
};

const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xf3, 0xf4, 0xf7),
    panel: Color32::from_rgb(0xfa, 0xfa, 0xfc),
    card: Color32::from_rgb(0xff, 0xff, 0xff),
    card_hi: Color32::from_rgb(0xee, 0xf0, 0xf5),
    border: Color32::from_rgb(0xdd, 0xe1, 0xe8),
    text: Color32::from_rgb(0x1c, 0x1f, 0x26),
    weak: Color32::from_rgb(0x5c, 0x63, 0x73),
    accent: Color32::from_rgb(0xe8, 0x6e, 0x12),
    on_accent: Color32::WHITE,
    green: Color32::from_rgb(0x12, 0x9a, 0x5c),
    red: Color32::from_rgb(0xd6, 0x33, 0x45),
    yellow: Color32::from_rgb(0xb0, 0x7c, 0x00),
    blue: Color32::from_rgb(0x1f, 0x6f, 0xd6),
    gray: Color32::from_rgb(0x77, 0x7e, 0x8c),
    lane: Color32::from_rgb(0xf4, 0xf5, 0xf8),
    lane_alt: Color32::from_rgb(0xea, 0xec, 0xf1),
};

fn tint(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// Colore di un gruppo nell'anteprima: il suo colore vero, se e scelto dall'utente
/// o arcobaleno, altrimenti uno ricavato dalla posizione (solo per l'anteprima).
fn group_color(g: &organize::Group, i: usize, n: usize) -> Color32 {
    match g.color {
        Some(c) if !organize::is_default_color(c) => rgb(c),
        _ => rgb(organize::rainbow(i, n.max(1))),
    }
}

// --------------------------------------------------------------------------
// Impostazioni salvate tra un avvio e l'altro
// --------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
enum OutputMode {
    #[default]
    NextToOriginal,
    Folder,
    Replace,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Settings {
    options: Options,
    output: OutputMode,
    out_folder: Option<PathBuf>,
    dark: bool,
    /// Lingua scelta dall'utente (si parte sempre in inglese).
    language: Lang,
    show_after: bool,
    last_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            options: Options::default(),
            output: OutputMode::NextToOriginal,
            out_folder: None,
            dark: true,
            language: Lang::En,
            show_after: true,
            last_dir: None,
        }
    }
}

// --------------------------------------------------------------------------
// Stato
// --------------------------------------------------------------------------

struct Ready {
    project: Arc<Project>,
    issues: Vec<Issue>,
    plan: Plan,
}

enum State {
    Loading,
    Ready(Box<Ready>),
    Failed(FlpError),
}

enum Outcome {
    Saved(Saved),
    Failed(String),
}

struct Item {
    path: PathBuf,
    state: State,
    outcome: Option<Outcome>,
}

impl Item {
    fn name(&self) -> String {
        self.path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
    fn ready(&self) -> Option<&Ready> {
        match &self.state {
            State::Ready(r) => Some(r),
            _ => None,
        }
    }
}

enum Msg {
    Loaded(PathBuf, Result<Project, FlpError>),
    Saved(PathBuf, Result<Saved, SaveError>),
    SaveDone,
}

enum Dialog {
    None,
    Results,
    ConfirmReplace(usize),
    FlOpen,
}

pub struct App {
    settings: Settings,
    lang: Lang,
    pal: Palette,
    items: Vec<Item>,
    selected: Option<usize>,
    hover_group: Option<usize>,
    pick_group: Option<usize>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    saving: bool,
    dialog: Dialog,
    fl_running: Arc<AtomicBool>,
    logo: egui::TextureHandle,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();
        setup_fonts(&cc.egui_ctx);

        let fl_running = Arc::new(AtomicBool::new(system::fl_studio_running()));
        {
            let flag = fl_running.clone();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(2));
                    let now = system::fl_studio_running();
                    if flag.swap(now, Ordering::Relaxed) != now {
                        ctx.request_repaint();
                    }
                }
            });
        }

        let (tx, rx) = channel();
        let mut app = App {
            lang: settings.language,
            pal: if settings.dark { DARK } else { LIGHT },
            settings,
            items: Vec::new(),
            selected: None,
            hover_group: None,
            pick_group: None,
            tx,
            rx,
            saving: false,
            dialog: Dialog::None,
            fl_running,
            logo: logo_texture(&cc.egui_ctx),
        };
        apply_style(&cc.egui_ctx, &app.pal, app.settings.dark);

        // Progetti trascinati sull'icona del programma
        let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
        app.add_paths(&cc.egui_ctx, args);
        app
    }

    fn l(&self, en: &'static str, it: &'static str) -> &'static str {
        self.lang.t(en, it)
    }

    fn loading(&self) -> bool {
        self.items.iter().any(|i| matches!(i.state, State::Loading))
    }

    fn busy(&self) -> bool {
        self.saving || self.loading()
    }

    /// Aggiunge file .flp (anche dentro cartelle trascinate, un livello).
    fn add_paths(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        let mut files = Vec::new();
        for p in paths {
            if p.is_dir() {
                if let Ok(rd) = std::fs::read_dir(&p) {
                    let mut v: Vec<PathBuf> = rd
                        .flatten()
                        .map(|e| e.path())
                        .filter(|x| is_flp(x))
                        .collect();
                    v.sort();
                    files.extend(v);
                }
            } else if is_flp(&p) {
                files.push(p);
            }
        }
        for p in files {
            if let Some(i) = self.items.iter().position(|it| it.path == p) {
                self.selected = Some(i);
                self.reload(ctx, i);
                continue;
            }
            self.items.push(Item {
                path: p.clone(),
                state: State::Loading,
                outcome: None,
            });
            self.selected = Some(self.items.len() - 1);
            self.spawn_load(ctx, p);
        }
    }

    fn spawn_load(&self, ctx: &egui::Context, p: PathBuf) {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = project::load(&p);
            let _ = tx.send(Msg::Loaded(p, r));
            ctx.request_repaint();
        });
    }

    fn reload(&mut self, ctx: &egui::Context, i: usize) {
        let it = &mut self.items[i];
        it.state = State::Loading;
        it.outcome = None;
        let p = it.path.clone();
        self.spawn_load(ctx, p);
    }

    fn replan(&mut self) {
        let o = self.settings.options.clone();
        for it in &mut self.items {
            if let State::Ready(r) = &mut it.state {
                r.plan = organize::plan(&r.project, &o);
                it.outcome = None;
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Loaded(p, r) => {
                    let o = self.settings.options.clone();
                    if let Some(it) = self.items.iter_mut().find(|i| i.path == p) {
                        it.state = match r {
                            Ok(project) => {
                                let issues = check::check(&project);
                                let plan = organize::plan(&project, &o);
                                State::Ready(Box::new(Ready {
                                    project: Arc::new(project),
                                    issues,
                                    plan,
                                }))
                            }
                            Err(e) => State::Failed(e),
                        };
                    }
                }
                Msg::Saved(p, r) => {
                    let lang = self.lang;
                    let mut reload = None;
                    if let Some((i, it)) =
                        self.items.iter_mut().enumerate().find(|(_, i)| i.path == p)
                    {
                        it.outcome = Some(match r {
                            Ok(s) => Outcome::Saved(s),
                            Err(e) => {
                                if matches!(e, SaveError::ChangedOnDisk) {
                                    reload = Some(i);
                                }
                                Outcome::Failed(save_error_text(lang, &e))
                            }
                        });
                    }
                    if let Some(i) = reload {
                        // ricarica ma tiene il messaggio di errore visibile
                        let msg = self.items[i].outcome.take();
                        self.reload(ctx, i);
                        self.items[i].outcome = msg;
                    }
                }
                Msg::SaveDone => {
                    self.saving = false;
                    self.dialog = Dialog::Results;
                }
            }
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !dropped.is_empty() && !self.saving {
            self.add_paths(ctx, dropped);
        }
    }

    fn browse(&mut self, ctx: &egui::Context) {
        let mut dlg = rfd::FileDialog::new()
            .set_title(self.l(
                "Select FL Studio projects",
                "Seleziona i progetti di FL Studio",
            ))
            .add_filter("FL Studio project", &["flp"]);
        if let Some(d) = self.settings.last_dir.as_ref().filter(|d| d.is_dir()) {
            dlg = dlg.set_directory(d);
        }
        if let Some(files) = dlg.pick_files() {
            self.settings.last_dir = files
                .first()
                .and_then(|f| f.parent())
                .map(Path::to_path_buf);
            self.add_paths(ctx, files);
        }
    }

    /// Progetti pronti da salvare (indici).
    fn savable(&self) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| {
                self.items[i]
                    .ready()
                    .is_some_and(|r| !r.project.clips.is_empty())
                    && !matches!(self.items[i].outcome, Some(Outcome::Saved(_)))
            })
            .collect()
    }

    fn try_save(&mut self, ctx: &egui::Context) {
        let todo = self.savable();
        if todo.is_empty() {
            return;
        }
        if self.settings.output == OutputMode::Folder
            && !self
                .settings
                .out_folder
                .as_ref()
                .is_some_and(|f| f.is_dir())
        {
            self.pick_out_folder();
            if !self
                .settings
                .out_folder
                .as_ref()
                .is_some_and(|f| f.is_dir())
            {
                return;
            }
        }
        if self.settings.output == OutputMode::Replace {
            if system::fl_studio_running() {
                self.dialog = Dialog::FlOpen;
            } else {
                self.dialog = Dialog::ConfirmReplace(todo.len());
            }
            return;
        }
        self.start_save(ctx, todo);
    }

    fn start_save(&mut self, ctx: &egui::Context, todo: Vec<usize>) {
        let target = || match self.settings.output {
            OutputMode::NextToOriginal => Target::NewFile(None),
            OutputMode::Folder => Target::NewFile(self.settings.out_folder.clone()),
            OutputMode::Replace => Target::ReplaceOriginal,
        };
        let jobs: Vec<(PathBuf, Arc<Project>, Plan, Target)> = todo
            .iter()
            .filter_map(|&i| {
                let it = &self.items[i];
                it.ready()
                    .map(|r| (it.path.clone(), r.project.clone(), r.plan.clone(), target()))
            })
            .collect();
        self.saving = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for (path, project, plan, target) in jobs {
                let r = write::save(&path, &project, &plan, &target);
                let _ = tx.send(Msg::Saved(path, r));
                ctx.request_repaint();
            }
            let _ = tx.send(Msg::SaveDone);
            ctx.request_repaint();
        });
    }

    fn pick_out_folder(&mut self) {
        let mut dlg = rfd::FileDialog::new().set_title(self.l(
            "Save organized projects in…",
            "Salva i progetti organizzati in…",
        ));
        if let Some(d) = self
            .settings
            .out_folder
            .as_ref()
            .or(self.settings.last_dir.as_ref())
            .filter(|d| d.is_dir())
        {
            dlg = dlg.set_directory(d);
        }
        if let Some(f) = dlg.pick_folder() {
            self.settings.out_folder = Some(f);
            self.settings.output = OutputMode::Folder;
        }
    }
}

fn is_flp(p: &Path) -> bool {
    p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("flp"))
}

// --------------------------------------------------------------------------
// Stile
// --------------------------------------------------------------------------

fn setup_fonts(ctx: &egui::Context) {
    // Inter (licenza OFL), incluso nell'eseguibile: stesso aspetto su Windows e macOS.
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "inter_semibold".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    // Cinese dell'interfaccia: Noto Sans SC (OFL) ridotto ai soli caratteri usati in i18n.rs.
    // Se cambiano le traduzioni cinesi va rigenerato (vedi assets/fonts/README.md).
    fonts.font_data.insert(
        "cjk_ui".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSansSC-UI.otf"
        ))),
    );
    let mut cjk = vec!["cjk_ui".to_string()];
    // Font CJK completo del sistema, se c'e: serve per i nomi delle clip in cinese/giapponese/coreano.
    if let Some(data) = system_cjk_font() {
        fonts.font_data.insert(
            "cjk_system".into(),
            Arc::new(egui::FontData::from_owned(data)),
        );
        cjk.push("cjk_system".into());
    }

    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".into());
    let mut bold_family = vec!["inter_semibold".to_string()];
    bold_family.extend(fallback);
    fonts
        .families
        .insert(FontFamily::Name("bold".into()), bold_family);
    for family in [
        FontFamily::Proportional,
        FontFamily::Monospace,
        FontFamily::Name("bold".into()),
    ] {
        fonts
            .families
            .entry(family)
            .or_default()
            .extend(cjk.iter().cloned());
    }
    ctx.set_fonts(fonts);
}

fn system_cjk_font() -> Option<Vec<u8>> {
    let candidates: &[&str] = if cfg!(windows) {
        &[
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\msyh.ttf",
            r"C:\Windows\Fonts\simsun.ttc",
        ]
    } else if cfg!(target_os = "macos") {
        &[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/Library/Fonts/Arial Unicode.ttf",
        ]
    } else {
        &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        ]
    };
    candidates.iter().find_map(|p| std::fs::read(p).ok())
}

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

fn apply_style(ctx: &egui::Context, p: &Palette, dark: bool) {
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = p.panel;
    v.window_fill = p.card;
    v.extreme_bg_color = if dark {
        Color32::from_rgb(0x12, 0x14, 0x18)
    } else {
        Color32::from_rgb(0xf0, 0xf2, 0xf6)
    };
    v.faint_bg_color = if dark {
        Color32::from_rgb(0x23, 0x27, 0x30)
    } else {
        Color32::from_rgb(0xf6, 0xf7, 0xfa)
    };
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.override_text_color = Some(p.text);
    v.selection.bg_fill = tint(p.accent, if dark { 60 } else { 50 });
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.hyperlink_color = p.accent;
    let r = CornerRadius::same(8);
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.inactive.bg_fill = p.card_hi;
    v.widgets.inactive.weak_bg_fill = p.card_hi;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.hovered.bg_fill = tint(p.accent, 40);
    v.widgets.hovered.weak_bg_fill = if dark {
        Color32::from_rgb(0x34, 0x3a, 0x46)
    } else {
        Color32::from_rgb(0xe4, 0xe7, 0xee)
    };
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, tint(p.accent, 160));
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, p.text);
    v.widgets.active.weak_bg_fill = tint(p.accent, 70);
    v.widgets.active.bg_fill = tint(p.accent, 90);
    v.widgets.active.fg_stroke = Stroke::new(1.5, p.text);
    v.striped = true;

    // Il tema lo decido io, non quello del sistema: altrimenti i colori si mescolano.
    let theme = if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(theme);
    ctx.set_visuals_of(theme, v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 7.0);
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.scroll.bar_width = 8.0;
        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        s.text_styles
            .insert(TextStyle::Button, FontId::proportional(14.0));
        s.text_styles
            .insert(TextStyle::Small, FontId::proportional(12.0));
        s.text_styles.insert(TextStyle::Heading, bold(19.0));
        s.text_styles
            .insert(TextStyle::Monospace, FontId::monospace(13.0));
    });
}

// --------------------------------------------------------------------------
// Pezzi di interfaccia riutilizzabili
// --------------------------------------------------------------------------

fn card(p: &Palette) -> Frame {
    Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(14))
}

fn chip(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    Frame::new()
        .fill(tint(color, 38))
        .stroke(Stroke::new(1.0, tint(color, 110)))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(9, 2))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(text).color(color).font(bold(12.0)))
                    .wrap_mode(egui::TextWrapMode::Extend),
            )
        })
        .response
}

fn step_title(ui: &mut egui::Ui, p: &Palette, n: &str, title: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 12.0, p.accent);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            n,
            bold(13.0),
            p.on_accent,
        );
        ui.label(RichText::new(title).font(bold(15.5)));
    });
}

fn hint(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.label(RichText::new(text).color(p.weak).size(12.5));
}

fn accent_button(p: &Palette, text: &str, enabled_look: bool) -> egui::Button<'static> {
    let fill = if enabled_look { p.accent } else { p.card_hi };
    let color = if enabled_look { p.on_accent } else { p.weak };
    egui::Button::new(
        RichText::new(text.to_string())
            .font(bold(14.5))
            .color(color),
    )
    .fill(fill)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(10))
    .min_size(Vec2::new(0.0, 38.0))
}

/// Due o piu pulsanti uniti, uno solo attivo.
fn segmented<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    p: &Palette,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;
    Frame::new()
        .fill(p.card_hi)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let w =
                (ui.available_width() - 2.0 * (options.len() as f32 - 1.0)) / options.len() as f32;
            ui.horizontal(|ui| {
                for (v, label) in options {
                    let sel = *value == *v;
                    let txt = RichText::new(*label).font(bold(13.0)).color(if sel {
                        p.on_accent
                    } else {
                        p.text
                    });
                    let b = egui::Button::new(txt)
                        .fill(if sel { p.accent } else { Color32::TRANSPARENT })
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(8))
                        .min_size(Vec2::new(w.max(60.0), 30.0));
                    if ui.add(b).clicked() && !sel {
                        *value = *v;
                        changed = true;
                    }
                }
            });
        });
    changed
}

fn stat_tile(ui: &mut egui::Ui, p: &Palette, value: &str, label: &str, color: Color32, width: f32) {
    Frame::new()
        .fill(tint(color, 22))
        .stroke(Stroke::new(1.0, tint(color, 70)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_width((width - 22.0).max(40.0));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add(
                    egui::Label::new(RichText::new(value).font(bold(21.0)).color(color)).truncate(),
                );
                ui.add(egui::Label::new(RichText::new(label).color(p.weak).size(11.5)).truncate());
            });
        });
}

fn fmt_num(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn track_range(first: usize, lanes: usize) -> String {
    if lanes <= 1 {
        format!("{}", first + 1)
    } else {
        format!("{}–{}", first + 1, first + lanes)
    }
}

/// Il logo (la stessa immagine dell'icona) come texture.
fn logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let icon = app_icon();
    let img = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    ctx.load_texture("logo", img, egui::TextureOptions::LINEAR)
}

fn app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default()
}

pub fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("{APP_NAME} - {APP_SUBTITLE}"))
            .with_app_id(crate::engine::APP_ID)
            .with_inner_size([1320.0, 840.0])
            .with_min_inner_size([900.0, 580.0])
            .with_resizable(true)
            .with_drag_and_drop(true)
            .with_icon(app_icon()),
        ..Default::default()
    }
}

// --------------------------------------------------------------------------
// Disegno
// --------------------------------------------------------------------------

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.handle_drops(&ctx);
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }

        let p = self.pal;
        egui::Panel::top("top")
            .frame(
                Frame::new()
                    .fill(p.bg)
                    .inner_margin(Margin::symmetric(18, 12)),
            )
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("bottom")
            .frame(
                Frame::new()
                    .fill(p.bg)
                    .inner_margin(Margin::symmetric(18, 12))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| self.save_bar(ui, &ctx));
        // Barra laterale: si allarga o si stringe trascinando il bordo destro
        egui::Panel::left("left")
            .resizable(true)
            .default_size(360.0)
            .min_size(300.0)
            .max_size(560.0)
            .frame(Frame::new().fill(p.bg).inner_margin(Margin {
                left: 18,
                right: 10,
                top: 4,
                bottom: 12,
            }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.left_panel(ui, &ctx));
            });
        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg).inner_margin(Margin {
                left: 8,
                right: 18,
                top: 4,
                bottom: 12,
            }))
            .show(ui, |ui| self.central(ui, &ctx));

        self.dialogs(&ctx);
        self.drop_overlay(&ctx);
    }
}

impl App {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        ui.horizontal(|ui| {
            ui.add(egui::Image::new(&self.logo).fit_to_exact_size(Vec2::splat(44.0)));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(APP_NAME).font(bold(20.0)));
                    ui.label(RichText::new(APP_SUBTITLE).color(p.weak).size(14.0));
                    chip(ui, &format!("v{APP_VERSION}"), p.weak);
                });
                ui.label(
                    RichText::new(self.l(
                        "Group every playlist clip by name onto neighbouring tracks, without moving anything in time",
                        "Raggruppa ogni clip della playlist per nome su tracce vicine, senza spostare niente nel tempo",
                    ))
                    .color(p.weak)
                    .size(12.5),
                );
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let icon = if self.settings.dark { "☀" } else { "🌙" };
                if ui
                    .add(egui::Button::new(RichText::new(icon).size(16.0)).min_size(Vec2::splat(34.0)))
                    .on_hover_text(self.l("Light / dark theme", "Tema chiaro / scuro"))
                    .clicked()
                {
                    self.settings.dark = !self.settings.dark;
                    self.pal = if self.settings.dark { DARK } else { LIGHT };
                    apply_style(ui.ctx(), &self.pal, self.settings.dark);
                }
                let label = RichText::new(format!("🌐  {}  ⏷", self.lang.code())).font(bold(12.5));
                ui.menu_button(label, |ui| {
                    for l in Lang::ALL {
                        if ui.selectable_label(self.lang == l, RichText::new(l.name()).size(14.0)).clicked() {
                            self.lang = l;
                            self.settings.language = l;
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text(self.l("Language", "Lingua"));
                ui.add_space(6.0);
                if self.settings.output == OutputMode::Replace {
                    if self.fl_running.load(Ordering::Relaxed) {
                        chip(ui, self.l("FL Studio is running. Close it before replacing projects", "FL Studio è aperto. Chiudilo prima di sostituire i progetti"), p.red);
                    } else {
                        chip(ui, self.l("FL Studio is closed", "FL Studio è chiuso"), p.green);
                    }
                }
            });
        });
    }

    fn left_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.pal;
        let lang = self.lang;
        let w = ui.available_width();
        let saving = self.saving;

        // ---------------- 1. Progetti ----------------
        card(&p).show(ui, |ui| {
            ui.set_width(w - 30.0);
            step_title(ui, &p, "1", lang.t("Projects", "Progetti"));
            hint(
                ui,
                &p,
                lang.t(
                    "Drop .flp files or folders here. You can organize many projects at once.",
                    "Trascina qui file .flp o cartelle. Puoi organizzare tanti progetti in una volta.",
                ),
            );
            let mut remove = None;
            let mut pick = None;
            Frame::new().fill(p.card_hi).corner_radius(CornerRadius::same(8)).inner_margin(Margin::same(6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if self.items.is_empty() {
                    ui.label(RichText::new(lang.t("No projects yet.", "Nessun progetto.")).color(p.weak).size(12.5));
                }
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for (i, it) in self.items.iter().enumerate() {
                        let sel = self.selected == Some(i);
                        let (txt, col) = item_status(lang, &p, it);
                        let fr = Frame::new()
                            .fill(if sel { tint(p.accent, 34) } else { Color32::TRANSPARENT })
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::symmetric(6, 3));
                        let resp = fr
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(Vec2::new(4.0, 18.0), Sense::hover());
                                    ui.painter().rect_filled(rect, CornerRadius::same(2), col);
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .add_enabled(!saving, egui::Button::new("✖").small().frame(false))
                                            .on_hover_text(lang.t("Remove from the list", "Togli dall'elenco"))
                                            .clicked()
                                        {
                                            remove = Some(i);
                                        }
                                        ui.label(RichText::new(txt).color(col).size(11.5));
                                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                            ui.add(egui::Label::new(RichText::new(it.name()).size(13.0)).truncate().selectable(false))
                                                .on_hover_text(it.path.to_string_lossy());
                                        });
                                    });
                                });
                            })
                            .response
                            .interact(Sense::click());
                        if resp.clicked() {
                            pick = Some(i);
                        }
                    }
                });
            });
            if let Some(i) = pick {
                self.selected = Some(i);
                self.pick_group = None;
            }
            if let Some(i) = remove {
                self.items.remove(i);
                self.selected = if self.items.is_empty() { None } else { Some(i.min(self.items.len() - 1)) };
                self.pick_group = None;
            }
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!saving, |ui| {
                    if ui.button(lang.t("➕  Add projects…", "➕  Aggiungi progetti…")).clicked() {
                        self.browse(ctx);
                    }
                    if !self.items.is_empty() && ui.button(lang.t("Clear", "Svuota")).clicked() {
                        self.items.clear();
                        self.selected = None;
                        self.pick_group = None;
                    }
                });
            });
        });

        ui.add_space(10.0);

        // ---------------- 2. Ordine ----------------
        let mut changed = false;
        card(&p).show(ui, |ui| {
            ui.set_width(w - 30.0);
            step_title(ui, &p, "2", lang.t("Track order", "Ordine delle tracce"));
            ui.add_enabled_ui(!saving, |ui| {
                changed |= segmented(
                    ui,
                    &p,
                    &mut self.settings.options.sort,
                    &[
                        (SortMode::Alphabetical, lang.t("A – Z", "A – Z")),
                        (SortMode::FirstAppearance, lang.t("Song order", "Ordine del brano")),
                    ],
                );
                hint(
                    ui,
                    &p,
                    match self.settings.options.sort {
                        SortMode::Alphabetical => lang.t(
                            "Groups sorted by name: easy to find a sample or pattern.",
                            "Gruppi in ordine di nome: trovi subito un sample o un pattern.",
                        ),
                        SortMode::FirstAppearance => lang.t(
                            "Groups sorted by when they first play: the playlist reads top to bottom like the song.",
                            "Gruppi in ordine di quando suonano la prima volta: la playlist si legge dall'alto in basso come il brano.",
                        ),
                    },
                );
                ui.add_space(2.0);
                ui.label(RichText::new(lang.t("Then group by", "Poi raggruppa per")).font(bold(13.0)));
                for (s, label, tip) in [
                    (
                        SubSort::Type,
                        lang.t("Type (audio, patterns, automation)", "Tipo (audio, pattern, automazioni)"),
                        lang.t("Audio clips first, then patterns, then automation clips", "Prima le clip audio, poi i pattern, poi le automazioni"),
                    ),
                    (
                        SubSort::Length,
                        lang.t("Clip length (longest first)", "Lunghezza delle clip (più lunghe prima)"),
                        lang.t("Groups with longer clips go higher", "I gruppi con clip più lunghe vanno più in alto"),
                    ),
                    (
                        SubSort::Color,
                        lang.t("Color (rainbow order)", "Colore (ordine arcobaleno)"),
                        lang.t("Red, orange, yellow, green, blue, purple. Grey clips go last", "Rosso, arancio, giallo, verde, blu, viola. Le clip grigie vanno in fondo"),
                    ),
                ] {
                    let mut on = self.settings.options.then_by.contains(&s);
                    if ui.checkbox(&mut on, label).on_hover_text(tip).changed() {
                        self.settings.options.then_by.retain(|x| *x != s);
                        if on {
                            self.settings.options.then_by.push(s);
                        }
                        changed = true;
                    }
                }
            });
        });

        ui.add_space(10.0);

        // ---------------- 3. Extra ----------------
        card(&p).show(ui, |ui| {
            ui.set_width(w - 30.0);
            step_title(ui, &p, "3", lang.t("Finishing touches", "Ritocchi finali"));
            ui.add_enabled_ui(!saving, |ui| {
                changed |= ui
                    .checkbox(&mut self.settings.options.rename_tracks, lang.t("Name tracks after their clips", "Dai alle tracce il nome delle clip"))
                    .on_hover_text(lang.t("Tracks you already named keep their name", "Le tracce a cui hai già dato un nome lo mantengono"))
                    .changed();
                changed |= ui
                    .checkbox(&mut self.settings.options.color_clips, lang.t("Color the groups like a rainbow", "Colora i gruppi come un arcobaleno"))
                    .on_hover_text(lang.t(
                        "Each group gets its own color, in order. Colors you picked yourself are kept",
                        "Ogni gruppo riceve il suo colore, in ordine. I colori scelti da te vengono mantenuti",
                    ))
                    .changed();
                hint(
                    ui,
                    &p,
                    lang.t(
                        "Always kept: positions, lengths, muted clips and muted tracks (they move to the bottom, still muted).",
                        "Sempre mantenuti: posizioni, lunghezze, clip mute e tracce mute (vanno in fondo, sempre mute).",
                    ),
                );
            });
        });

        ui.add_space(10.0);

        // ---------------- 4. Salvataggio ----------------
        card(&p).show(ui, |ui| {
            ui.set_width(w - 30.0);
            step_title(ui, &p, "4", lang.t("Save to", "Dove salvare"));
            ui.add_enabled_ui(!saving, |ui| {
                ui.radio_value(
                    &mut self.settings.output,
                    OutputMode::NextToOriginal,
                    lang.t("New file next to the original", "Nuovo file accanto all'originale"),
                );
                hint(ui, &p, lang.t("Saved as \"Project_organized.flp\". Your project stays as it is.", "Salvato come \"Progetto_organized.flp\". Il tuo progetto resta com'è."));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.settings.output, OutputMode::Folder, lang.t("New file in a folder", "Nuovo file in una cartella"));
                    if ui.small_button(lang.t("Choose…", "Scegli…")).clicked() {
                        self.pick_out_folder();
                    }
                });
                if self.settings.output == OutputMode::Folder {
                    let shown = self
                        .settings
                        .out_folder
                        .as_ref()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_else(|| lang.t("No folder selected", "Nessuna cartella selezionata").into());
                    Frame::new().fill(p.card_hi).corner_radius(CornerRadius::same(8)).inner_margin(Margin::symmetric(10, 5)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(RichText::new(&shown).size(12.0).color(p.weak)).truncate()).on_hover_text(&shown);
                    });
                }
                ui.radio_value(
                    &mut self.settings.output,
                    OutputMode::Replace,
                    lang.t("Replace the original (a backup copy is kept)", "Sostituisci l'originale (viene tenuta una copia di sicurezza)"),
                );
                if self.settings.output == OutputMode::Replace {
                    hint(
                        ui,
                        &p,
                        lang.t(
                            "The backup is saved next to the project as \"Project.flp.backup_DATE\". Close the project in FL Studio first.",
                            "La copia di sicurezza viene salvata accanto al progetto come \"Progetto.flp.backup_DATA\". Prima chiudi il progetto in FL Studio.",
                        ),
                    );
                }
            });
        });

        if changed {
            self.replan();
        }
    }

    fn central(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.pal;
        let lang = self.lang;
        let Some(sel) = self.selected.filter(|&s| s < self.items.len()) else {
            self.empty_state(ui, ctx);
            return;
        };
        let it = &self.items[sel];
        let r = match &it.state {
            State::Loading => {
                card(&p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(ui.available_height());
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * 0.38);
                        ui.spinner();
                        ui.label(lang.t("Reading the project…", "Lettura del progetto…"));
                    });
                });
                return;
            }
            State::Failed(e) => {
                let msg = error_text(lang, e);
                card(&p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(ui.available_height());
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * 0.32);
                        ui.label(RichText::new("⚠").size(44.0).color(p.red));
                        ui.label(RichText::new(it.name()).font(bold(17.0)));
                        ui.label(RichText::new(msg).color(p.weak));
                    });
                });
                return;
            }
            State::Ready(r) => r,
        };
        let project = r.project.clone();
        let plan = &r.plan;
        let issues = r.issues.clone();

        // ---- intestazione ----
        card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(RichText::new(it.name()).font(bold(17.0))).truncate());
                let lvl = check::level(&issues);
                let ver = if project.version.is_empty() {
                    "?".to_string()
                } else {
                    project.version.clone()
                };
                match lvl {
                    Level::Warning => chip(
                        ui,
                        &lang.f(
                            "⚠  Check needed · FL Studio {0}",
                            "⚠  Da controllare · FL Studio {0}",
                            &[&ver],
                        ),
                        p.yellow,
                    ),
                    _ => chip(
                        ui,
                        &lang.f(
                            "✔  Compatible · FL Studio {0}",
                            "✔  Compatibile · FL Studio {0}",
                            &[&ver],
                        ),
                        p.green,
                    ),
                };
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .button(lang.t("📂  Show in folder", "📂  Mostra nella cartella"))
                        .clicked()
                    {
                        system::reveal(&it.path.to_string_lossy());
                    }
                });
            });
            ui.add(
                egui::Label::new(
                    RichText::new(it.path.to_string_lossy())
                        .size(12.0)
                        .color(p.weak),
                )
                .truncate(),
            );
            ui.add_space(4.0);
            let tw = (ui.available_width() - 24.0) / 4.0;
            let before = project.tracks_used();
            ui.horizontal(|ui| {
                stat_tile(
                    ui,
                    &p,
                    &fmt_num(project.clips.len()),
                    lang.t("Clips", "Clip"),
                    p.text,
                    tw,
                );
                stat_tile(
                    ui,
                    &p,
                    &fmt_num(plan.groups.len()),
                    lang.t("Groups", "Gruppi"),
                    p.accent,
                    tw,
                );
                stat_tile(
                    ui,
                    &p,
                    &format!("{} ➡ {}", before, plan.tracks_needed),
                    lang.t("Tracks used: before ➡ after", "Tracce usate: prima ➡ dopo"),
                    p.green,
                    tw,
                );
                stat_tile(
                    ui,
                    &p,
                    &fmt_num(plan.moved),
                    lang.t("Clips that change track", "Clip che cambiano traccia"),
                    p.blue,
                    tw,
                );
            });
            let notes: Vec<(String, Color32)> = issues
                .iter()
                .map(|i| {
                    (
                        issue_text(lang, i),
                        if i.level() == Level::Warning {
                            p.yellow
                        } else {
                            p.weak
                        },
                    )
                })
                .chain(
                    plan.warnings
                        .iter()
                        .map(|w| (warning_text(lang, w), p.yellow)),
                )
                .collect();
            for (n, c) in notes {
                ui.label(RichText::new(format!("•  {n}")).color(c).size(12.5));
            }
            match &it.outcome {
                Some(Outcome::Saved(s)) => {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(lang.f(
                                "✔  Saved: {0}",
                                "✔  Salvato: {0}",
                                &[&s.path.display()],
                            ))
                            .color(p.green)
                            .size(12.5),
                        );
                        if ui.small_button(lang.t("Show", "Mostra")).clicked() {
                            system::reveal(&s.path.to_string_lossy());
                        }
                    });
                }
                Some(Outcome::Failed(e)) => {
                    ui.label(RichText::new(format!("✖  {e}")).color(p.red).size(12.5));
                }
                None => {}
            }
        });
        ui.add_space(8.0);

        // ---- elenco dei gruppi: si ridimensiona trascinando il bordo superiore ----
        let max = (ui.available_height() - 180.0).max(140.0);
        let mut hovered_row = None;
        egui::Panel::bottom("groups")
            .resizable(true)
            .default_size(210.0)
            .min_size(110.0)
            .max_size(max)
            .frame(Frame::new().inner_margin(Margin {
                left: 0,
                right: 0,
                top: 8,
                bottom: 0,
            }))
            .show(ui, |ui| {
                let (h, c) = groups_table(ui, &p, lang, plan, self.pick_group);
                hovered_row = h;
                if let Some(i) = c {
                    self.pick_group = if self.pick_group == Some(i) {
                        None
                    } else {
                        Some(i)
                    };
                }
            });
        let hovered_row = hovered_row.or(self.hover_group);

        // ---- anteprima della playlist ----
        card(&p).inner_margin(Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(lang.t("Playlist preview", "Anteprima della playlist"))
                        .font(bold(14.5)),
                );
                ui.add_space(8.0);
                ui.allocate_ui(Vec2::new(230.0, 36.0), |ui| {
                    segmented(
                        ui,
                        &p,
                        &mut self.settings.show_after,
                        &[
                            (false, lang.t("Before", "Prima")),
                            (true, lang.t("After", "Dopo")),
                        ],
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    hint(
                        ui,
                        &p,
                        lang.t(
                            "Point at a clip to see its name",
                            "Passa sopra una clip per vederne il nome",
                        ),
                    );
                });
            });
            let focus = hovered_row.or(self.pick_group);
            let hov = playlist_view(
                ui,
                &p,
                lang,
                &project,
                plan,
                self.settings.show_after,
                focus,
            );
            self.hover_group = hov;
        });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.pal;
        let lang = self.lang;
        let resp = card(&p)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height());
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.26);
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(150.0, 86.0), Sense::hover());
                    // tre clip sparse che si mettono in fila
                    let cols = [p.accent, p.yellow, p.green];
                    for (i, c) in cols.iter().enumerate() {
                        let y = rect.top() + 8.0 + i as f32 * 26.0;
                        let x = rect.left() + 10.0 + i as f32 * 16.0;
                        ui.painter().rect_filled(
                            Rect::from_min_size(Pos2::new(x, y), Vec2::new(rect.width() - 20.0 - i as f32 * 16.0, 18.0)),
                            CornerRadius::same(4),
                            tint(*c, 200),
                        );
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new(lang.t("Drop your FL Studio projects here", "Trascina qui i tuoi progetti di FL Studio")).font(bold(18.0)));
                    hint(
                        ui,
                        &p,
                        lang.t(
                            "Or click \"Add projects…\" on the left. Works with projects from FL Studio 2025 and newer.",
                            "Oppure premi \"Aggiungi progetti…\" a sinistra. Funziona con i progetti di FL Studio 2025 e successivi.",
                        ),
                    );
                    ui.add_space(6.0);
                    hint(
                        ui,
                        &p,
                        lang.t(
                            "Save your project in FL Studio first. Your original file is never changed unless you choose to replace it.",
                            "Prima salva il progetto in FL Studio. Il file originale non viene mai modificato, a meno che tu non scelga di sostituirlo.",
                        ),
                    );
                    ui.add_space(10.0);
                    if ui.add(accent_button(&p, lang.t("➕  Add projects…", "➕  Aggiungi progetti…"), true).min_size(Vec2::new(220.0, 42.0))).clicked() {
                        self.browse(ctx);
                    }
                });
            })
            .response;
        let _ = resp;
    }
}

/// Tabella dei gruppi. Restituisce il gruppo sotto il mouse e quello cliccato.
fn groups_table(
    ui: &mut egui::Ui,
    p: &Palette,
    lang: Lang,
    plan: &Plan,
    pick: Option<usize>,
) -> (Option<usize>, Option<usize>) {
    let p = *p;
    let mut hovered = None;
    let mut clicked = None;
    let n = plan.groups.len();
    card(&p).inner_margin(Margin::same(6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(ui.available_height());
        if plan.groups.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.label(
                    RichText::new(lang.t("No clips to organize.", "Nessuna clip da organizzare."))
                        .color(p.weak),
                );
            });
            return;
        }
        let h = ui.available_height();
        TableBuilder::new(ui)
            .id_salt("gruppi")
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .auto_shrink([false, false])
            .max_scroll_height(h - 34.0)
            .column(Column::exact(14.0).resizable(false))
            .column(Column::initial(90.0).at_least(70.0))
            .column(Column::initial(320.0).at_least(140.0).clip(true))
            .column(Column::initial(110.0).at_least(80.0))
            .column(Column::initial(70.0).at_least(50.0))
            .column(Column::remainder().at_least(80.0).clip(true))
            .header(26.0, |mut hd| {
                hd.col(|_| {});
                for t in [
                    lang.t("Tracks", "Tracce"),
                    lang.t("Group", "Gruppo"),
                    lang.t("Type", "Tipo"),
                    lang.t("Clips", "Clip"),
                    lang.t("Notes", "Note"),
                ] {
                    hd.col(|ui| {
                        ui.label(RichText::new(t).color(p.weak).font(bold(12.0)));
                    });
                }
            })
            .body(|body| {
                body.rows(28.0, n, |mut row| {
                    let i = row.index();
                    let g = &plan.groups[i];
                    row.set_selected(pick == Some(i));
                    row.col(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(10.0, 16.0), Sense::hover());
                        ui.painter()
                            .rect_filled(rect, CornerRadius::same(3), group_color(g, i, n));
                    });
                    row.col(|ui| {
                        ui.label(
                            RichText::new(track_range(g.first_track, g.lanes)).font(bold(13.0)),
                        );
                    });
                    row.col(|ui| {
                        let t = RichText::new(&g.name);
                        ui.add(
                            egui::Label::new(if g.muted { t.color(p.weak) } else { t })
                                .truncate()
                                .selectable(false),
                        )
                        .on_hover_text(&g.name);
                    });
                    row.col(|ui| {
                        ui.label(
                            RichText::new(kind_label(lang, g.kind))
                                .color(p.weak)
                                .size(12.5),
                        );
                    });
                    row.col(|ui| {
                        ui.label(fmt_num(g.clips));
                    });
                    row.col(|ui| {
                        if g.muted {
                            chip(ui, lang.t("Muted track", "Traccia muta"), p.gray);
                        } else if g.lanes > 1 {
                            ui.label(
                                RichText::new(lang.f(
                                    "{0} lanes (overlapping clips)",
                                    "{0} corsie (clip sovrapposte)",
                                    &[&g.lanes],
                                ))
                                .color(p.weak)
                                .size(12.0),
                            );
                        }
                    });
                    if row.response().hovered() {
                        hovered = Some(i);
                    }
                    if row.response().clicked() {
                        clicked = Some(i);
                    }
                });
            });
    });
    (hovered, clicked)
}

impl App {
    fn save_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.pal;
        let lang = self.lang;
        let todo = self.savable();
        let clips: usize = todo
            .iter()
            .filter_map(|&i| self.items[i].ready())
            .map(|r| r.plan.moved)
            .sum();
        ui.horizontal(|ui| {
            let (label, sub, col) = match self.settings.output {
                OutputMode::Replace => (
                    lang.t("Replace originals", "Sostituisci gli originali"),
                    lang.t(
                        "A backup copy of each project is saved first",
                        "Prima viene salvata una copia di sicurezza di ogni progetto",
                    ),
                    p.red,
                ),
                _ => (
                    lang.t("Safe mode", "Modalità sicura"),
                    lang.t(
                        "A new file is created. Your projects are not changed",
                        "Viene creato un nuovo file. I tuoi progetti non vengono modificati",
                    ),
                    p.blue,
                ),
            };
            let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 34.0), Sense::hover());
            ui.painter()
                .rect_filled(rect, CornerRadius::same(3), tint(col, 200));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new(label).font(bold(13.5)).color(col));
                ui.label(RichText::new(sub).size(12.0).color(p.weak));
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let can = !todo.is_empty() && !self.busy();
                let txt = if self.saving {
                    lang.t("Saving…", "Salvataggio…").to_string()
                } else if todo.len() > 1 {
                    lang.f(
                        "✔  Organize {0} projects",
                        "✔  Organizza {0} progetti",
                        &[&todo.len()],
                    )
                } else {
                    lang.t("✔  Organize and save", "✔  Organizza e salva")
                        .to_string()
                };
                if ui
                    .add_enabled(
                        can,
                        accent_button(&p, &txt, can).min_size(Vec2::new(230.0, 40.0)),
                    )
                    .clicked()
                {
                    self.try_save(ctx);
                }
                ui.add_space(12.0);
                if !self.items.is_empty() {
                    let msg = if todo.is_empty() {
                        if self.loading() {
                            RichText::new(lang.t("Reading projects…", "Lettura dei progetti…"))
                                .color(p.weak)
                        } else {
                            RichText::new(lang.t("Nothing left to save", "Nient'altro da salvare"))
                                .color(p.weak)
                        }
                    } else if todo.len() == 1 {
                        RichText::new(lang.f(
                            "1 project ready  ·  {0} clips change track",
                            "1 progetto pronto  ·  {0} clip cambiano traccia",
                            &[&fmt_num(clips)],
                        ))
                    } else {
                        RichText::new(lang.f(
                            "{0} projects ready  ·  {1} clips change track",
                            "{0} progetti pronti  ·  {1} clip cambiano traccia",
                            &[&todo.len(), &fmt_num(clips)],
                        ))
                    };
                    ui.add(egui::Label::new(msg.size(13.5)).truncate());
                }
            });
        });
    }

    fn drop_overlay(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }
        let p = self.pal;
        let rect = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("drop"),
        ));
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_black_alpha(170));
        painter.rect_stroke(
            rect.shrink(16.0),
            CornerRadius::same(16),
            Stroke::new(2.0, p.accent),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            self.l(
                "Drop FL Studio projects (.flp) or folders here",
                "Rilascia qui progetti di FL Studio (.flp) o cartelle",
            ),
            bold(21.0),
            Color32::WHITE,
        );
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let p = self.pal;
        let lang = self.lang;
        let dialog = std::mem::replace(&mut self.dialog, Dialog::None);
        let modal_frame = Frame::new()
            .fill(p.card)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::same(22));
        let mut keep = true;

        match &dialog {
            Dialog::None => keep = false,
            Dialog::Results => {
                let r = egui::Modal::new(egui::Id::unique("results")).frame(modal_frame).show(ctx, |ui| {
                    ui.set_width(620.0);
                    let saved: Vec<&Item> = self.items.iter().filter(|i| matches!(i.outcome, Some(Outcome::Saved(_)))).collect();
                    let failed: Vec<&Item> = self.items.iter().filter(|i| matches!(i.outcome, Some(Outcome::Failed(_)))).collect();
                    let (title, col) = if failed.is_empty() {
                        (lang.t("Done!", "Fatto!"), p.green)
                    } else {
                        (lang.t("Some projects were not saved", "Alcuni progetti non sono stati salvati"), p.red)
                    };
                    ui.label(RichText::new(title).font(bold(18.0)).color(col));
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for it in &saved {
                            if let Some(Outcome::Saved(s)) = &it.outcome {
                                Frame::new().fill(p.card_hi).corner_radius(CornerRadius::same(8)).inner_margin(Margin::symmetric(10, 6)).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("✔").color(p.green));
                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = 1.0;
                                            let name = s.path.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                                            ui.label(RichText::new(name).font(bold(13.5)));
                                            ui.add(egui::Label::new(RichText::new(s.path.to_string_lossy()).size(11.5).color(p.weak)).truncate());
                                            if let Some(b) = &s.backup {
                                                let bn = b.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                                                ui.label(RichText::new(lang.f("Backup: {0}", "Copia di sicurezza: {0}", &[&bn])).size(11.5).color(p.weak));
                                            }
                                        });
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if ui.button(lang.t("📂  Show", "📂  Mostra")).clicked() {
                                                system::reveal(&s.path.to_string_lossy());
                                            }
                                        });
                                    });
                                });
                            }
                        }
                        for it in &failed {
                            if let Some(Outcome::Failed(e)) = &it.outcome {
                                ui.label(RichText::new(format!("✖  {}: {e}", it.name())).color(p.red));
                            }
                        }
                    });
                    ui.add_space(8.0);
                    if !saved.is_empty() {
                        hint(
                            ui,
                            &p,
                            lang.t(
                                "Every file was checked after saving. Open it in FL Studio: the clips are grouped, nothing moved in time.",
                                "Ogni file è stato controllato dopo il salvataggio. Aprilo in FL Studio: le clip sono raggruppate e niente si è spostato nel tempo.",
                            ),
                        );
                    }
                    ui.add_space(10.0);
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| ui.add(accent_button(&p, lang.t("  Close  ", "  Chiudi  "), true)).clicked()).inner
                });
                if r.inner || r.should_close() {
                    keep = false;
                }
            }
            Dialog::ConfirmReplace(n) => {
                let r = egui::Modal::new(egui::Id::unique("replace")).frame(modal_frame).show(ctx, |ui| {
                    ui.set_width(480.0);
                    ui.label(RichText::new(lang.f("Replace {0} projects?", "Sostituire {0} progetti?", &[n])).font(bold(17.0)));
                    ui.add_space(6.0);
                    ui.label(lang.t(
                        "Each project is overwritten with its organized version.",
                        "Ogni progetto viene sovrascritto con la sua versione organizzata.",
                    ));
                    hint(
                        ui,
                        &p,
                        lang.t(
                            "A backup copy of every original is saved next to it first, so you can always go back.",
                            "Prima viene salvata una copia di sicurezza di ogni originale accanto a esso, così puoi sempre tornare indietro.",
                        ),
                    );
                    ui.add_space(10.0);
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        let yes = ui.add(accent_button(&p, lang.t("  Replace  ", "  Sostituisci  "), true)).clicked();
                        let no = ui.button(lang.t("Cancel", "Annulla")).clicked();
                        (yes, no)
                    })
                    .inner
                });
                let sc = r.should_close();
                let (yes, no) = r.inner;
                if yes {
                    self.dialog = Dialog::None;
                    let todo = self.savable();
                    self.start_save(ctx, todo);
                    return;
                }
                if no || sc {
                    keep = false;
                }
            }
            Dialog::FlOpen => {
                let r = egui::Modal::new(egui::Id::unique("flopen")).frame(modal_frame).show(ctx, |ui| {
                    ui.set_width(460.0);
                    ui.label(RichText::new(lang.t("FL Studio is running", "FL Studio è aperto")).font(bold(17.0)).color(p.red));
                    ui.add_space(4.0);
                    ui.label(lang.t(
                        "Close FL Studio before replacing projects: if a project is open, FL Studio would save over the organized version. Or choose \"New file next to the original\".",
                        "Chiudi FL Studio prima di sostituire i progetti: se un progetto è aperto, FL Studio salverebbe sopra la versione organizzata. Oppure scegli \"Nuovo file accanto all'originale\".",
                    ));
                    ui.add_space(10.0);
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| ui.add(accent_button(&p, "  OK  ", true)).clicked()).inner
                });
                if r.inner || r.should_close() {
                    keep = false;
                }
            }
        }
        if keep {
            self.dialog = dialog;
        }
    }
}

fn item_status(l: Lang, p: &Palette, it: &Item) -> (&'static str, Color32) {
    match (&it.state, &it.outcome) {
        (State::Loading, _) => (l.t("Reading…", "Lettura…"), p.gray),
        (State::Failed(_), _) => (l.t("Cannot open", "Non apribile"), p.red),
        (_, Some(Outcome::Saved(_))) => (l.t("Saved", "Salvato"), p.green),
        (_, Some(Outcome::Failed(_))) => (l.t("Not saved", "Non salvato"), p.red),
        (State::Ready(r), None) if r.project.clips.is_empty() => (l.t("Empty", "Vuoto"), p.gray),
        (State::Ready(r), None) if check::level(&r.issues) == Level::Warning => {
            (l.t("Check", "Controlla"), p.yellow)
        }
        (State::Ready(_), None) => (l.t("Ready", "Pronto"), p.blue),
    }
}

/// Disegna la playlist (prima o dopo). Restituisce il gruppo sotto il mouse.
fn playlist_view(
    ui: &mut egui::Ui,
    p: &Palette,
    lang: Lang,
    project: &Project,
    plan: &Plan,
    after: bool,
    focus: Option<usize>,
) -> Option<usize> {
    if project.clips.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(
                RichText::new(lang.t("The playlist is empty.", "La playlist è vuota."))
                    .color(p.weak),
            );
        });
        return None;
    }
    // Primo arrangement con clip (gli altri si vedono nella tabella)
    let arr = project.clips[0].arrangement;
    let clips: Vec<usize> = (0..project.clips.len())
        .filter(|&i| project.clips[i].arrangement == arr)
        .collect();
    let track_of = |i: usize| {
        if after {
            plan.new_track[i]
        } else {
            project.clips[i].track
        }
    };
    let rows = clips
        .iter()
        .map(|&i| track_of(i) + 1)
        .max()
        .unwrap_or(1)
        .max(plan.tracks_needed.min(500))
        .max(8);
    let end = clips
        .iter()
        .map(|&i| project.clips[i].position as u64 + project.clips[i].length as u64)
        .max()
        .unwrap_or(1)
        .max(1);
    let n = plan.groups.len();
    let colors: Vec<Color32> = plan
        .groups
        .iter()
        .enumerate()
        .map(|(i, g)| group_color(g, i, n))
        .collect();

    // nome del gruppo di ogni traccia (dopo)
    let mut track_name: Vec<Option<usize>> = vec![None; rows];
    if after {
        for (gi, g) in plan.groups.iter().enumerate() {
            if g.arrangement != arr {
                continue;
            }
            for t in g.first_track..(g.first_track + g.lanes).min(rows) {
                track_name[t] = Some(gi);
            }
        }
    }

    let row_h = 18.0;
    let label_w = 150.0;
    let mut hovered = None;
    egui::ScrollArea::vertical()
        .id_salt("playlist")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width();
            let (rect, resp) =
                ui.allocate_exact_size(Vec2::new(width, rows as f32 * row_h), Sense::hover());
            let painter = ui.painter_at(rect);
            let lane_x = rect.left() + label_w;
            let lane_w = (rect.right() - lane_x - 4.0).max(10.0);
            let mouse = resp.hover_pos();

            let visible = ui.clip_rect();
            for t in 0..rows {
                let y = rect.top() + t as f32 * row_h;
                if y > visible.bottom() || y + row_h < visible.top() {
                    continue;
                }
                let row =
                    Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(rect.width(), row_h));
                painter.rect_filled(
                    row,
                    CornerRadius::ZERO,
                    if t % 2 == 0 { p.lane } else { p.lane_alt },
                );
                let muted = if after {
                    track_name[t].is_some_and(|g| plan.groups[g].muted)
                } else {
                    project
                        .arrangements
                        .get(arr)
                        .and_then(|a| a.tracks.get(t))
                        .is_some_and(|x| x.muted)
                };
                let label = match track_name[t] {
                    Some(g) if t == plan.groups[g].first_track => {
                        format!("{}  {}", t + 1, plan.groups[g].name)
                    }
                    _ => format!("{}", t + 1),
                };
                let col = if muted { tint(p.weak, 120) } else { p.weak };
                let galley = painter.layout_no_wrap(label, FontId::proportional(11.5), col);
                let clipped = Rect::from_min_size(
                    Pos2::new(rect.left() + 6.0, y),
                    Vec2::new(label_w - 12.0, row_h),
                );
                painter.with_clip_rect(clipped).galley(
                    Pos2::new(rect.left() + 6.0, y + (row_h - galley.size().y) / 2.0),
                    galley,
                    col,
                );
                if muted {
                    painter.text(
                        Pos2::new(lane_x - 14.0, y + row_h / 2.0),
                        egui::Align2::CENTER_CENTER,
                        "🔇",
                        FontId::proportional(10.0),
                        p.weak,
                    );
                }
            }
            painter.vline(lane_x - 2.0, rect.y_range(), Stroke::new(1.0, p.border));

            for &i in &clips {
                let c = &project.clips[i];
                let t = track_of(i);
                let y = rect.top() + t as f32 * row_h;
                if y > visible.bottom() || y + row_h < visible.top() {
                    continue;
                }
                let x0 = lane_x + (c.position as f64 / end as f64) as f32 * lane_w;
                let x1 = lane_x
                    + ((c.position as u64 + c.length as u64) as f64 / end as f64) as f32 * lane_w;
                let r = Rect::from_min_max(
                    Pos2::new(x0, y + 2.0),
                    Pos2::new(x1.max(x0 + 3.0), y + row_h - 2.0),
                );
                let g = plan.clip_group[i];
                let mut col = colors.get(g).copied().unwrap_or(p.accent);
                if c.clip_muted || c.track_muted {
                    col = tint(col, 90);
                }
                if focus.is_some_and(|f| f != g) {
                    col = tint(col, 45);
                }
                painter.rect_filled(r, CornerRadius::same(2), col);
                if r.width() > 46.0 {
                    let txt_col = if focus.is_some_and(|f| f != g) {
                        tint(Color32::BLACK, 80)
                    } else {
                        Color32::from_black_alpha(210)
                    };
                    let galley =
                        painter.layout_no_wrap(c.name.clone(), FontId::proportional(10.5), txt_col);
                    painter.with_clip_rect(r.shrink(2.0)).galley(
                        Pos2::new(r.left() + 4.0, r.center().y - galley.size().y / 2.0),
                        galley,
                        txt_col,
                    );
                }
                if mouse.is_some_and(|m| r.contains(m)) {
                    hovered = Some(g);
                }
            }
            if let Some(g) = hovered {
                let gr = &plan.groups[g];
                resp.on_hover_ui_at_pointer(|ui| {
                    ui.label(RichText::new(&gr.name).font(bold(13.0)));
                    ui.label(
                        RichText::new(lang.f(
                            "{0} · {1} clips · tracks {2}",
                            "{0} · {1} clip · tracce {2}",
                            &[
                                &kind_label(lang, gr.kind),
                                &gr.clips,
                                &track_range(gr.first_track, gr.lanes),
                            ],
                        ))
                        .size(12.0),
                    );
                });
            }
        });
    hovered
}
