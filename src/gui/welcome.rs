//! Finestra di benvenuto: cinque schede che spiegano a cosa serve Gridlord e come si usa.
//! Si apre da sola al primo avvio e poi dal pulsante "?" in alto.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Frame, Key, Margin, Painter, Pos2, Rect, RichText,
    Sense, Shape, Stroke, TextureId, Vec2,
};

use super::{App, Lang, Palette, accent_button, bold, tint};

const PAGES: usize = 5;
const WIDTH: f32 = 760.0;
const ART_H: f32 = 300.0;

pub struct Welcome {
    page: usize,
    /// Quando e comparsa la scheda attuale (per le animazioni).
    since: Option<f64>,
    /// +1 avanti, -1 indietro: da che lato entra la scheda.
    dir: f32,
}

impl Welcome {
    pub fn new() -> Self {
        // Solo per gli screenshot di sviluppo: apre direttamente una scheda.
        #[cfg(debug_assertions)]
        if let Some(n) = std::env::var("GRIDLORD_WELCOME_PAGE").ok().and_then(|v| v.parse().ok()) {
            return Welcome { page: n, since: None, dir: 1.0 };
        }
        Welcome { page: 0, since: None, dir: 1.0 }
    }
}

impl App {
    pub(super) fn welcome_ui(&mut self, ctx: &egui::Context) {
        let Some(w) = self.welcome.as_mut() else { return };
        let p = self.pal;
        let lang = self.lang;
        let logo = self.logo.id();
        let now = ctx.input(|i| i.time);
        let t = (now - *w.since.get_or_insert(now)) as f32;
        if t < 3.0 {
            ctx.request_repaint();
        }
        let (page, dir) = (w.page, w.dir);
        let last = page == PAGES - 1;

        let frame = Frame::new()
            .fill(p.card)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(18))
            .inner_margin(Margin::ZERO);
        let mut go: Option<usize> = None;
        let mut close = false;

        let r = egui::Modal::new(egui::Id::unique("welcome")).frame(frame).show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let e = ease(t / 0.45);

            // Illustrazione
            let (rect, _) = ui.allocate_exact_size(Vec2::new(WIDTH, ART_H), Sense::hover());
            let bg = if self.settings.dark { Color32::from_rgb(0x17, 0x1a, 0x20) } else { p.card_hi };
            ui.painter().rect_filled(rect, CornerRadius { nw: 18, ne: 18, sw: 0, se: 0 }, bg);
            let mut painter = ui.painter_at(rect);
            painter.multiply_opacity(e);
            let art = rect.translate(Vec2::new((1.0 - e) * 36.0 * dir, 0.0));
            match page {
                0 => art_hello(&painter, art, &p, t, logo),
                1 => art_group(&painter, art, &p, lang, t),
                2 => art_steps(&painter, art, &p, lang, t),
                3 => art_touches(&painter, art, &p, t),
                _ => art_safe(&painter, art, &p, lang, t),
            }

            // Testo
            Frame::new().inner_margin(Margin { left: 40, right: 40, top: 26, bottom: 6 }).show(ui, |ui| {
                ui.set_width(WIDTH - 80.0);
                ui.set_min_height(150.0);
                ui.multiply_opacity(e);
                let (title, body) = page_text(lang, page);
                ui.label(RichText::new(format!("{} / {PAGES}", page + 1)).color(p.accent).font(bold(12.5)));
                ui.add_space(6.0);
                ui.label(RichText::new(title).font(bold(24.0)).color(p.text));
                ui.add_space(10.0);
                ui.label(RichText::new(body).size(14.5).color(p.weak).line_height(Some(21.0)));
            });

