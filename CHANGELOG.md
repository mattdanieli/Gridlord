# Changelog

All notable changes to this project will be documented in this file.

## [1.6.0] - 2026-10-17

### Added
- **FL Studio 2026 support.** FL Studio 2026 (version 26.x) stores each
  playlist clip in 88 bytes instead of the 80 bytes used by FL 24/25.
  Earlier versions of the tool didn't recognise that size, found zero
  clips, and had nothing to reorganize. 88 is now a recognised item size.
- The compatibility validator lists FL Studio 2026 as a tested version.

### Notes
- Verified on a real FL Studio 26.1.1 project: all clips are detected,
  only the track-index bytes change, and the output re-parses cleanly
  with the same file size.
- Mute preservation, auto-color and auto-rename use the same 70-byte
  TRACK_DATA layout as FL 25, which FL 2026 keeps. They have not yet been
  checked by opening the output in FL Studio 2026.

## [1.5.5] - 2026-04-26

### Added
- **Compatibility validator.** Before processing, the tool now scans the
  .flp file for known risk factors and shows a popup if anything looks
  off. The validator is read-only — it never modifies the file. It checks:
  - FL Studio version (tested: FL 24, FL 25)
  - TRACK_DATA payload size (known: 66 and 70 bytes)
  - Number of arrangements (less testing on multi-arrangement projects)
  - Trailing bytes after FLdt chunk
  - File parsability end-to-end
  - Outliers in pattern/track counts
- ERROR-level findings (e.g. file is not a valid .flp, file structure is
  corrupted) block the Apply button until the user explicitly confirms.
- WARNING-level findings (untested FL version, unusual TRACK_DATA size,
  multiple arrangements) show an info dialog and a status banner but do
  not block.
- 8 new translation strings for the validator messages, in all 6 languages.

### Changed
- The CLI and library API expose the validator via `validate_compatibility()`,
  returning a `ValidationReport` dataclass with `issues: list[CompatibilityIssue]`,
  `overall_severity` property, and structured stats.

## [1.5.4] - 2026-04-26

### Fixed (critical)
- **FL Studio crash on auto-rename for FL 24 projects.** The writer
  hard-coded a 70-byte payload size for `ID_TRACK_DATA` (the size used by
  FL Studio 25.1.6, which is what reverse-engineering had been done on).
  FL Studio 24 uses a 66-byte payload. The auto-rename insert position
  was therefore computed 4 bytes inside an existing event, splitting it
  in two and corrupting every event that followed. FL Studio crashed
  when reading the file. The writer now uses the actual parsed payload
  size for each `ID_TRACK_DATA` (stored as `payload_end` in
  `track_info_state`).
- **Auto-rename now skips tracks that already have a custom name.**
  Previously, on tracks with an existing `ID_TRACK_NAME` (event 239),
  the tool would insert a *second* event 239 of its own. Two consecutive
  TRACK_NAME events made the parser misalign and FL Studio crashed.
  Now the parser sets `has_name = True` for any track that already has
  a name, and auto-rename skips those.

### Notes
- This bug affected any project saved by FL Studio 24 or earlier where
  a track already had a custom name. The 1987.flp test project hit both
  conditions and crashed FL Studio on opening the organized output.

## [1.5.3] - 2026-04-25

### Fixed
- **Pattern colour now actually applies in FL Studio.** v1.5.2 wrote
  events with the wrong event ID (149 instead of 150) and with the byte
  order reversed (B, G, R instead of R, G, B). These were inserted into
  the file but FL Studio quietly ignored them. v1.5.3 uses the correct
  event ID (`ID_PATTERN_COLOR = 150`) and the correct byte order
  (R, G, B, alpha).
- Added a `_decode_color_payload` helper so colour-event payloads are
  consistently decoded as canonical `0xRRGGBB` integers regardless of
  the file's little-endian byte order. Both `ID_CHANNEL_COLOR` (128)
  and `ID_PATTERN_COLOR` (150) go through it.

### Notes
- Reverse-engineered with a tiny test project: a one-pattern .flp with
  the pattern's colour set to `#7F0010` (Maroon) revealed the byte layout
  `7F 00 10 00` at the right offset for event ID 0x96 (150). The
  previous tentative ID (149) turned out to be a different DWORD-typed
  event whose payload happens to look colour-like.

