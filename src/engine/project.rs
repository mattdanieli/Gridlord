//! Modello del progetto: canali, pattern, arrangement, tracce e clip della playlist.

use std::collections::BTreeMap;
use std::path::Path;

use super::flp::{self, DWORD, FlpError, Parsed, TEXT, WORD, decode_text, u16_at, u32_at};

const ID_CHANNEL_TYPE: u8 = 21;
const ID_CHANNEL_NEW: u8 = WORD; // 64
const ID_PATTERN_NEW: u8 = WORD + 1; // 65
const ID_ARRANGEMENT_NEW: u8 = WORD + 35; // 99
const ID_CHANNEL_COLOR: u8 = DWORD; // 128
const ID_PATTERN_COLOR: u8 = DWORD + 22; // 150
const ID_PATTERN_NAME: u8 = TEXT + 1; // 193
const ID_CHANNEL_SAMPLE: u8 = TEXT + 4; // 196
const ID_DEFAULT_NAME: u8 = TEXT + 9; // 201
const ID_DISPLAY_NAME: u8 = TEXT + 11; // 203: nome del canale come appare in FL
const ID_CHANNEL_NAME: u8 = TEXT + 39; // 231
const ID_PLAYLIST: u8 = 208 + 25; // 233
const ID_TRACK_DATA: u8 = 208 + 30; // 238
pub const ID_TRACK_NAME: u8 = TEXT + 47; // 239
const ID_ARRANGEMENT_NAME: u8 = TEXT + 49; // 241

pub const PATTERN_EVENT: u8 = ID_PATTERN_COLOR;

/// Il campo "pattern_base" vale sempre 20480 nelle clip vere.
const PATTERN_BASE: u16 = 20480;
/// FL Studio ha 500 tracce di playlist; nel file l'indice e salvato al contrario (499 - traccia).
pub const MAX_TRACKS: usize = 500;
/// Dimensioni possibili di una clip, dalla piu piccola: 80 = FL 24/25, 88 = FL 2026.
const ITEM_SIZES: [usize; 6] = [32, 60, 64, 80, 88, 320];

/// Dentro i dati di una traccia: byte 12 = 1 attiva / 0 silenziata.
const TRACK_MUTE_OFFSET: usize = 12;
/// Dentro una clip: bit 0x20 del byte 19 = clip silenziata singolarmente.
const CLIP_FLAGS_OFFSET: usize = 19;
const CLIP_MUTED_BIT: u8 = 0x20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ClipKind {
    Audio,
    Automation,
    Pattern,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Channel(u16),
    Pattern(u16),
}

#[derive(Clone, Debug, Default)]
pub struct Channel {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub sample: Option<String>,
    pub default_name: Option<String>,
    pub kind: Option<u8>,
    /// Colore 0xRRGGBB e posizione dei suoi byte nel file.
    pub color: Option<(u32, usize)>,
}

#[derive(Clone, Debug, Default)]
pub struct Pattern {
    pub name: Option<String>,
    pub color: Option<(u32, usize)>,
    /// Dove inserire un evento colore se il pattern non ne ha uno.
    pub insert_color_at: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub mute_off: usize,
    pub muted: bool,
    /// Primo byte dopo i dati della traccia: qui si inserisce il nome.
    pub data_end: usize,
    pub has_name: bool,
}

#[derive(Clone, Debug)]
pub struct Arrangement {
    pub name: String,
    pub item_size: usize,
    pub tracks: Vec<Track>,
    /// Elementi della playlist che non sembrano clip valide (ignorati).
    pub skipped_items: usize,
    pub has_playlist: bool,
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub arrangement: usize,
    pub name: String,
    pub kind: ClipKind,
    pub source: Source,
    pub position: u32,
    pub length: u32,
    /// Traccia originale, da 0.
    pub track: usize,
    pub track_muted: bool,
    pub clip_muted: bool,
    pub color: Option<u32>,
    /// Posizione dei 2 byte dell'indice di traccia.
    pub track_off: usize,
}

pub struct Project {
    pub bytes: Vec<u8>,
    pub hash: blake3::Hash,
    pub version: String,
    pub major: u32,
    pub fldt_pos: usize,
    pub trailing_bytes: usize,
    pub channels: BTreeMap<u16, Channel>,
    pub patterns: BTreeMap<u16, Pattern>,
    pub arrangements: Vec<Arrangement>,
    pub clips: Vec<Clip>,
    /// Dimensioni dei dati delle tracce trovate (per il controllo di compatibilita).
    pub track_data_sizes: Vec<usize>,
}