            // Piede: salta, pallini, indietro / avanti
            Frame::new().inner_margin(Margin { left: 28, right: 28, top: 8, bottom: 22 }).show(ui, |ui| {
                let (row, _) = ui.allocate_exact_size(Vec2::new(WIDTH - 56.0, 38.0), Sense::hover());
                if !last {
                    let skip = egui::Button::new(RichText::new(lang.t("Skip", "Salta")).color(p.weak).size(14.0)).frame(false);
                    if ui.put(Rect::from_min_size(row.left_top(), Vec2::new(90.0, 38.0)), skip).clicked() {
                        close = true;
                    }
                }
                let gap = 22.0;
                let x0 = row.center().x - gap * (PAGES as f32 - 1.0) / 2.0;
                for i in 0..PAGES {
                    let c = Pos2::new(x0 + gap * i as f32, row.center().y);
                    let hit = Rect::from_center_size(c, Vec2::splat(16.0));
                    let resp = ui.interact(hit, ui.scope_id().with(("dot", i)), Sense::click());
                    if resp.clicked() {
                        go = Some(i);
                    }
                    let (size, col) = if i == page {
                        (Vec2::new(24.0, 8.0), p.accent)
                    } else if resp.hovered() {
                        (Vec2::splat(8.0), p.weak)
                    } else {
                        (Vec2::splat(8.0), tint(p.weak, 90))
                    };
                    ui.painter().rect_filled(Rect::from_center_size(c, size), CornerRadius::same(4), col);
                }
                let next_label = if last { lang.t("Get started", "Inizia") } else { lang.t("Next", "Avanti") };
                let next_rect = Rect::from_min_size(Pos2::new(row.right() - 160.0, row.top()), Vec2::new(160.0, 38.0));
                if ui.put(next_rect, accent_button(&p, next_label, true)).clicked() {
                    if last {
                        close = true;
                    } else {
                        go = Some(page + 1);
                    }
                }
                if page > 0 {
                    let back = egui::Button::new(RichText::new(lang.t("Back", "Indietro")).size(14.0));
                    let back_rect = Rect::from_min_size(Pos2::new(next_rect.left() - 118.0, row.top()), Vec2::new(108.0, 38.0));
                    if ui.put(back_rect, back).clicked() {
                        go = Some(page - 1);
                    }
                }
            });
        });

        ctx.input(|i| {
            if i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::Enter) {
                if last && i.key_pressed(Key::Enter) {
                    close = true;
                } else if !last {
                    go = Some(page + 1);
                }
            }
            if i.key_pressed(Key::ArrowLeft) && page > 0 {
                go = Some(page - 1);
            }
        });
        if r.should_close() {
            close = true;
        }

        if close {
            self.welcome = None;
            self.settings.welcome_seen = true;
        } else if let Some(n) = go.filter(|&n| n != page)
            && let Some(w) = self.welcome.as_mut()
        {
            w.dir = if n > page { 1.0 } else { -1.0 };
            w.page = n;
            w.since = None;
        }
    }
}