## [1.5.2] - 2026-04-25

### Fixed
- **Auto-color now also colours pattern clips.** v1.5.1 wrote
  `ID_PATTERN_COLOR` (event 149) only when an existing colour event was
  found, but most patterns in real projects have no such event by default.
  v1.5.2 inserts a new `ID_PATTERN_COLOR` event right after each
  `ID_PATTERN_NEW` (event 65) for patterns that lack one, so the rainbow
  hue is applied to all groups regardless of clip type (audio or pattern).

### Technical
- New `_color_inserts` list in `AnalysisResult`, applied alongside
  `_name_inserts` in the writer pass that grows the file. The FLdt chunk
  size is updated to reflect the total inserted bytes.
- Each inserted event is 5 bytes: `0x95` (ID 149) + 4-byte little-endian
  DWORD `0x00 BB GG RR`.

## [1.5.1] - 2026-04-25

### Fixed
- **Auto-color now actually applies to clips.** v1.5.0 wrote colour bytes
  to the playlist tracks, but FL Studio doesn't reliably honour those
  bytes (the override flag is missing for the very first track). v1.5.1
  changes strategy: it writes a perceptually-distinct rainbow colour to
  each group's source channels and patterns (`ID_CHANNEL_COLOR` event 128
  and `ID_PATTERN_COLOR` event 149). Since clip colour in FL Studio comes
  from the source channel/pattern, this is what users actually see.
- Improved default-colour detection: a heuristic (`_is_default_color`)
  identifies FL Studio's auto-assigned greys (low saturation and low
  luminosity) so user-picked colours are preserved while drab defaults
  get overwritten.

### Other
- All v1.5.0 features remain (preserve muted tracks, preserve clip-level
  mute, auto-rename tracks). Only the auto-color implementation changed.

## [1.5.0] - 2026-04-25

### Added
- **Preserve muted tracks.** Clips from originally-muted tracks are placed
  into a dedicated group of muted tracks at the bottom of the playlist
  (labelled `[muted] ...` in the preview). Tracks that used to be muted but
  no longer contain muted content are reactivated automatically.
- **Preserve individual clip mute (ghost clips).** The "X" marker on single
  muted clips is preserved when those clips are moved to new tracks.
- **Auto-color (rainbow)** (opt-in checkbox). For each group, the source
  channels and patterns get a perceptually-distinct rainbow colour. This
  colours the **clips** in the playlist (because clip colour comes from the
  source channel/pattern in FL Studio). Custom colours that the user has
  manually picked are preserved — only auto-assigned default greys are
  overwritten. (Note: this is different from colouring the playlist tracks
  themselves, which FL Studio handles via a separate mechanism that proved
  unreliable to write to.)
- **Auto-rename tracks** (opt-in checkbox). Each destination track is
  renamed to match its group (e.g. `Kick Basic.wav` or `Pattern 24`).
  Implemented by inserting `ID_TRACK_NAME` (event 239) entries after each
  `ID_TRACK_DATA` and updating the FLdt chunk size.

### Technical
- Reverse-engineered on FL Studio 25.1.6:
  - Track mute: byte 12 of `ID_TRACK_DATA` payload (`0x01` active / `0x00` muted).
  - Clip mute: bit 5 (`0x20`) of byte 19 in the 80-byte playlist item.
  - Channel colour: `ID_CHANNEL_COLOR` (event 128), 4-byte DWORD `0x00RRGGBB`.
  - Pattern colour: `ID_PATTERN_COLOR` (event 149), same layout.
  - Track name: `ID_TRACK_NAME` (event 239), UTF-16LE null-terminated.
- Heuristic `_is_default_color`: identifies FL Studio's auto-assigned greys
  (R, G, B all under 0x70 with low spread) so user customisations are kept.

### Roadmap
- **v1.6.0**: remove empty tracks, custom colour palettes for groups.

## [1.4.0] - 2026-04-21

### Added
- **Sort by color** — new sub-sort modifier that orders groups by a
  perceptual rainbow hue (red → orange → yellow → green → blue → purple).
  Greys go in the middle, uncoloured groups at the end.
