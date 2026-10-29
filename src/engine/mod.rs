//! Motore: legge il progetto .flp, calcola il nuovo ordine della playlist e lo salva.

/// Nome usato per le impostazioni.
pub const APP_ID: &str = "Gridlord";

pub mod check;
pub mod flp;
pub mod organize;
pub mod project;
pub mod system;
pub mod write;

#[cfg(test)]
mod tests;
