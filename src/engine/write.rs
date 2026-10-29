//! Scrittura del progetto riorganizzato e controllo del risultato.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::flp::{FlpError, u32_at};
use super::organize::Plan;
use super::project::{MAX_TRACKS, Project};

/// Applica il piano ai byte del progetto. L'originale non viene toccato.
pub fn apply(p: &Project, plan: &Plan) -> Vec<u8> {
    let mut out = p.bytes.clone();
    for &(off, v) in &plan.byte_patches {
        out[off] = v;
    }
    if !plan.inserts.is_empty() {
        // dal fondo, cosi le posizioni precedenti restano valide
        let mut ins: Vec<&(usize, Vec<u8>)> = plan.inserts.iter().collect();
        ins.sort_by(|a, b| b.0.cmp(&a.0));
        let added: usize = ins.iter().map(|x| x.1.len()).sum();
        for (at, bytes) in ins {
            out.splice(*at..*at, bytes.iter().copied());
        }
        let size_pos = p.fldt_pos + 4;
        let size = u32_at(&out, size_pos) as usize + added;
        out[size_pos..size_pos + 4].copy_from_slice(&(size as u32).to_le_bytes());
    }
    out
}

/// Rilegge il risultato e controlla che sia quello previsto: stesse clip, stesse
/// posizioni e lunghezze, solo la traccia cambiata.
pub fn verify(original: &Project, plan: &Plan, out: &[u8]) -> Result<(), FlpError> {
    let fail = |m: &str| Err(FlpError::VerifyFailed(m.to_string()));
    let re = Project::from_bytes(out.to_vec())?;
    if re.version != original.version || re.arrangements.len() != original.arrangements.len() {
        return fail("project structure changed");
    }
    if re.clips.len() != original.clips.len() {
        return fail("clip count changed");
    }
    for (i, (a, b)) in original.clips.iter().zip(&re.clips).enumerate() {
        if a.position != b.position
            || a.length != b.length
            || a.source != b.source
            || a.clip_muted != b.clip_muted
        {
            return fail("a clip changed");
        }
        if b.track != plan.new_track[i] || b.track >= MAX_TRACKS {
            return fail("a clip is not on the expected track");
        }
    }
    for (ta, tb) in original.arrangements.iter().zip(&re.arrangements) {
        if ta.tracks.len() != tb.tracks.len() {
            return fail("track count changed");
        }
    }
    Ok(())
}

/// Nome del file di uscita: "Progetto_organized.flp" accanto all'originale (o nella cartella
/// scelta). Non sovrascrive mai un file esistente: aggiunge (2), (3)...
pub fn output_path(src: &Path, folder: Option<&Path>) -> PathBuf {
    let dir = folder
        .map(Path::to_path_buf)
        .or_else(|| src.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    let first = dir.join(format!("{stem}_organized.flp"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem}_organized ({n}).flp")))
        .find(|p| !p.exists())
        .unwrap()
}

/// Copia di sicurezza accanto all'originale: "Progetto.flp.backup_20261029_134501".
pub fn backup_path(src: &Path) -> PathBuf {
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let name = src
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let base = src.with_file_name(format!("{name}.backup_{ts}"));
    if !base.exists() {
        return base;
    }
    (2..)
        .map(|n| src.with_file_name(format!("{name}.backup_{ts}_{n}")))
        .find(|p| !p.exists())
        .unwrap()
}

/// Scrive in un file temporaneo, lo rilegge, controlla l'impronta e solo allora lo mette al suo posto.
pub fn write_verified(dest: &Path, bytes: &[u8]) -> Result<(), FlpError> {
    let io = |e: std::io::Error| FlpError::Io(e.to_string());
    let dir = dest
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.gridlord-tmp",
        dest.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let result = (|| {
        let mut f = fs::File::create(&tmp).map_err(io)?;
        f.write_all(bytes).map_err(io)?;
        f.sync_all().map_err(io)?;
        drop(f);
        let back = fs::read(&tmp).map_err(io)?;
        if blake3::hash(&back) != blake3::hash(bytes) {
            return Err(FlpError::VerifyFailed(
                "the saved file does not match".into(),
            ));
        }
        if dest.exists() {
            let mut perm = fs::metadata(dest).map_err(io)?.permissions();
            if perm.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perm.set_readonly(false);
                fs::set_permissions(dest, perm).map_err(io)?;
            }
        }
        fs::rename(&tmp, dest).map_err(io)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub enum Target {
    /// Nuovo file accanto all'originale o nella cartella indicata.
    NewFile(Option<PathBuf>),
    /// Sostituisce l'originale, dopo averne fatto una copia di sicurezza.
    ReplaceOriginal,
}

pub struct Saved {
    pub path: PathBuf,
    pub backup: Option<PathBuf>,
}

/// Salva il progetto riorganizzato. Se il file sul disco e cambiato dal caricamento
/// (per esempio salvato di nuovo da FL Studio) si ferma: va ricaricato.
pub fn save(src: &Path, p: &Project, plan: &Plan, target: &Target) -> Result<Saved, SaveError> {
    let now = fs::read(src).map_err(|e| SaveError::Flp(FlpError::Io(e.to_string())))?;
    if blake3::hash(&now) != p.hash {
        return Err(SaveError::ChangedOnDisk);
    }
    let out = apply(p, plan);
    verify(p, plan, &out).map_err(SaveError::Flp)?;
    match target {
        Target::NewFile(folder) => {
            let path = output_path(src, folder.as_deref());
            write_verified(&path, &out).map_err(SaveError::Flp)?;
            Ok(Saved { path, backup: None })
        }
        Target::ReplaceOriginal => {
            let backup = backup_path(src);
            write_verified(&backup, &p.bytes).map_err(SaveError::Flp)?;
            write_verified(src, &out).map_err(SaveError::Flp)?;
            Ok(Saved {
                path: src.to_path_buf(),
                backup: Some(backup),
            })
        }
    }
}

#[derive(Debug)]
pub enum SaveError {
    ChangedOnDisk,
    Flp(FlpError),
}