- The color of a group is the most frequent color among its source
  channels or patterns (majority wins).
- Core now parses `ID_CHANNEL_COLOR` (event 128) and `ID_PATTERN_COLOR`
  (event 149). `ClipInfo` exposes a new `color` field.
- CLI: `--sub-color` is now fully functional.

### Roadmap (coming in v1.5.0)
- Auto-rename tracks from group names
- Auto-color tracks
- Remove empty tracks
- Preserve muted track state (strada B from v1.3.x discussion)

## [1.3.1] - 2026-04-21

### Fixed
- Language selector dropdown was invisible because the top-bar frame was
  placed on the same grid row as the header, causing the header to overlap
  and hide it. The layout now reserves a dedicated row for the language
  selector above the header.

## [1.3.0] - 2026-04-21

### Added
- **Batch mode** — process up to 30 .flp files in one run. New "Single file /
  Batch" tab selector at the top. In batch mode you can choose a custom output
  folder or leave the default (outputs saved next to each input).
- **Sub-sort modifiers** — checkboxes that refine the primary track order:
  - *Group by type*: audio clips come before patterns.
  - *Sort by clip length*: groups with longer clips first.
  - *Sort by color* (placeholder, coming soon).
  These combine freely with both Alphabetical and By-first-appearance.
- **Post-process options card** with checkboxes for upcoming features
  (auto-rename tracks, auto-color tracks, remove empty tracks). These are
  visible but disabled ("coming soon") and will be implemented in v1.4.0.
- **Multi-language UI** — language selector in the top-right corner. Choose
  between English, Deutsch, Español, Français, Italiano, and Русский. The
  choice is persisted in `~/.flp_organizer_config.json` and remembered across
  launches. All labels, captions, buttons, status messages, and the disclaimer
  text are translated.
- CLI: `--batch`, `--output-dir`, `--sub-length`, `--sub-type`, `--sub-color`.

### Changed
- CLI now accepts multiple input paths; existing single-file usage is
  backwards-compatible.
- Default window size slightly larger (920×780) to fit the new controls card.

### Technical
- New module `src/translations.py` holding every UI string for 6 languages.
- `flp_core.analyze()` and `flp_core.reorganize()` accept an optional
  `sub_sort` list argument.

## [1.2.1] - 2026-04-21

### Fixed
- Disclaimer dialog buttons ("I agree" / "Decline") no longer get clipped off
  the bottom on some display configurations. Dialog is now larger (620×500),
  resizable with a minimum size, and the buttons reserve fixed space at the
  bottom.

## [1.2.0] - 2026-04-21

### Added
- Custom app icon bundled into the executable (taskbar + window title bar).
- Centred header at the top: icon + "FLP" (orange) + "Organizer" (white) +
  version.
- First-launch disclaimer popup clarifying that FLP Organizer is independent
  and not affiliated with Image-Line.
- Footer with "Made with 🧡 by Matt Danieli" and a "Help me build more tools"
  button that opens a donation link in the browser.

### Changed
- Window is now fully resizable without buttons disappearing (grid layout).

## [1.1.0] - 2026-04-21

### Added
- **Two sorting modes selectable from the GUI:**
  - Alphabetical (A–Z)
  - By first appearance
- CLI accepts `--sort alpha` or `--sort first`.

### Changed
- Dark FL-Studio-inspired theme with orange accents.
- GUI rebuilt around standard ttk widgets for stability on Windows.

## [1.0.0] - 2026-04-20

### Added
- Initial release.
- Drag & drop GUI for Windows (also runs on macOS / Linux from source).
- Byte-level surgical patching: modifies only track-index bytes, leaves
  everything else untouched.
- Auto-detection of playlist item size (supports FL Studio 21, 24, 25).
- Grouping by sample name / channel name / pattern name.
- Lane assignment for overlapping clips of the same group.
- Alphabetical sorting (case-insensitive).
- Preview pane before applying changes.
- Threaded analysis/write with progress bar.
- Safety: refuses to overwrite the input file.
- Python library API (`flp_core`) for scripting / batch use.
- Command-line interface (`cli.py`) for headless / batch processing.
- GitHub Actions workflow for automatic Windows release builds.
