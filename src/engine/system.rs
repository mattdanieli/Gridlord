//! Cose legate al sistema: FL Studio aperto, Esplora file / Finder.

use std::path::Path;
use std::process::Command;

#[cfg(windows)]
fn hidden(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW)
}

#[cfg(not(windows))]
fn hidden(cmd: &mut Command) -> &mut Command {
    cmd
}

/// True se FL Studio e in esecuzione (Windows e macOS).
pub fn fl_studio_running() -> bool {
    if cfg!(windows) {
        let Ok(out) = hidden(Command::new("tasklist").args(["/FO", "CSV", "/NH"])).output() else {
            return false;
        };
        let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
        ["\"fl64.exe\"", "\"fl.exe\"", "\"fl studio"]
            .iter()
            .any(|n| text.contains(n))
    } else if cfg!(target_os = "macos") {
        let Ok(out) = Command::new("ps").args(["-axo", "comm="]).output() else {
            return false;
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|l| l.to_lowercase().contains("/fl studio") && l.contains("/Contents/MacOS/"))
    } else {
        false
    }
}

/// Mostra il file in Esplora file / Finder (o la cartella, se il file non c'e).
pub fn reveal(path: &str) {
    let p = Path::new(path);
    if cfg!(windows) {
        if p.exists() {
            let _ = hidden(&mut Command::new("explorer"))
                .arg(format!("/select,{path}"))
                .spawn();
        } else if let Some(parent) = p.parent().filter(|d| d.exists()) {
            let _ = hidden(&mut Command::new("explorer")).arg(parent).spawn();
        }
    } else if cfg!(target_os = "macos") {
        if p.exists() {
            let _ = Command::new("open").arg("-R").arg(p).spawn();
        } else if let Some(parent) = p.parent().filter(|d| d.exists()) {
            let _ = Command::new("open").arg(parent).spawn();
        }
    } else if let Some(parent) = p.parent() {
        let _ = Command::new("xdg-open").arg(parent).spawn();
    }
}