fn page_text(l: Lang, page: usize) -> (&'static str, &'static str) {
    match page {
        0 => (
            l.t("Welcome to Gridlord", "Benvenuto in Gridlord"),
            l.t(
                "Gridlord tidies up the playlist of your FL Studio projects in one click. Here is a quick tour: it takes 30 seconds.",
                "Gridlord mette in ordine la playlist dei tuoi progetti di FL Studio con un clic. Ecco un giro veloce: ci vogliono 30 secondi.",
            ),
        ),
        1 => (
            l.t("Every clip, grouped by name", "Ogni clip, raggruppata per nome"),
            l.t(
                "After a long session the same sample ends up scattered over dozens of tracks. Gridlord moves every clip with the same name onto neighbouring tracks. Nothing moves in time, so your song sounds exactly the same.",
                "Dopo una lunga sessione lo stesso sample finisce sparso su decine di tracce. Gridlord sposta tutte le clip con lo stesso nome su tracce vicine. Niente si sposta nel tempo, quindi la canzone suona esattamente uguale.",
            ),
        ),
        2 => (
            l.t("Three steps", "Tre passaggi"),
            l.t(
                "Drop one or more .flp files (or a whole folder) on the window, choose the track order, then press \"Organize and save\". The preview shows the result before anything is saved.",
                "Trascina uno o più file .flp (o una cartella intera) sulla finestra, scegli l'ordine delle tracce e premi \"Organizza e salva\". L'anteprima ti mostra il risultato prima di salvare.",
            ),
        ),
        3 => (
            l.t("Finishing touches", "Ritocchi finali"),
            l.t(
                "If you like, Gridlord names every track after its group and colors the groups like a rainbow (clips you colored yourself keep their color). Muted tracks move to the bottom, still muted.",
                "Se vuoi, Gridlord dà a ogni traccia il nome del suo gruppo e colora i gruppi come un arcobaleno (le clip che hai colorato tu mantengono il loro colore). Le tracce mute vanno in fondo e restano mute.",
            ),
        ),
        _ => (
            l.t("Your original stays safe", "L'originale resta al sicuro"),
            l.t(
                "By default Gridlord saves a new file next to the original, ending in _organized.flp. Every file is read back and checked before it is kept. Ready? Drop your first project.",
                "Di base Gridlord salva un nuovo file accanto all'originale, che finisce con _organized.flp. Ogni file viene riletto e controllato prima di essere tenuto. Pronto? Trascina il tuo primo progetto.",
            ),
        ),
    }
}

// --------------------------------------------------------------------------
// Illustrazioni (disegnate, niente immagini: restano nitide a ogni scala)
// --------------------------------------------------------------------------

const LANES: [Color32; 4] = [
    Color32::from_rgb(0xff, 0x8a, 0x2b),
    Color32::from_rgb(0xf2, 0xc1, 0x4e),
    Color32::from_rgb(0x3e, 0xcf, 0x8e),
    Color32::from_rgb(0x5a, 0xaa, 0xff),
];

fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

/// Avanzamento 0..1 di un'animazione che parte a `start` secondi e dura `dur`.
fn phase(t: f32, start: f32, dur: f32) -> f32 {
    ease((t - start) / dur)
}

fn lerp(a: f32, b: f32, x: f32) -> f32 {
    a + (b - a) * x
}

/// Arcobaleno come quello dei gruppi (stessa formula di organize::rainbow).
fn rainbow(i: usize, n: usize) -> Color32 {
    let c = crate::engine::organize::rainbow(i, n);
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn art_hello(pt: &Painter, r: Rect, p: &Palette, t: f32, logo: TextureId) {
    let c = r.center();
    // corsie di clip che entrano dai lati
    let h = 26.0;
    let gap = 14.0;
    let top = c.y - (4.0 * h + 3.0 * gap) / 2.0;
    let pieces: [&[(f32, f32)]; 4] = [
        &[(0.02, 0.10), (0.15, 0.08), (0.27, 0.06), (0.67, 0.09), (0.80, 0.12)],
        &[(0.05, 0.14), (0.22, 0.07), (0.70, 0.06), (0.79, 0.16)],
        &[(0.01, 0.07), (0.11, 0.12), (0.26, 0.05), (0.66, 0.13), (0.83, 0.07)],
        &[(0.04, 0.09), (0.16, 0.13), (0.69, 0.08), (0.80, 0.10), (0.93, 0.05)],
    ];
    for (i, row) in pieces.iter().enumerate() {
        let k = phase(t, 0.05 * i as f32, 0.7);
        let y = top + i as f32 * (h + gap);
        for &(x, w) in row.iter() {
            let left = x < 0.5;
            let slide = (1.0 - k) * 120.0 * if left { -1.0 } else { 1.0 };
            let x0 = r.left() + 30.0 + x * (r.width() - 60.0) + slide;
            let rr = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w * (r.width() - 60.0) - 6.0, h));
            pt.rect_filled(rr, CornerRadius::same(6), tint(LANES[i], (150.0 * k) as u8));
        }
    }
    // logo
    let s = lerp(0.82, 1.0, phase(t, 0.0, 0.6)) * 150.0;
    pt.circle_filled(c, s * 0.62, tint(Color32::BLACK, 60));
    pt.image(
        logo,
        Rect::from_center_size(c, Vec2::splat(s)),
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
    let _ = p;
}

