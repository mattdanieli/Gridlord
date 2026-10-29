//! Calcolo del nuovo ordine: raggruppa le clip per nome su tracce vicine.
//!
//! Non si sposta niente nel tempo: cambia solo la traccia di ogni clip. Le clip che
//! stavano su tracce silenziate finiscono in fondo, su tracce che restano silenziate.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::flp::text_event;
use super::project::{Clip, ClipKind, ID_TRACK_NAME, MAX_TRACKS, PATTERN_EVENT, Project, Source};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SortMode {
    /// A-Z, senza distinguere maiuscole e minuscole.
    #[default]
    Alphabetical,
    /// Nell'ordine in cui le clip compaiono nel brano.
    FirstAppearance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubSort {
    Type,
    Length,
    Color,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub sort: SortMode,
    /// Criteri secondari, applicati in quest'ordine prima di quello principale.
    pub then_by: Vec<SubSort>,
    pub rename_tracks: bool,
    pub color_clips: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            sort: SortMode::Alphabetical,
            then_by: vec![],
            rename_tracks: false,
            color_clips: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Group {
    pub arrangement: usize,
    pub name: String,
    pub kind: ClipKind,
    /// Prima traccia, da 0.
    pub first_track: usize,
    pub lanes: usize,
    pub clips: usize,
    pub muted: bool,
    /// Colore mostrato nell'anteprima (quello che avranno le clip, se noto).
    pub color: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Warning {
    /// Servirebbero piu di 500 tracce: alcune clip restano dove sono.
    TooManyTracks { arrangement: String, needed: usize },
}

#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub groups: Vec<Group>,
    /// Nuova traccia di ogni clip (stesso ordine di `Project::clips`).
    pub new_track: Vec<usize>,
    /// Gruppo di ogni clip.
    pub clip_group: Vec<usize>,
    pub tracks_needed: usize,
    pub moved: usize,
    pub warnings: Vec<Warning>,
    /// Modifiche di singoli byte (posizione, valore).
    pub byte_patches: Vec<(usize, u8)>,
    /// Inserimenti di nuovi eventi (posizione, byte).
    pub inserts: Vec<(usize, Vec<u8>)>,
    pub renamed_tracks: usize,
    pub colored_sources: usize,
}

/// Colori che FL Studio assegna da solo (grigi scuri poco saturi): si possono sovrascrivere.
pub fn is_default_color(rgb: u32) -> bool {
    let (r, g, b) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
    r < 0x70 && g < 0x70 && b < 0x70 && r.max(g).max(b) - r.min(g).min(b) < 0x40
}

fn hsv_to_rgb(h_deg: f64, s: f64, v: f64) -> u32 {
    let h = h_deg.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let q = |f: f64| ((f + m) * 255.0).round_ties_even() as u32;
    (q(r) << 16) | (q(g) << 8) | q(b)
}

pub fn rainbow(i: usize, n: usize) -> u32 {
    if n == 0 {
        return 0x808080;
    }
    hsv_to_rgb((i as f64 * 360.0 / n as f64) % 360.0, 0.65, 0.95)
}

/// Il colore piu frequente; a parita vince il valore piu basso.
fn dominant_color(clips: &[&Clip]) -> Option<u32> {
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for c in clips {
        if let Some(col) = c.color {
            *counts.entry(col).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|x| x.0)
}

/// Ordine arcobaleno (rosso, arancio, giallo, verde, blu, viola); grigi e senza colore in fondo.
fn hue_key(rgb: Option<u32>) -> (u8, f64, f64) {
    let Some(rgb) = rgb else { return (2, 0.0, 0.0) };
    let r = ((rgb >> 16) & 0xff) as f64 / 255.0;
    let g = ((rgb >> 8) & 0xff) as f64 / 255.0;
    let b = (rgb & 0xff) as f64 / 255.0;
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let delta = mx - mn;
    let sat = if mx == 0.0 { 0.0 } else { delta / mx };
    if delta < 1e-6 || sat < 0.1 {
        return (1, 0.0, mx);
    }
    let h = if mx == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if mx == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    (0, h * 60.0, mx)
}

#[derive(PartialEq, PartialOrd)]
enum KeyPart {
    Int(i64),
    Float(f64),
    Hue((u8, f64, f64)),
    Text(String),
}

fn cmp_keys(a: &[KeyPart], b: &[KeyPart]) -> std::cmp::Ordering {
    a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
}

fn kind_rank(k: ClipKind) -> i64 {
    match k {
        ClipKind::Audio => 0,
        ClipKind::Pattern => 1,
        ClipKind::Automation => 2,
    }
}

/// Mette le clip di un gruppo su corsie: una clip va sulla prima corsia libera in quel momento.
fn assign_lanes(clips: &[usize], all: &[Clip], base: usize, out: &mut [usize]) -> usize {
    let mut order: Vec<usize> = clips.to_vec();
    order.sort_by_key(|&i| all[i].position);
    let mut lane_end: Vec<u64> = Vec::new();
    for i in order {
        let c = &all[i];
        let (pos, end) = (c.position as u64, c.position as u64 + c.length as u64);
        match lane_end.iter().position(|&e| pos >= e) {
            Some(l) => {
                lane_end[l] = end;
                out[i] = base + l;
            }
            None => {
                lane_end.push(end);
                out[i] = base + lane_end.len() - 1;
            }
        }
    }
    lane_end.len()
}

pub fn plan(p: &Project, o: &Options) -> Plan {
    let d = &p.bytes;
    let mut plan = Plan {
        new_track: p.clips.iter().map(|c| c.track).collect(),
        clip_group: vec![0; p.clips.len()],
        ..Default::default()
    };
    // colore finale di ogni sorgente (l'ultimo assegnato vince)
    let mut source_color: BTreeMap<Source, u32> = BTreeMap::new();

    for (ai, arr) in p.arrangements.iter().enumerate() {
        let idx: Vec<usize> = (0..p.clips.len())
            .filter(|&i| p.clips[i].arrangement == ai)
            .collect();
        if idx.is_empty() {
            continue;
        }
        let group_start = plan.groups.len();
        let mut base = 0usize;
        // track -> (nome del gruppo, silenziata)
        let mut dest: HashMap<usize, (String, bool)> = HashMap::new();

        for muted_pool in [false, true] {
            // gruppi in ordine di prima comparsa nel file
            let mut names: Vec<String> = Vec::new();
            let mut members: HashMap<String, Vec<usize>> = HashMap::new();
            for &i in &idx {
                let c = &p.clips[i];
                if c.track_muted != muted_pool {
                    continue;
                }
                members
                    .entry(c.name.clone())
                    .or_insert_with(|| {
                        names.push(c.name.clone());
                        Vec::new()
                    })
                    .push(i);
            }
            let key = |n: &String| -> Vec<KeyPart> {
                let m: Vec<&Clip> = members[n].iter().map(|&i| &p.clips[i]).collect();
                let mut k = Vec::new();
                for s in &o.then_by {
                    k.push(match s {
                        SubSort::Type => KeyPart::Int(kind_rank(m[0].kind)),
                        SubSort::Length => KeyPart::Float(
                            -(m.iter().map(|c| c.length as f64).sum::<f64>() / m.len() as f64),
                        ),
                        SubSort::Color => KeyPart::Hue(hue_key(dominant_color(&m))),
                    });
                }
                k.push(match o.sort {
                    SortMode::Alphabetical => KeyPart::Text(n.to_lowercase()),
                    SortMode::FirstAppearance => {
                        KeyPart::Int(m.iter().map(|c| c.position as i64).min().unwrap_or(0))
                    }
                });
                k
            };
            let keys: HashMap<&String, Vec<KeyPart>> = names.iter().map(|n| (n, key(n))).collect();
            let mut order = names.clone();
            order.sort_by(|a, b| cmp_keys(&keys[a], &keys[b]));

            for name in order {
                let m = &members[&name];
                let first = base;
                let lanes = assign_lanes(m, &p.clips, base, &mut plan.new_track);
                for t in first..first + lanes {
                    dest.insert(t, (name.clone(), muted_pool));
                }
                let gi = plan.groups.len();
                for &i in m {
                    plan.clip_group[i] = gi;
                }
                let refs: Vec<&Clip> = m.iter().map(|&i| &p.clips[i]).collect();
                plan.groups.push(Group {
                    arrangement: ai,
                    name: name.clone(),
                    kind: refs[0].kind,
                    first_track: first,
                    lanes,
                    clips: m.len(),
                    muted: muted_pool,
                    color: dominant_color(&refs),
                });
                base += lanes;
            }
        }

        if base > MAX_TRACKS {
            plan.warnings.push(Warning::TooManyTracks {
                arrangement: arr.name.clone(),
                needed: base,
            });
        }
        plan.tracks_needed = plan.tracks_needed.max(base.min(MAX_TRACKS));

        // Stato attiva/silenziata delle tracce: le tracce del gruppo silenziato restano silenziate
        for (t, tr) in arr.tracks.iter().enumerate() {
            let muted = dest.get(&t).is_some_and(|x| x.1);
            let want = if muted { 0 } else { 1 };
            if d[tr.mute_off] != want {
                plan.byte_patches.push((tr.mute_off, want));
            }
        }

        // Colori arcobaleno: si colorano canali e pattern di ogni gruppo (non i colori scelti a mano)
        if o.color_clips {
            let n = plan.groups.len() - group_start;
            for (gi, g) in plan.groups[group_start..].iter_mut().enumerate() {
                let col = rainbow(gi, n);
                let mut any = false;
                for &i in &idx {
                    let c = &p.clips[i];
                    if c.name != g.name || c.track_muted != g.muted {
                        continue;
                    }
                    let current = match c.source {
                        Source::Channel(id) => p.channels.get(&id).and_then(|x| x.color),
                        Source::Pattern(id) => p.patterns.get(&id).and_then(|x| x.color),
                    };
                    let paintable = match (c.source, current) {
                        (Source::Channel(_), None) => false, // nessun evento colore da modificare
                        (_, Some((rgb, _))) => is_default_color(rgb),
                        (Source::Pattern(id), None) => p
                            .patterns
                            .get(&id)
                            .is_some_and(|x| x.insert_color_at.is_some()),
                    };
                    if paintable {
                        source_color.insert(c.source, col);
                        any = true;
                    }
                }
                if any {
                    g.color = Some(col);
                }
            }
        }

        // Nomi delle tracce: il nome del gruppo, solo sulle tracce che non hanno gia un nome
        if o.rename_tracks {
            for (t, tr) in arr.tracks.iter().enumerate() {
                if tr.has_name {
                    continue;
                }
                if let Some((name, _)) = dest.get(&t) {
                    let short: String = name.chars().take(120).collect();
                    plan.inserts
                        .push((tr.data_end, text_event(ID_TRACK_NAME, &short)));
                    plan.renamed_tracks += 1;
                }
            }
        }

        for &i in &idx {
            let c = &p.clips[i];
            let t = plan.new_track[i];
            if t >= MAX_TRACKS {
                plan.new_track[i] = c.track; // non c'e posto: resta dove sta
                continue;
            }
            if t != c.track {
                let rv = (MAX_TRACKS - 1 - t) as u16;
                let [lo, hi] = rv.to_le_bytes();
                plan.byte_patches.push((c.track_off, lo));
                plan.byte_patches.push((c.track_off + 1, hi));
                plan.moved += 1;
            }
        }
    }

    for (src, col) in &source_color {
        let [r, g, b] = [(col >> 16) as u8, (col >> 8) as u8, *col as u8];
        let existing = match src {
            Source::Channel(id) => p.channels.get(id).and_then(|x| x.color),
            Source::Pattern(id) => p.patterns.get(id).and_then(|x| x.color),
        };
        match (existing, src) {
            (Some((_, off)), _) => {
                for (k, v) in [r, g, b].into_iter().enumerate() {
                    if d[off + k] != v {
                        plan.byte_patches.push((off + k, v));
                    }
                }
            }
            (None, Source::Pattern(id)) => {
                if let Some(at) = p.patterns.get(id).and_then(|x| x.insert_color_at) {
                    plan.inserts.push((at, vec![PATTERN_EVENT, r, g, b, 0]));
                }
            }
            _ => {}
        }
        plan.colored_sources += 1;
    }
    plan
}
