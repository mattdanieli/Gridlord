//! Test del motore: un piccolo progetto costruito qui e, se presenti, progetti veri
//! confrontati byte per byte con i risultati della versione precedente (cartella in GRIDLORD_FIXTURES).

use super::check::{self, Issue};
use super::flp::{self, write_varint};
use super::organize::{self, Options, SortMode, SubSort};
use super::project::{ClipKind, Project};
use super::write;

struct Item {
    pos: u32,
    item: u16,
    len: u32,
    track: usize,
    muted: bool,
}

fn ev_word(out: &mut Vec<u8>, id: u8, v: u16) {
    out.push(id);
    out.extend_from_slice(&v.to_le_bytes());
}

fn ev_text(out: &mut Vec<u8>, id: u8, s: &str) {
    out.extend(flp::text_event(id, s));
}

fn ev_data(out: &mut Vec<u8>, id: u8, d: &[u8]) {
    out.push(id);
    write_varint(out, d.len());
    out.extend_from_slice(d);
}

/// Progetto FL 2026 minimo: 3 canali audio, 1 automazione, 2 pattern, 500 tracce.
fn build(items: &[Item], muted_tracks: &[usize]) -> Vec<u8> {
    build_with(
        &[(1, "Pads"), (2, "Bass")],
        &[
            (0, "Kick", 4, [0x39, 0x35, 0x2f, 0]),
            (1, "Snare", 4, [0xff, 0x00, 0x00, 0]),
            (2, "Hat", 4, [0x20, 0x20, 0x20, 0]),
            (3, "Cutoff", 5, [0x30, 0x30, 0x30, 0]),
        ],
        items,
        muted_tracks,
    )
}

type ChannelSpec<'a> = (u16, &'a str, u8, [u8; 4]);

fn build_with(
    patterns: &[(u16, &str)],
    channels: &[ChannelSpec],
    items: &[Item],
    muted_tracks: &[usize],
) -> Vec<u8> {
    let mut e = Vec::new();
    ev_data(&mut e, 199, b"26.1.1.5547\0");
    e.extend_from_slice(&[172, 1, 2, 3, 4, 5]); // evento a 5 byte di FL 2026
    for &(id, name) in patterns {
        ev_word(&mut e, 65, id);
        ev_text(&mut e, 193, name);
    }
    for &(id, name, kind, color) in channels {
        ev_word(&mut e, 64, id);
        e.extend_from_slice(&[21, kind]);
        ev_text(&mut e, 203, name);
        e.push(128);
        e.extend_from_slice(&color);
    }
    ev_word(&mut e, 99, 0);
    ev_text(&mut e, 241, "Arrangement");
    let mut pl = Vec::new();
    for it in items {
        let mut b = [0u8; 88];
        b[0..4].copy_from_slice(&it.pos.to_le_bytes());
        b[4..6].copy_from_slice(&20480u16.to_le_bytes());
        b[6..8].copy_from_slice(&it.item.to_le_bytes());
        b[8..12].copy_from_slice(&it.len.to_le_bytes());
        b[12..14].copy_from_slice(&((499 - it.track) as u16).to_le_bytes());
        b[16] = 0x78;
        b[19] = if it.muted { 0x20 } else { 0 };
        b[20..24].copy_from_slice(&[0x40, 0x64, 0x80, 0x80]);
        pl.extend_from_slice(&b);
    }
    ev_data(&mut e, 233, &pl);
    for t in 0..500 {
        let mut d = [0u8; 70];
        d[12] = if muted_tracks.contains(&t) { 0 } else { 1 };
        ev_data(&mut e, 238, &d);
        if t == 7 && channels.len() == 4 {
            ev_text(&mut e, 239, "My track");
        }
    }
    let mut out = b"FLhd".to_vec();
    out.extend_from_slice(&6u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 4, 0, 96, 0]);
    out.extend_from_slice(b"FLdt");
    out.extend_from_slice(&(e.len() as u32).to_le_bytes());
    out.extend(e);
    out
}

fn sample() -> Vec<u8> {
    build(&sample_items(), &[12])
}