/// La playlist prima e dopo: le clip scivolano sulle tracce del loro gruppo.
fn art_group(pt: &Painter, r: Rect, p: &Palette, l: Lang, t: f32) {
    // (gruppo, inizio, lunghezza, traccia prima)
    const CLIPS: [(usize, f32, f32, usize); 12] = [
        (0, 0.00, 0.11, 3),
        (0, 0.25, 0.11, 6),
        (0, 0.50, 0.11, 1),
        (0, 0.75, 0.11, 5),
        (1, 0.04, 0.19, 0),
        (1, 0.54, 0.19, 4),
        (2, 0.12, 0.27, 7),
        (2, 0.62, 0.29, 2),
        (3, 0.30, 0.17, 5),
        (3, 0.80, 0.16, 3),
        (1, 0.79, 0.10, 6),
        (3, 0.06, 0.14, 2),
    ];
    const NAMES: [&str; 4] = ["Kick", "Bass", "Chords", "Vocal"];
    let area = r.shrink2(Vec2::new(46.0, 30.0));
    let rows = 8;
    let row_h = area.height() / rows as f32;
    let label_w = 96.0;
    let lane_x = area.left() + label_w;
    let lane_w = area.right() - lane_x;
    let k = phase(t, 0.7, 1.0);

    for i in 0..rows {
        let y = area.top() + i as f32 * row_h;
        let row = Rect::from_min_size(Pos2::new(area.left(), y), Vec2::new(area.width(), row_h));
        let fill = if i % 2 == 0 { tint(p.card, 230) } else { tint(p.card_hi, 230) };
        pt.rect_filled(row, CornerRadius::ZERO, fill);
        let label = if i < 4 && k > 0.5 { format!("{}  {}", i + 1, NAMES[i]) } else { format!("{}", i + 1) };
        pt.text(Pos2::new(area.left() + 10.0, y + row_h / 2.0), Align2::LEFT_CENTER, label, FontId::proportional(12.0), p.weak);
    }
    pt.vline(lane_x - 2.0, area.y_range(), Stroke::new(1.0, p.border));

    for (n, &(g, x, w, before)) in CLIPS.iter().enumerate() {
        let kk = phase(t, 0.7 + n as f32 * 0.03, 0.9);
        let row = lerp(before as f32, g as f32, kk);
        let y = area.top() + row * row_h;
        let rr = Rect::from_min_size(Pos2::new(lane_x + x * lane_w, y + 3.0), Vec2::new(w * lane_w - 2.0, row_h - 6.0));
        pt.rect_filled(rr, CornerRadius::same(3), LANES[g]);
        if rr.width() > 48.0 {
            pt.with_clip_rect(rr.shrink(2.0)).text(
                Pos2::new(rr.left() + 6.0, rr.center().y),
                Align2::LEFT_CENTER,
                NAMES[g],
                FontId::proportional(11.0),
                Color32::from_black_alpha(200),
            );
        }
    }
    // etichetta prima / dopo
    let (txt, col) = if k < 0.5 { (l.t("Before", "Prima"), p.weak) } else { (l.t("After", "Dopo"), p.green) };
    let g = pt.layout_no_wrap(txt.to_string(), super::bold(12.0), col);
    let chip = Rect::from_min_size(Pos2::new(area.right() - g.size().x - 22.0, r.top() + 6.0), g.size() + Vec2::new(16.0, 6.0));
    pt.rect_filled(chip, CornerRadius::same(10), tint(col, 40));
    pt.galley(chip.min + Vec2::new(8.0, 3.0), g, col);
}

