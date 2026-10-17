# FLP Organizer

> Automatically organize the FL Studio playlist by grouping every clip by name onto adjacent tracks — without moving a single note in time.

FL Studio has no built-in way to say "take every clip called *Kick*, put them all on the same track; every clip called *Snare*, on the next track; every automation called *Reverb Send*, on the next one again." You can `Select source channel`, but `Shift+drag` only moves clips in parallel — it doesn't collapse them onto a single lane. **FLP Organizer does exactly that**, directly on the `.flp` file.

It works by patching **only the track-index bytes** of each playlist item. Every other byte of your project stays identical to the original: positions, lengths, colors, automations, patterns, mixer routing, plugin state, everything.

---

## Features

- **Drag & drop** any `.flp` file onto the window
- **Two sorting modes** selectable from the UI:
  - **Alphabetical (A–Z)** — best for finding a specific sample or pattern quickly
  - **By first appearance** — orders groups by the earliest time any clip plays, so the arrangement reads top-to-bottom like a song timeline
- **Sub-sort modifiers** (combine freely with the two primary modes):
  - *Group by type*: audio clips come before patterns
  - *Sort by clip length*: groups with longer clips first
- **Batch mode** — process up to 30 `.flp` files in a single run, with optional custom output folder
- **Multi-language UI** — English, Deutsch, Español, Français, Italiano, Русский
- **Preview** the grouping plan before committing (how many tracks, which clips go where)
- **One-click apply** — writes a new `.flp`, never touches your original
- Groups by **sample file name**, **channel name**, or **pattern name** (automatically picks the right one for each clip)
- **Lane assignment** — when clips of the same group overlap in time, they go on adjacent tracks instead of on top of each other
- Preserves every clip's **position, length, color, offsets, and every other property**
- **Safe by design** — output is always a different file; original is read-only during processing
- **Zero config, zero external dependencies at runtime** (standalone Windows `.exe`)

## Tested with

- FL Studio **25.1.6** (64-bit) — primary target
- FL Studio **21.x** — should work (playlist item size auto-detected)

If you use an older version and you hit an issue, please open an issue with your FL version and a redacted project that reproduces the problem.

---

## Download

**Windows users:** download `FLPOrganizer.exe` from the [latest release](../../releases). No installation needed — double-click to run.

**Other platforms / run from source:** see [Building from source](#building-from-source).

---

## How to use

1. **Save your project in FL Studio.** Close FL or switch to a different project — don't keep the same `.flp` open while you reorganize it.
2. **Double-click `FLPOrganizer.exe`.**
3. **Drag your `.flp` file** onto the window (or click *Browse…*).
4. Look at the preview. Each row shows a group, which tracks it will occupy, and how many clips it contains.
5. Click **Apply & Save…** and choose a destination. By default the new file is saved as `YourProject_organized.flp` next to the original.
6. **Open the new file in FL Studio.** Hit play, confirm it sounds identical, and check that everything is where you want it.
7. The tracks above number N (where N is "tracks needed" in the preview) contain your reorganized clips; tracks below that number are empty — delete them manually if you want.

### Tips

- **Always keep the original.** The app never overwrites it, but back it up anyway — `.flp` files are small.
- **Rename your channels and automations in FL before running the tool.** Clips inherit their group name from the channel/sample they reference. A named channel (e.g. *Kick Top*) groups better than an auto-named one (e.g. *Channel#37*).
- **Automations that share a name will be merged onto the same track group.** If you want them separate, rename them first.

---

## How it works (technical)

The `.flp` file format is a sequence of "events" — ID byte + payload — defined by Image-Line internally. Each arrangement contains a `Playlist` event whose payload is an array of fixed-size records, one per playlist clip. Each record contains (among other things):

- The clip's **time position** (u32)
- The clip's **length** (u32)
- A **reference** to the source channel or pattern (u16)
- The clip's **track index**, stored reversed as `499 - track_0based` (u16)

FLP Organizer parses the file, identifies every real clip, groups them by the channel/pattern/sample name, sorts the groups alphabetically, runs a classic **lane-assignment algorithm** inside each group (so overlapping clips fall onto adjacent tracks), and finally **patches only the 2 bytes** that encode the track index of each clip.

A typical run on a medium project modifies around 40–100 bytes in a 1–5 MB file. Because nothing else is touched, the risk of introducing corruption is effectively zero — as long as the format detection is correct for your FL Studio version.

### Supported FL Studio versions

Each playlist clip is stored as a fixed-size record, and the size has changed between FL Studio versions. Image-Line doesn't document this; the sizes below were found by comparing hex dumps of real projects.

| FL Studio version | Clip record size | Status |
|---|---|---|
| FL Studio 24 | 80 bytes | Tested |
| FL Studio 25 | 80 bytes | Tested |
| FL Studio 2026 (26.x) | 88 bytes | Tested (since v1.6.0) |

FLP Organizer detects the record size automatically by checking the `pattern_base` field (always `20480` for real clips) across several candidate sizes.

If a future FL version changes the layout again, the auto-detect may fail. In that case please open an issue and attach a small test project.

---

## Building from source

### Requirements
- Python 3.9 or newer
- Windows (for building the `.exe`). The GUI itself runs on macOS and Linux too, but releases target Windows because FL Studio is Windows-native.

### Build steps (Windows)

```bat
git clone https://github.com/YOUR_USERNAME/flp-organizer.git
cd flp-organizer
build.bat
```

The executable is produced at `dist\FLPOrganizer.exe`.

### Run from source (any platform)

```bash
pip install -r requirements.txt
python src/flp_gui.py
```

### Command-line usage (no GUI)

The core engine is importable as a library:

```python
from flp_core import analyze, reorganize

# Just inspect what would change
result = analyze("MyProject.flp")
print(f"{len(result.groups)} groups, {result.total_tracks_needed} tracks needed")
for g in result.groups:
    print(f"  track {g.first_track}: {g.name} ({g.clip_count} clips)")

# Or do it in one call
reorganize("MyProject.flp", "MyProject_organized.flp")
```

---

## Disclaimer & safety

- **Always keep a backup of your `.flp`.** Although the tool writes to a new file and never modifies the original, storage and software are imperfect. Your project is your work.
- This is an **unofficial** tool. It is not affiliated with or endorsed by Image-Line. FL Studio and the `.flp` format are trademarks / property of Image-Line Software.
- Use at your own risk. See [LICENSE](LICENSE).

---

## Contributing

Issues and PRs welcome. Particularly useful contributions:
- Sample `.flp` files from different FL versions (sanitized — no audio, no VSTs, just the structure)
- Tests with edge cases (empty playlists, 500+ tracks, very long projects)
- Support for older FL Studio versions
- macOS / Linux build configurations (if someone uses FL on Linux via Wine)

## License

[MIT](LICENSE)