fn sample_items() -> Vec<Item> {
    let it = |pos, item, len, track, muted| Item {
        pos,
        item,
        len,
        track,
        muted,
    };
    vec![
        it(0, 1, 96, 5, false),      // Snare
        it(0, 0, 96, 2, false),      // Kick
        it(96, 0, 96, 9, false),     // Kick
        it(48, 0, 96, 3, false),     // Kick, si sovrappone: seconda corsia
        it(0, 20481, 384, 0, false), // Pads
        it(0, 20482, 384, 7, true),  // Bass, clip silenziata
        it(0, 2, 96, 12, false),     // Hat su traccia silenziata
        it(0, 3, 384, 20, false),    // automazione
    ]
}

#[test]
fn legge_il_progetto() {
    let p = Project::from_bytes(sample()).unwrap();
    assert_eq!(p.version, "26.1.1.5547");
    assert_eq!(p.major, 26);
    assert_eq!(p.clips.len(), 8);
    assert_eq!(p.arrangements[0].tracks.len(), 500);
    assert_eq!(p.arrangements[0].item_size, 88);
    assert_eq!(p.clips[0].name, "Snare");
    assert_eq!(p.clips[4].name, "Pads");
    assert_eq!(p.clips[7].kind, ClipKind::Automation);
    assert!(p.clips[5].clip_muted);
    assert!(p.clips[6].track_muted);
    assert!(check::check(&p).is_empty(), "{:?}", check::check(&p));
}

#[test]
fn raggruppa_in_ordine_alfabetico() {
    let p = Project::from_bytes(sample()).unwrap();
    let plan = organize::plan(&p, &Options::default());
    let names: Vec<_> = plan
        .groups
        .iter()
        .map(|g| (g.name.as_str(), g.first_track, g.lanes, g.muted))
        .collect();
    assert_eq!(
        names,
        [
            ("Bass", 0, 1, false),
            ("Cutoff", 1, 1, false),
            ("Kick", 2, 2, false),
            ("Pads", 4, 1, false),
            ("Snare", 5, 1, false),
            ("Hat", 6, 1, true),
        ]
    );
    // Kick: 0-96 corsia 1, 48-144 corsia 2, 96-192 di nuovo corsia 1
    assert_eq!(plan.new_track[1], 2);
    assert_eq!(plan.new_track[3], 3);
    assert_eq!(plan.new_track[2], 2);
    assert_eq!(plan.tracks_needed, 7);
}

#[test]
fn altri_ordinamenti() {
    let p = Project::from_bytes(sample()).unwrap();
    let o = Options {
        sort: SortMode::FirstAppearance,
        then_by: vec![SubSort::Type],
        ..Default::default()
    };
    let plan = organize::plan(&p, &o);
    let names: Vec<_> = plan.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Snare", "Kick", "Pads", "Bass", "Cutoff", "Hat"]);
    let o = Options {
        then_by: vec![SubSort::Length],
        ..Default::default()
    };
    let names: Vec<_> = organize::plan(&p, &o)
        .groups
        .iter()
        .map(|g| g.name.clone())
        .collect();
    assert_eq!(names, ["Bass", "Cutoff", "Pads", "Kick", "Snare", "Hat"]);
}

#[test]
fn scrive_e_rilegge() {
    let p = Project::from_bytes(sample()).unwrap();
    let o = Options {
        rename_tracks: true,
        color_clips: true,
        ..Default::default()
    };
    let plan = organize::plan(&p, &o);
    let out = write::apply(&p, &plan);
    write::verify(&p, &plan, &out).unwrap();
    let re = Project::from_bytes(out).unwrap();
    for (i, c) in re.clips.iter().enumerate() {
        assert_eq!(c.track, plan.new_track[i]);
        assert_eq!(c.position, p.clips[i].position);
        assert_eq!(c.clip_muted, p.clips[i].clip_muted);
    }
    // 7 tracce usate, tutte senza nome: ricevono il nome del gruppo
    assert_eq!(plan.renamed_tracks, 7);
    let arr = &re.arrangements[0];
    assert!(arr.tracks[..7].iter().all(|t| t.has_name));
    // con piu gruppi la traccia 7 (che ha gia un nome) non ne riceve un secondo
    let mut many = sample_items();
    many.push(Item {
        pos: 0,
        item: 1,
        len: 96,
        track: 40,
        muted: false,
    });
    many.push(Item {
        pos: 50,
        item: 1,
        len: 96,
        track: 41,
        muted: false,
    });
    let p2 = Project::from_bytes(build(&many, &[12])).unwrap();
    let plan2 = organize::plan(&p2, &o);
    assert_eq!(plan2.tracks_needed, 9);
    assert_eq!(plan2.renamed_tracks, 8);
    write::verify(&p2, &plan2, &write::apply(&p2, &plan2)).unwrap();
    // traccia 6 (gruppo silenziato) silenziata, la vecchia traccia 12 riattivata
    assert!(arr.tracks[6].muted);
    assert!(!arr.tracks[12].muted);
    // Snare e rosso scelto a mano: resta rosso. Kick era grigio: ora ha un colore.
    assert_eq!(re.channels[&1].color.unwrap().0, 0xff0000);
    assert_ne!(re.channels[&0].color.unwrap().0, 0x39352f);
    // i pattern non avevano colore: ne ricevono uno nuovo
    assert!(re.patterns[&1].color.is_some());
}