/// Tre schede: trascina, scegli l'ordine, salva.
fn art_steps(pt: &Painter, r: Rect, p: &Palette, l: Lang, t: f32) {
    let labels = [
        l.t("Drop your projects", "Trascina i progetti"),
        l.t("Pick an order", "Scegli l'ordine"),
        l.t("✔  Organize and save", "✔  Organizza e salva"),
    ];
    let cw = 196.0;
    let ch = 210.0;
    let gap = 28.0;
    let x0 = r.center().x - (3.0 * cw + 2.0 * gap) / 2.0;
    for (i, label) in labels.iter().enumerate() {
        let k = phase(t, 0.15 + i as f32 * 0.22, 0.55);
        let card = Rect::from_min_size(
            Pos2::new(x0 + i as f32 * (cw + gap), r.center().y - ch / 2.0 + (1.0 - k) * 24.0),
            Vec2::new(cw, ch),
        );
        let a = (255.0 * k) as u8;
        pt.rect_filled(card, CornerRadius::same(14), tint(p.card, a));
        pt.rect_stroke(card, CornerRadius::same(14), Stroke::new(1.0, tint(p.border, a)), egui::StrokeKind::Inside);
        // numero
        let badge = Pos2::new(card.left() + 26.0, card.top() + 26.0);
        pt.circle_filled(badge, 13.0, tint(LANES[i], a));
        pt.text(badge, Align2::CENTER_CENTER, format!("{}", i + 1), super::bold(13.0), tint(Color32::from_rgb(0x1a, 0x10, 0x05), a));
        // disegno
        let c = Pos2::new(card.center().x, card.top() + 98.0);
        match i {
            0 => {
                // file .flp con freccia che scende
                let drop = (t * 1.4 - 0.6).rem_euclid(1.6).min(1.0);
                let doc = Rect::from_center_size(c + Vec2::new(0.0, 8.0), Vec2::new(58.0, 72.0));
                pt.rect_filled(doc, CornerRadius::same(8), tint(p.card_hi, a));
                pt.rect_stroke(doc, CornerRadius::same(8), Stroke::new(2.0, tint(LANES[0], a)), egui::StrokeKind::Inside);
                pt.text(doc.center() + Vec2::new(0.0, 12.0), Align2::CENTER_CENTER, ".flp", super::bold(14.0), tint(LANES[0], a));
                let ay = lerp(c.y - 50.0, c.y - 24.0, ease(drop));
                arrow(pt, Pos2::new(c.x, ay - 22.0), Pos2::new(c.x, ay), tint(p.text, a));
            }
            1 => {
                for j in 0..4 {
                    let w = [86.0, 70.0, 54.0, 38.0][j];
                    let rr = Rect::from_min_size(Pos2::new(c.x - 43.0, c.y - 34.0 + j as f32 * 18.0), Vec2::new(w, 12.0));
                    pt.rect_filled(rr, CornerRadius::same(4), tint(LANES[j], a));
                }
                pt.text(Pos2::new(c.x + 40.0, c.y + 34.0), Align2::RIGHT_CENTER, "A – Z", super::bold(12.0), tint(p.weak, a));
            }
            _ => {
                let pulse = 1.0 + 0.06 * (t * 3.0).sin().max(0.0) * (1.0 - phase(t, 2.2, 0.6));
                pt.circle_filled(c, 34.0 * pulse, tint(p.green, (60.0 * k) as u8));
                pt.circle_stroke(c, 34.0 * pulse, Stroke::new(3.0, tint(p.green, a)));
                pt.line_segment([c + Vec2::new(-14.0, 1.0), c + Vec2::new(-4.0, 11.0)], Stroke::new(5.0, tint(p.green, a)));
                pt.line_segment([c + Vec2::new(-4.0, 11.0), c + Vec2::new(16.0, -11.0)], Stroke::new(5.0, tint(p.green, a)));
            }
        }
        let g = pt.layout(label.trim().to_string(), super::bold(13.5), tint(p.text, a), cw - 24.0);
        pt.galley(Pos2::new(card.center().x - g.size().x / 2.0, card.bottom() - 30.0 - g.size().y / 2.0), g, tint(p.text, a));
    }
}