pub fn load(path: &Path) -> Result<Project, FlpError> {
    let bytes = std::fs::read(path).map_err(|e| FlpError::Io(e.to_string()))?;
    Project::from_bytes(bytes)
}

fn decode_color(d: &[u8]) -> u32 {
    ((d[0] as u32) << 16) | ((d[1] as u32) << 8) | d[2] as u32
}

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

/// Sceglie la dimensione delle clip: quella per cui ogni elemento ha pattern_base = 20480.
fn detect_item_size(data: &[u8], off: usize, total: usize) -> Option<usize> {
    let good = |size: usize| {
        (0..total / size)
            .filter(|k| u16_at(data, off + k * size + 4) == PATTERN_BASE)
            .count()
    };
    for size in ITEM_SIZES {
        if total % size == 0 && total / size > 0 && good(size) == total / size {
            return Some(size);
        }
    }
    let mut best = None;
    let mut best_ratio = 0.5;
    for size in ITEM_SIZES {
        if total % size != 0 || total / size == 0 {
            continue;
        }
        let ratio = good(size) as f64 / (total / size) as f64;
        if ratio > best_ratio {
            best_ratio = ratio;
            best = Some(size);
        }
    }
    best
}

impl Project {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Project, FlpError> {
        let parsed: Parsed = flp::parse(&bytes)?;
        let hash = blake3::hash(&bytes);
        let d = &bytes;
        let unicode = parsed.major >= 12 || (parsed.major == 11 && parsed.minor >= 5);

        let mut channels: BTreeMap<u16, Channel> = BTreeMap::new();
        let mut patterns: BTreeMap<u16, Pattern> = BTreeMap::new();
        let mut arrangements: Vec<Arrangement> = Vec::new();
        let mut playlists: Vec<Option<(usize, usize)>> = Vec::new();
        let mut track_data_sizes = Vec::new();
        let mut cur_ch: Option<u16> = None;
        let mut cur_pat: Option<u16> = None;

