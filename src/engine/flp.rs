//! Lettura a basso livello di un file .flp: intestazione e lista degli eventi.
//!
//! Un .flp e fatto cosi:
//!   "FLhd" + u32 dimensione (6) + formato u16 + canali u16 + PPQ u16
//!   "FLdt" + u32 dimensione + eventi
//! Ogni evento e: 1 byte id + dati. La dimensione dei dati dipende dall'id:
//!   0..63 -> 1 byte, 64..127 -> 2 byte, 128..191 -> 4 byte, 192..255 -> lunghezza varint + dati.
//! Eccezione: FL Studio 2026 scrive l'evento 172 con 5 byte invece di 4.

use std::fmt;

pub const WORD: u8 = 64;
pub const DWORD: u8 = 128;
pub const TEXT: u8 = 192;

/// FL Studio 2026 (26.x) scrive l'evento 172 con 5 byte. Leggerlo con 4 sposta di un
/// byte tutto quello che segue. Vale solo per i file salvati da FL 26 o successivi.
pub const ID_FL26_FIVE_BYTE: u8 = 172;
pub const ID_FL_VERSION: u8 = TEXT + 7; // 199

#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub id: u8,
    /// Posizione del primo byte dei dati.
    pub data: usize,
    pub len: usize,
}

impl Event {
    /// Il primo byte dopo l'evento.
    pub fn end(&self) -> usize {
        self.data + self.len
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlpError {
    Io(String),
    NotFlp,
    /// La dimensione dichiarata dei dati supera la fine del file.
    Truncated,
    /// Un evento esce dai confini dei dati (file danneggiato o formato sconosciuto).
    Corrupt {
        offset: usize,
    },
    /// Lo stesso controllo dopo la scrittura non e andato a buon fine.
    VerifyFailed(String),
}

impl fmt::Display for FlpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FlpError::Io(e) => write!(f, "{e}"),
            FlpError::NotFlp => write!(f, "not an FL Studio project"),
            FlpError::Truncated => write!(f, "the file is incomplete"),
            FlpError::Corrupt { offset } => write!(f, "unreadable data at byte {offset}"),
            FlpError::VerifyFailed(e) => write!(f, "verification failed: {e}"),
        }
    }
}

pub struct Parsed {
    /// Posizione di "FLdt".
    pub fldt_pos: usize,
    /// Fine degli eventi (di solito = lunghezza del file).
    pub events_end: usize,
    pub events: Vec<Event>,
    pub version: String,
    pub major: u32,
    pub minor: u32,
}

pub fn read_varint(data: &[u8], mut pos: usize, end: usize) -> Option<(usize, usize)> {
    let mut v: usize = 0;
    let mut shift = 0;
    loop {
        if pos >= end || shift > 35 {
            return None;
        }
        let b = data[pos];
        pos += 1;
        v |= ((b & 0x7f) as usize) << shift;
        if b & 0x80 == 0 {
            return Some((v, pos));
        }
        shift += 7;
    }
}

pub fn write_varint(out: &mut Vec<u8>, mut n: usize) {
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

pub fn u16_at(d: &[u8], p: usize) -> u16 {
    u16::from_le_bytes([d[p], d[p + 1]])
}

pub fn u32_at(d: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]])
}

/// Legge l'intestazione e l'elenco degli eventi.
pub fn parse(data: &[u8]) -> Result<Parsed, FlpError> {
    if data.len() < 22 || &data[0..4] != b"FLhd" {
        return Err(FlpError::NotFlp);
    }
    let hdr = u32_at(data, 4) as usize;
    let fldt_pos = 8usize.checked_add(hdr).ok_or(FlpError::NotFlp)?;
    if hdr < 6 || fldt_pos + 8 > data.len() || &data[fldt_pos..fldt_pos + 4] != b"FLdt" {
        return Err(FlpError::NotFlp);
    }
    let size = u32_at(data, fldt_pos + 4) as usize;
    let start = fldt_pos + 8;
    let end = start + size;
    if end > data.len() {
        return Err(FlpError::Truncated);
    }

    let mut events = Vec::with_capacity(size / 8);
    let mut pos = start;
    let (mut version, mut major, mut minor) = (String::new(), 0u32, 0u32);
    while pos < end {
        let ev_start = pos;
        let id = data[pos];
        pos += 1;
        let (dpos, len) = if id < WORD {
            (pos, 1)
        } else if id < DWORD {
            (pos, 2)
        } else if id == ID_FL26_FIVE_BYTE && major >= 26 {
            (pos, 5)
        } else if id < TEXT {
            (pos, 4)
        } else {
            let (l, p) =
                read_varint(data, pos, end).ok_or(FlpError::Corrupt { offset: ev_start })?;
            (p, l)
        };
        if dpos + len > end {
            return Err(FlpError::Corrupt { offset: ev_start });
        }
        pos = dpos + len;
        if id == ID_FL_VERSION {
            let s: String = data[dpos..pos]
                .iter()
                .take_while(|&&b| b != 0)
                .map(|&b| b as char)
                .collect();
            let mut parts = s.split('.');
            major = parts
                .next()
                .and_then(|x| x.trim().parse().ok())
                .unwrap_or(0);
            minor = parts
                .next()
                .and_then(|x| x.trim().parse().ok())
                .unwrap_or(0);
            version = s;
        }
        events.push(Event {
            id,
            data: dpos,
            len,
        });
    }
    Ok(Parsed {
        fldt_pos,
        events_end: end,
        events,
        version,
        major,
        minor,
    })
}

/// Testo di un evento: UTF-16 per i progetti moderni, Latin-1 per quelli molto vecchi.
pub fn decode_text(raw: &[u8], unicode: bool) -> String {
    let s = if unicode {
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        raw.iter().map(|&b| b as char).collect()
    };
    s.trim_end_matches('\0').to_string()
}

/// Evento di testo UTF-16 con terminatore, pronto da inserire nel file.
pub fn text_event(id: u8, text: &str) -> Vec<u8> {
    let mut payload: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    payload.extend_from_slice(&[0, 0]);
    let mut out = vec![id];
    write_varint(&mut out, payload.len());
    out.extend_from_slice(&payload);
    out
}
