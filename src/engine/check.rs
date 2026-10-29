//! Controllo di compatibilita: cosa c'e di insolito in un progetto, prima di toccarlo.

use super::project::{MAX_TRACKS, Project};

/// Versioni di FL Studio provate su progetti veri.
pub const TESTED_MAJORS: [u32; 3] = [24, 25, 26];
/// Dimensioni dei dati di traccia conosciute (66 = FL 24, 70 = FL 25 e 2026).
const KNOWN_TRACK_DATA: [usize; 2] = [66, 70];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Ok,
    Note,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Issue {
    /// Versione mai provata (piu nuova o piu vecchia).
    UntestedVersion(String),
    /// Progetto salvato da una versione precedente a FL Studio 20.7 (niente arrangement).
    TooOld(String),
    NoPlaylist,
    NoClips,
    UnknownItemSize,
    UnusualTrackData,
    TrackCount(usize),
    TrailingData,
    MultipleArrangements(usize),
}

impl Issue {
    pub fn level(&self) -> Level {
        match self {
            Issue::MultipleArrangements(_) | Issue::NoClips | Issue::TrailingData => Level::Note,
            _ => Level::Warning,
        }
    }
}

pub fn check(p: &Project) -> Vec<Issue> {
    let mut out = Vec::new();
    if p.major > 0 && p.major < 20 {
        out.push(Issue::TooOld(p.version.clone()));
    } else if !TESTED_MAJORS.contains(&p.major) {
        out.push(Issue::UntestedVersion(p.version.clone()));
    }
    if !p.arrangements.iter().any(|a| a.has_playlist) {
        out.push(Issue::NoPlaylist);
    } else {
        if p.arrangements
            .iter()
            .any(|a| a.has_playlist && a.item_size == 0)
        {
            out.push(Issue::UnknownItemSize);
        }
        if p.clips.is_empty() {
            out.push(Issue::NoClips);
        }
    }
    if p.track_data_sizes
        .iter()
        .any(|s| !KNOWN_TRACK_DATA.contains(s))
    {
        out.push(Issue::UnusualTrackData);
    }
    if let Some(a) = p
        .arrangements
        .iter()
        .find(|a| !a.tracks.is_empty() && a.tracks.len() != MAX_TRACKS)
    {
        out.push(Issue::TrackCount(a.tracks.len()));
    }
    if p.trailing_bytes > 0 {
        out.push(Issue::TrailingData);
    }
    if p.arrangements.len() > 1 {
        out.push(Issue::MultipleArrangements(p.arrangements.len()));
    }
    out
}

pub fn level(issues: &[Issue]) -> Level {
    issues.iter().map(Issue::level).max().unwrap_or(Level::Ok)
}