/// Nomi delle tracce e colori arcobaleno; la traccia muta va in fondo.
fn art_touches(pt: &Painter, r: Rect, p: &Palette, t: f32) {
    const NAMES: [&str; 6] = ["Kick", "Snare", "Bass", "Chords", "Lead", "FX Riser"];
    const CLIPS: [&[(f32, f32)]; 6] = [
        &[(0.00, 0.10), (0.25, 0.10), (0.50, 0.10), (0.75, 0.10)],
        &[(0.12, 0.08), (0.37, 0.08), (0.62, 0.08), (0.87, 0.08)],
        &[(0.00, 0.22), (0.50, 0.22)],
        &[(0.00, 0.48), (0.50, 0.48)],
        &[(0.25, 0.24), (0.75, 0.22)],
        &[(0.40, 0.09), (0.90, 0.09)],
    ];
    let area = r.shrink2(Vec2::new(46.0, 30.0));
    let rows = NAMES.len();
    let row_h = area.height() / rows as f32;
    let label_w = 126.0;
    let lane_x = area.left() + label_w;
    let lane_w = area.right() - lane_x;
    for i in 0..rows {
        let muted = i == rows - 1;
        let y = area.top() + i as f32 * row_h;
        let row = Rect::from_min_size(Pos2::new(area.left(), y), Vec2::new(area.width(), row_h));
        pt.rect_filled(row, CornerRadius::ZERO, if i % 2 == 0 { tint(p.card, 230) } else { tint(p.card_hi, 230) });
        let k = phase(t, 0.3 + i as f32 * 0.16, 0.5);
        // il nome si scrive lettera per lettera
        let name = NAMES[i];
        let shown: String = name.chars().take((name.chars().count() as f32 * k).ceil() as usize).collect();
        let col = if muted { tint(p.weak, 140) } else { p.text };
        pt.text(Pos2::new(area.left() + 10.0, y + row_h / 2.0), Align2::LEFT_CENTER, format!("{}  {shown}", i + 1), super::bold(12.5), col);
        if muted {
            pt.text(Pos2::new(lane_x - 16.0, y + row_h / 2.0), Align2::CENTER_CENTER, "🔇", FontId::proportional(12.0), p.weak);
        }
        let base = rainbow(i, rows - 1);
        for &(x, w) in CLIPS[i] {
            let rr = Rect::from_min_size(Pos2::new(lane_x + x * lane_w, y + 4.0), Vec2::new(w * lane_w - 2.0, row_h - 8.0));
            let grey = Color32::from_rgb(0x8a, 0x91, 0x9e);
            let mut c = if muted { grey } else { mix(grey, base, k) };
            if muted {
                c = tint(c, 90);
            }
            pt.rect_filled(rr, CornerRadius::same(4), c);
        }
    }
    pt.vline(lane_x - 2.0, area.y_range(), Stroke::new(1.0, p.border));
}