#[test]
fn salva_senza_toccare_l_originale() {
    let dir = std::env::temp_dir().join(format!("gridlord-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("Song.flp");
    let bytes = sample();
    std::fs::write(&src, &bytes).unwrap();
    let p = super::project::load(&src).unwrap();
    let plan = organize::plan(&p, &Options::default());
    let saved = write::save(&src, &p, &plan, &write::Target::NewFile(None)).unwrap();
    assert_eq!(saved.path, dir.join("Song_organized.flp"));
    assert_eq!(std::fs::read(&src).unwrap(), bytes);
    // la seconda volta non sovrascrive
    let again = write::save(&src, &p, &plan, &write::Target::NewFile(None)).unwrap();
    assert_eq!(again.path, dir.join("Song_organized (2).flp"));
    // sostituzione dell'originale con copia di sicurezza
    let r = write::save(&src, &p, &plan, &write::Target::ReplaceOriginal).unwrap();
    assert_eq!(std::fs::read(r.backup.unwrap()).unwrap(), bytes);
    assert_ne!(std::fs::read(&src).unwrap(), bytes);
    // il file e cambiato dopo il caricamento: si ferma
    assert!(matches!(
        write::save(&src, &p, &plan, &write::Target::NewFile(None)),
        Err(write::SaveError::ChangedOnDisk)
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rifiuta_i_file_sbagliati() {
    assert!(Project::from_bytes(b"hello world, not a project".to_vec()).is_err());
    let mut b = sample();
    let n = b.len();
    b.truncate(n - 10);
    assert!(Project::from_bytes(b).is_err());
    // un byte in piu nell'evento a 5 byte: deve accorgersene, non leggere a caso
    let mut b = sample();
    let size = u32::from_le_bytes([b[18], b[19], b[20], b[21]]) + 1;
    b[18..22].copy_from_slice(&size.to_le_bytes());
    b.push(0);
    let p = Project::from_bytes(b);
    assert!(
        p.is_err()
            || p.as_ref()
                .is_ok_and(|p| !check::check(p).is_empty() || p.clips.len() == 8)
    );
}

#[test]
fn versioni_non_provate() {
    let mut b = sample();
    let i = b.windows(11).position(|w| w == b"26.1.1.5547").unwrap();
    b[i..i + 2].copy_from_slice(b"27");
    let p = Project::from_bytes(b).unwrap();
    assert!(
        check::check(&p)
            .iter()
            .any(|x| matches!(x, Issue::UntestedVersion(_)))
    );
}

/// Confronto byte per byte con la versione precedente su progetti veri (non inclusi nel repository).
#[test]
fn uguale_alla_versione_precedente() {
    let Ok(dir) = std::env::var("GRIDLORD_FIXTURES") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let combos: [(&str, Options); 6] = [
        ("a", Options::default()),
        (
            "f",
            Options {
                sort: SortMode::FirstAppearance,
                ..Default::default()
            },
        ),
        (
            "atl",
            Options {
                then_by: vec![SubSort::Type, SubSort::Length],
                ..Default::default()
            },
        ),
        (
            "fc",
            Options {
                sort: SortMode::FirstAppearance,
                then_by: vec![SubSort::Color],
                ..Default::default()
            },
        ),
        (
            "acr",
            Options {
                color_clips: true,
                rename_tracks: true,
                ..Default::default()
            },
        ),
        (
            "ftlcCR",
            Options {
                sort: SortMode::FirstAppearance,
                then_by: vec![SubSort::Type, SubSort::Length, SubSort::Color],
                color_clips: true,
                rename_tracks: true,
            },
        ),
    ];
    let mut checked = 0;
    for f in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = f.path();
        if path.extension().is_none_or(|e| e != "flp") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let p = super::project::load(&path).unwrap();
        for (k, o) in &combos {
            let reference = dir.join("ref").join(format!("{stem}.{k}.flp"));
            let Ok(expected) = std::fs::read(&reference) else {
                continue;
            };
            let plan = organize::plan(&p, o);
            let out = write::apply(&p, &plan);
            write::verify(&p, &plan, &out).unwrap();
            assert!(
                out == expected,
                "{stem} {k}: diverso dalla versione precedente"
            );
            checked += 1;
        }
    }
    assert!(checked > 0);
    eprintln!("{checked} confronti identici");
}

/// Progetto dimostrativo per lo screenshot del README: GRIDLORD_DEMO=percorso cargo test demo -- --ignored
#[test]
#[ignore]
fn demo() {
    let Ok(out) = std::env::var("GRIDLORD_DEMO") else {
        return;
    };
    let grey = [0x3c, 0x40, 0x43, 0];
    let patterns = [
        (1u16, "Chords"),
        (2, "Bassline"),
        (3, "Lead Melody"),
        (4, "Arp"),
        (5, "Drum Fill"),
    ];
    let channels: [ChannelSpec; 9] = [
        (0, "Kick", 4, grey),
        (1, "Snare", 4, grey),
        (2, "Hi-Hat Loop", 4, grey),
        (3, "Clap", 4, grey),
        (4, "Vocal Chop", 4, grey),
        (5, "Riser FX", 4, grey),
        (6, "Crash", 4, grey),
        (7, "Filter Cutoff", 5, grey),
        (8, "Master Volume", 5, grey),
    ];
    let bar = 384u32;
    let mut items = Vec::new();
    let mut seed = 7u32;
    let mut rnd = |n: u32| {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        (seed >> 16) % n
    };
    let put =
        |pos: u32, item: u16, len: u32, items: &mut Vec<Item>, r: &mut dyn FnMut(u32) -> u32| {
            items.push(Item {
                pos,
                item,
                len,
                track: r(26) as usize,
                muted: false,
            });
        };
    for b in (0..64).step_by(4) {
        put(b * bar, 20481, 4 * bar, &mut items, &mut rnd);
        if b >= 8 {
            put(b * bar, 20482, 4 * bar, &mut items, &mut rnd);
        }
        if b >= 16 && b < 48 {
            put(b * bar, 20483, 4 * bar, &mut items, &mut rnd);
        }
        if (24..56).contains(&b) {
            put(b * bar, 20484, 4 * bar, &mut items, &mut rnd);
        }
    }
    for b in 8..60 {
        put(b * bar, 0, bar, &mut items, &mut rnd);
        if b % 2 == 0 {
            put(b * bar, 1, bar, &mut items, &mut rnd);
            put(b * bar, 2, 2 * bar, &mut items, &mut rnd);
        }
        if b % 4 == 3 {
            put(b * bar, 3, bar, &mut items, &mut rnd);
        }
    }
    for b in [15u32, 31, 47] {
        put(b * bar, 20485, bar, &mut items, &mut rnd);
        put(b * bar, 5, bar, &mut items, &mut rnd);
        put((b + 1) * bar, 6, 2 * bar, &mut items, &mut rnd);
    }
    for b in (16..48).step_by(8) {
        put(b * bar, 4, 2 * bar, &mut items, &mut rnd);
    }
    put(0, 7, 16 * bar, &mut items, &mut rnd);
    put(40 * bar, 7, 8 * bar, &mut items, &mut rnd);
    put(56 * bar, 8, 8 * bar, &mut items, &mut rnd);
    std::fs::write(out, build_with(&patterns, &channels, &items, &[])).unwrap();
}