        for ev in &parsed.events {
            let p = &d[ev.data..ev.end()];
            match ev.id {
                ID_CHANNEL_NEW => {
                    let id = u16_at(d, ev.data);
                    channels.entry(id).or_default();
                    cur_ch = Some(id);
                    cur_pat = None;
                }
                ID_PATTERN_NEW => {
                    let id = u16_at(d, ev.data);
                    let pat = patterns.entry(id).or_default();
                    pat.insert_color_at = Some(ev.end());
                    cur_pat = Some(id);
                    cur_ch = None;
                }
                ID_ARRANGEMENT_NEW => {
                    arrangements.push(Arrangement {
                        name: String::new(),
                        item_size: 0,
                        tracks: Vec::new(),
                        skipped_items: 0,
                        has_playlist: false,
                    });
                    playlists.push(None);
                    cur_ch = None;
                    cur_pat = None;
                }
                _ => {}
            }
            if let Some(c) = cur_ch.and_then(|c| channels.get_mut(&c)) {
                match ev.id {
                    ID_CHANNEL_TYPE => c.kind = Some(p[0]),
                    ID_CHANNEL_NAME => c.name = non_empty(decode_text(p, unicode)),
                    // solo il primo: quelli dopo appartengono ai plugin del mixer
                    ID_DISPLAY_NAME if c.display_name.is_none() => {
                        c.display_name = non_empty(decode_text(p, unicode))
                    }
                    ID_CHANNEL_SAMPLE => c.sample = non_empty(decode_text(p, unicode)),
                    ID_DEFAULT_NAME if c.default_name.is_none() => {
                        c.default_name = non_empty(decode_text(p, unicode))
                    }
                    ID_CHANNEL_COLOR => c.color = Some((decode_color(p), ev.data)),
                    _ => {}
                }
            }
            if let Some(pt) = cur_pat.and_then(|c| patterns.get_mut(&c)) {
                match ev.id {
                    ID_PATTERN_NAME => pt.name = non_empty(decode_text(p, unicode)),
                    ID_PATTERN_COLOR => pt.color = Some((decode_color(p), ev.data)),
                    _ => {}
                }
            }
            if let Some(arr) = arrangements.last_mut() {
                match ev.id {
                    ID_ARRANGEMENT_NAME => arr.name = decode_text(p, unicode),
                    ID_PLAYLIST => {
                        *playlists.last_mut().unwrap() = Some((ev.data, ev.len));
                        arr.has_playlist = true;
                    }
                    ID_TRACK_DATA => {
                        track_data_sizes.push(ev.len);
                        if ev.len > TRACK_MUTE_OFFSET {
                            arr.tracks.push(Track {
                                mute_off: ev.data + TRACK_MUTE_OFFSET,
                                muted: d[ev.data + TRACK_MUTE_OFFSET] == 0,
                                data_end: ev.end(),
                                has_name: false,
                            });
                        }
                    }
                    ID_TRACK_NAME => {
                        if let Some(t) = arr.tracks.last_mut() {
                            t.has_name = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        // Clip della playlist
        let mut clips = Vec::new();
        for (ai, arr) in arrangements.iter_mut().enumerate() {
            let Some((off, size)) = playlists[ai] else {
                continue;
            };
            let Some(isz) = detect_item_size(d, off, size) else {
                continue;
            };
            arr.item_size = isz;
            for k in 0..size / isz {
                let b = off + k * isz;
                let position = u32_at(d, b);
                let pattern_base = u16_at(d, b + 4);
                let item = u16_at(d, b + 6);
                let length = u32_at(d, b + 8);
                let rvidx = u16_at(d, b + 12) as usize;
                // Le clip vere hanno byte fissi in 16-17 e 20-23
                let real = pattern_base == PATTERN_BASE
                    && rvidx < MAX_TRACKS
                    && length <= 10_000_000
                    && isz >= 24
                    && d[b + 16..b + 18] == [0x78, 0x00]
                    && d[b + 20..b + 24] == [0x40, 0x64, 0x80, 0x80];
                if !real {
                    arr.skipped_items += 1;
                    continue;
                }
                let track = MAX_TRACKS - 1 - rvidx;
                let (name, kind, source, color) = if item > PATTERN_BASE {
                    let n = item - PATTERN_BASE;
                    let pt = patterns.get(&n);
                    let name = pt
                        .and_then(|x| x.name.clone())
                        .unwrap_or_else(|| format!("Pattern {n}"));
                    (
                        name,
                        ClipKind::Pattern,
                        Source::Pattern(n),
                        pt.and_then(|x| x.color.map(|c| c.0)),
                    )
                } else {
                    let ch = channels.get(&item);
                    let name = ch
                        .and_then(channel_label)
                        .unwrap_or_else(|| format!("Channel {item}"));
                    let kind = if ch.and_then(|c| c.kind) == Some(5) {
                        ClipKind::Automation
                    } else {
                        ClipKind::Audio
                    };
                    (
                        name,
                        kind,
                        Source::Channel(item),
                        ch.and_then(|x| x.color.map(|c| c.0)),
                    )
                };
                clips.push(Clip {
                    arrangement: ai,
                    name,
                    kind,
                    source,
                    position,
                    length,
                    track,
                    track_muted: arr.tracks.get(track).is_some_and(|t| t.muted),
                    clip_muted: d[b + CLIP_FLAGS_OFFSET] & CLIP_MUTED_BIT != 0,
                    color,
                    track_off: b + 12,
                });
            }
        }

        Ok(Project {
            hash,
            version: parsed.version,
            major: parsed.major,
            fldt_pos: parsed.fldt_pos,
            trailing_bytes: bytes.len() - parsed.events_end,
            channels,
            patterns,
            arrangements,
            clips,
            track_data_sizes,
            bytes,
        })
    }

    pub fn tracks_used(&self) -> usize {
        self.clips.iter().map(|c| c.track + 1).max().unwrap_or(0)
    }
}

/// Nome con cui raggruppare le clip di un canale.
fn channel_label(c: &Channel) -> Option<String> {
    c.name
        .clone()
        .or_else(|| c.display_name.clone())
        .or_else(|| {
            c.sample
                .as_ref()
                .map(|s| s.rsplit(['\\', '/']).next().unwrap_or(s).to_string())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| c.default_name.clone())
}