/// L'originale resta com'e; accanto nasce il file organizzato.
fn art_safe(pt: &Painter, r: Rect, p: &Palette, l: Lang, t: f32) {
    let c = r.center() - Vec2::new(0.0, 22.0);
    let left = Pos2::new(c.x - 180.0, c.y);
    let right = Pos2::new(c.x + 180.0, c.y);
    let lr = doc(pt, left, p, "Song.flp", p.weak, 255);
    lock(pt, lr.right_bottom() - Vec2::new(8.0, 8.0), p);
    pt.text(Pos2::new(left.x, lr.bottom() + 44.0), Align2::CENTER_TOP, l.t("Original, untouched", "Originale, intatto"), super::bold(13.5), p.text);

    let k = phase(t, 0.35, 0.7);
    arrow(pt, Pos2::new(c.x - 70.0, c.y), Pos2::new(lerp(c.x - 70.0, c.x + 70.0, k), c.y), p.accent);

    let k2 = phase(t, 0.9, 0.6);
    let a = (255.0 * k2) as u8;
    let rr = doc(pt, right + Vec2::new(0.0, (1.0 - k2) * 16.0), p, "Song_organized.flp", p.green, a);
    let ck = rr.right_bottom() - Vec2::new(8.0, 8.0);
    pt.circle_filled(ck, 19.0, tint(p.green, a));
    pt.line_segment([ck + Vec2::new(-8.0, 0.0), ck + Vec2::new(-2.0, 7.0)], Stroke::new(3.5, tint(Color32::WHITE, a)));
    pt.line_segment([ck + Vec2::new(-2.0, 7.0), ck + Vec2::new(9.0, -6.0)], Stroke::new(3.5, tint(Color32::WHITE, a)));
    pt.text(Pos2::new(right.x, rr.bottom() + 44.0), Align2::CENTER_TOP, l.t("New file, checked", "Nuovo file, controllato"), super::bold(13.5), tint(p.green, a));
}

/// Un documento con le corsie colorate; il nome del file sotto. Restituisce il riquadro.
fn doc(pt: &Painter, c: Pos2, p: &Palette, name: &str, accent: Color32, a: u8) -> Rect {
    let rect = Rect::from_center_size(c, Vec2::new(112.0, 136.0));
    pt.rect_filled(rect, CornerRadius::same(12), tint(p.card, a));
    pt.rect_stroke(rect, CornerRadius::same(12), Stroke::new(2.0, tint(accent, a)), egui::StrokeKind::Inside);
    for i in 0..4 {
        let w = [72.0, 56.0, 64.0, 44.0][i];
        let rr = Rect::from_min_size(Pos2::new(rect.left() + 20.0, rect.top() + 28.0 + i as f32 * 20.0), Vec2::new(w, 10.0));
        pt.rect_filled(rr, CornerRadius::same(5), tint(LANES[i], (a as f32 * 0.85) as u8));
    }
    pt.text(Pos2::new(c.x, rect.bottom() + 18.0), Align2::CENTER_CENTER, name, FontId::proportional(12.5), tint(p.weak, a));
    rect
}

/// Lucchetto disegnato (le emoji non sono uguali su ogni sistema).
fn lock(pt: &Painter, c: Pos2, p: &Palette) {
    pt.circle_filled(c, 19.0, p.card_hi);
    pt.circle_stroke(c, 19.0, Stroke::new(2.0, p.weak));
    let body = Rect::from_center_size(c + Vec2::new(0.0, 3.0), Vec2::new(16.0, 12.0));
    let arc: Vec<Pos2> = (0..=12)
        .map(|i| {
            let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
            Pos2::new(c.x + 5.0 * a.cos(), body.top() + 5.0 * a.sin())
        })
        .collect();
    pt.add(Shape::line(arc, Stroke::new(2.0, p.weak)));
    pt.rect_filled(body, CornerRadius::same(2), p.weak);
}

fn arrow(pt: &Painter, from: Pos2, to: Pos2, col: Color32) {
    let d = to - from;
    if d.length() < 1.0 {
        return;
    }
    let dir = d.normalized();
    let n = Vec2::new(-dir.y, dir.x);
    pt.line_segment([from, to - dir * 8.0], Stroke::new(4.0, col));
    pt.add(Shape::convex_polygon(
        vec![to + dir * 4.0, to - dir * 10.0 + n * 9.0, to - dir * 10.0 - n * 9.0],
        col,
        Stroke::NONE,
    ));
}

fn mix(a: Color32, b: Color32, k: f32) -> Color32 {
    let m = |x: u8, y: u8| lerp(x as f32, y as f32, k) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}
