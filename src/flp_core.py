"""
FLP Organizer - Core engine
===========================

Surgical byte-level reorganizer for FL Studio project files (.flp).

Groups all playlist clips by name (sample name, automation name, pattern name)
onto adjacent tracks while preserving every clip's original time position,
length, color, automation data, and every other property.

Works by patching only the 2 bytes that encode the track index of each playlist
item. The rest of the file is left byte-for-byte identical to the original.

Compatible with FL Studio 21 and newer. Tested on FL Studio 25.1.6 (item size
320 bytes). Automatic detection of item size for other versions (falls back to
64, 60, or 32 bytes).

Public API:
    analyze(flp_path) -> AnalysisResult
        Reads the .flp file and returns the grouping plan without modifying it.

    reorganize(flp_path, output_path) -> AnalysisResult
        Reads, reorganizes, and writes the output file.
"""
from __future__ import annotations
import struct
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Optional

# ----- FLP format constants -----
BYTE_BASE, WORD_BASE, DWORD_BASE, TEXT_BASE, DATA_BASE = 0, 64, 128, 192, 208

ID_CHANNEL_NEW      = WORD_BASE + 0          # 64  - opens a channel, payload u16 iid
ID_PATTERN_NEW      = WORD_BASE + 1          # 65  - opens a pattern, payload u16 num
ID_PATTERN_NAME     = TEXT_BASE + 1          # 193
ID_CHANNEL_SAMPLE   = TEXT_BASE + 4          # 196 - sample path
ID_FL_VERSION       = TEXT_BASE + 7          # 199
ID_DEFAULT_NAME     = TEXT_BASE + 9          # 201 - default channel name
ID_CHANNEL_NAME     = TEXT_BASE + 39         # 231 - user channel name
ID_ARRANGEMENT_NEW  = WORD_BASE + 35         # 99
ID_TRACK_NAME       = TEXT_BASE + 47         # 239
ID_ARRANGEMENT_NAME = TEXT_BASE + 49         # 241
ID_PLAYLIST         = DATA_BASE + 25         # 233
ID_TRACK_DATA       = DATA_BASE + 30         # 238
# FL Studio 2026 (26.x) writes event 172 with a 5-byte payload, breaking the
# usual "IDs 128-191 carry 4 bytes" rule. Reading it as 4 bytes shifts every
# following event by one byte. Only applied to FL 26+ files; FL 24/25
# projects never contain this event.
ID_FL26_FIVE_BYTE = 172
FL26_FIVE_BYTE_SIZE = 5

ID_CHANNEL_COLOR    = DWORD_BASE + 0         # 128 - channel colour, 4 bytes 0x00RRGGBB
ID_PATTERN_COLOR    = DWORD_BASE + 22        # 150 - pattern colour (verified on FL 25.1.6)

# Byte offset inside the ID_TRACK_DATA payload where the "enabled" byte lives.
# Reverse-engineered on FL 25.1.6: byte 12 = 0x01 (active) / 0x00 (muted).
TRACK_DATA_MUTE_OFFSET = 12

# Byte offset inside a single 80-byte playlist item where the clip's
# "muted" flag bit lives. Reverse-engineered on FL 25.1.6: bit 5 (0x20) of
# byte 19 is set when the clip is individually muted (ghost/X).
CLIP_ITEM_FLAGS_OFFSET = 19
CLIP_MUTED_BIT = 0x20

# Byte offsets inside the ID_TRACK_DATA payload for the track colour (3 bytes).
# Bytes 4-6 = RGB (0xRR 0xGG 0xBB).
TRACK_DATA_COLOR_OFFSET = 4    # first of 3 RGB bytes


def _decode_color_payload(payload: bytes) -> int:
    """Decode a 4-byte color event payload into a 0xRRGGBB integer.

    FL Studio stores colours as a 4-byte event payload where the bytes are,
    in file order: R, G, B, alpha. As a little-endian DWORD this reads as
    0x00BBGGRR which is NOT what we want for our internal 0xRRGGBB model.
    This helper rebuilds the canonical 0xRRGGBB integer from the raw bytes.
    """
    if len(payload) < 3:
        return 0
    return (payload[0] << 16) | (payload[1] << 8) | payload[2]

PATTERN_BASE_VAL = 20480   # item_index > PATTERN_BASE_VAL => pattern clip
MAX_TRACKS = 500           # FL Studio 21+ has 500 playlist tracks

ITEM_SIZE_CANDIDATES = (32, 60, 64, 80, 88, 320)  # smallest first — 80 = FL 24/25, 88 = FL 2026


# ----- Sort mode constants -----
SORT_ALPHABETICAL        = "alphabetical"
SORT_BY_FIRST_APPEARANCE = "first_appearance"

# Sub-sort modifiers (applied on top of the primary sort mode)
SUB_BY_LENGTH   = "length"    # longer clips first within the same primary order
SUB_BY_TYPE     = "type"      # audio clips first, then patterns, then automation
SUB_BY_COLOR    = "color"     # groups with the same primary rank are then grouped by color


# ----- Data classes -----

@dataclass
class ClipInfo:
    name: str                   # Group name (sample file, channel name, pattern name)
    kind: str                   # 'channel' or 'pattern'
    position: int               # Start position in PPQ ticks
    length: int                 # Length in PPQ ticks
    old_track: int              # Original track, 1-based
    new_track: int = 0          # Assigned track, 1-based
    color: Optional[int] = None  # 24-bit RGB of source channel/pattern (0xRRGGBB) or None
    was_muted: bool = False      # True if source track was muted in the original file
    was_clip_muted: bool = False # True if the clip itself was individually muted (ghost)
    _source_idx: Optional[int] = None  # Internal: source channel id or pattern id (0-based)
    # Byte offsets in the file (used by the writer)
    item_offset: int = 0
    rvidx_offset: int = 0
    old_rvidx: int = 0
    clip_mute_offset: int = 0    # absolute file offset of the clip's mute-flag byte (item_offset + 19)


@dataclass
class GroupPlan:
    name: str
    first_track: int            # 1-based
    lanes_used: int             # number of adjacent tracks used by this group
    clip_count: int


@dataclass
class AnalysisResult:
    fl_version: str = ""
    ppq: int = 0
    channel_count: int = 0
    pattern_count: int = 0
    arrangement_count: int = 0
    total_clips: int = 0
    total_tracks_needed: int = 0
    groups: list[GroupPlan] = field(default_factory=list)
    clips: list[ClipInfo] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    # Internal: for the writer
    _data: bytes = b""
    _patches: list[tuple[int, int]] = field(default_factory=list)  # (offset, new_rvidx)
    # Mute patches: list of (absolute_byte_offset, new_value) to write into TRACK_DATA.
    # new_value is 0x01 (active) or 0x00 (muted).
    _mute_patches: list[tuple[int, int]] = field(default_factory=list)
    # Colour patches: same form (absolute_offset, new_byte_value).
    _color_patches: list[tuple[int, int]] = field(default_factory=list)
    # Colour-event inserts: list of (insert_at_offset, bytes_to_insert) for
    # patterns/channels that don't already have a colour event. Applied like
    # name inserts: file grows, FLdt size is updated.
    _color_inserts: list[tuple[int, bytes]] = field(default_factory=list)
    # Track-name inserts: list of (insert_at_offset, bytes_to_insert).
    # Applied after all byte-level patches. Each insert grows the file by
    # len(bytes_to_insert). The FLdt chunk size is updated accordingly.
    _name_inserts: list[tuple[int, bytes]] = field(default_factory=list)


# ----- Compatibility validation -----

# Severity levels for compatibility issues. Use the strings directly so they
# are easy to display in the UI without an enum import.
SEVERITY_OK      = "ok"
SEVERITY_WARNING = "warning"
SEVERITY_ERROR   = "error"


@dataclass
class CompatibilityIssue:
    """A single problem (or note) found by validate_compatibility()."""
    severity: str   # "ok", "warning", or "error"
    code: str       # Stable identifier for the UI to localise the message
    message: str    # English fallback message
    details: str = ""   # Optional extra context for "Show details"


@dataclass
class ValidationReport:
    """Outcome of validating an .flp before processing.

    The report is non-blocking — the GUI decides what to do with it. As a rule
    of thumb the GUI should refuse to run when there's at least one ERROR-level
    issue, ask for confirmation on WARNING-level, and proceed silently when
    everything is OK.
    """
    issues: list[CompatibilityIssue] = field(default_factory=list)
    fl_version: str = ""
    file_size: int = 0
    track_data_payload_size: Optional[int] = None
    arrangement_count: int = 0
    channel_count: int = 0
    pattern_count: int = 0
    track_count: int = 0
    patterns_without_color: int = 0
    tracks_without_name: int = 0

    @property
    def has_errors(self) -> bool:
        return any(i.severity == SEVERITY_ERROR for i in self.issues)

    @property
    def has_warnings(self) -> bool:
        return any(i.severity == SEVERITY_WARNING for i in self.issues)

    @property
    def overall_severity(self) -> str:
        if self.has_errors:
            return SEVERITY_ERROR
        if self.has_warnings:
            return SEVERITY_WARNING
        return SEVERITY_OK


# Versions that have been actually tested and confirmed working.
KNOWN_GOOD_FL_VERSIONS = {
    "24": "FL Studio 24 (66-byte TRACK_DATA payload)",
    "25": "FL Studio 25 (70-byte TRACK_DATA payload)",
    "26": "FL Studio 2026 (70-byte TRACK_DATA payload, 88-byte playlist items)",
}

# TRACK_DATA payload sizes seen in the wild. The writer doesn't care about the
# exact value (it uses the parsed end position), but unknown sizes deserve a
# warning because they signal an FL Studio version we haven't tested.
KNOWN_TRACK_DATA_SIZES = {66, 70}


def validate_compatibility(flp_path: Path | str) -> ValidationReport:
    """Examine an .flp file and report potential compatibility issues.

    Goal: warn the user BEFORE the writer modifies anything so they can
    decide whether to proceed. The function only reads the file — never
    writes — so it's always safe to call.

    Returns a ValidationReport whose `issues` list is empty in the happy path.
    """
    report = ValidationReport()
    p = Path(flp_path)

    # 1. File exists & has the right magic bytes
    if not p.is_file():
        report.issues.append(CompatibilityIssue(
            severity=SEVERITY_ERROR,
            code="file_not_found",
            message=f"File not found: {p}",
        ))
        return report

    try:
        data = p.read_bytes()
    except OSError as e:
        report.issues.append(CompatibilityIssue(
            severity=SEVERITY_ERROR,
            code="file_unreadable",
            message=f"Cannot read file: {e}",
        ))
        return report

    report.file_size = len(data)

    if len(data) < 22 or data[:4] != b"FLhd":
        report.issues.append(CompatibilityIssue(
            severity=SEVERITY_ERROR,
            code="not_flp",
            message="File is not a valid FL Studio .flp project (missing FLhd header).",
        ))
        return report

    # 2. Walk the structure to look for obvious anomalies. We do a *minimal*
    #    parse here, not the full analyze(), because we want to catch problems
    #    that would crash analyze() too.
    try:
        hdr_size = struct.unpack_from("<I", data, 4)[0]
        fldt_pos = 8 + hdr_size
        if data[fldt_pos:fldt_pos+4] != b"FLdt":
            report.issues.append(CompatibilityIssue(
                severity=SEVERITY_ERROR,
                code="bad_fldt_marker",
                message="FLdt chunk marker not found where expected.",
                details=f"Expected at offset {fldt_pos}, got {data[fldt_pos:fldt_pos+4]!r}",
            ))
            return report
        fldt_size = struct.unpack_from("<I", data, fldt_pos + 4)[0]
        events_end = fldt_pos + 8 + fldt_size

        if events_end != len(data):
            # Some FL Studio projects legitimately have trailing bytes after
            # the FLdt chunk (presets, plugin caches), so this is only a
            # warning, not an error.
            extra = len(data) - events_end
            if extra < 0:
                report.issues.append(CompatibilityIssue(
                    severity=SEVERITY_ERROR,
                    code="fldt_overruns_file",
                    message="FLdt chunk size exceeds file size (file is truncated).",
                    details=f"Declared size: {fldt_size}, file ends at: {len(data)}",
                ))
                return report
            else:
                report.issues.append(CompatibilityIssue(
                    severity=SEVERITY_WARNING,
                    code="trailing_bytes",
                    message=f"File has {extra} bytes of trailing data after FLdt chunk.",
                    details="This is unusual but should not affect processing.",
                ))

        # 3. Walk events
        pos = fldt_pos + 8
        end = events_end
        track_data_sizes_seen: set[int] = set()
        cur_track_idx = -1
        cur_pat = None
        cur_arr_count = 0
        channel_ids: set[int] = set()
        pattern_ids: set[int] = set()
        track_count = 0
        patterns_with_color: set[int] = set()
        tracks_with_name: set[int] = set()
        fl_version = ""
        fl_major = 0

        while pos < end:
            evt_start = pos
            evt_id = data[pos]; pos += 1
            if evt_id < 64:
                payload_start = pos; pos += 1
            elif evt_id < 128:
                payload_start = pos; pos += 2
            elif evt_id == ID_FL26_FIVE_BYTE and fl_major >= 26:
                payload_start = pos; pos += FL26_FIVE_BYTE_SIZE
            elif evt_id < 192:
                payload_start = pos; pos += 4
            else:
                length, shift = 0, 0
                while True:
                    if pos >= end:
                        report.issues.append(CompatibilityIssue(
                            severity=SEVERITY_ERROR,
                            code="varint_truncated",
                            message="Event length varint truncated near end of FLdt chunk.",
                            details=f"At file offset 0x{evt_start:08x}",
                        ))
                        return report
                    byte = data[pos]; pos += 1
                    length |= (byte & 0x7f) << shift
                    if not (byte & 0x80):
                        break
                    shift += 7
                    if shift > 35:
                        report.issues.append(CompatibilityIssue(
                            severity=SEVERITY_ERROR,
                            code="varint_overflow",
                            message="Malformed event length (varint overflow).",
                            details=f"At file offset 0x{evt_start:08x}, last_id={evt_id}",
                        ))
                        return report
                payload_start = pos
                pos += length

            if pos > end:
                report.issues.append(CompatibilityIssue(
                    severity=SEVERITY_ERROR,
                    code="event_overruns",
                    message=f"Event at offset 0x{evt_start:08x} (id={evt_id}) overruns FLdt boundary.",
                    details="The file structure is corrupted — probably modified by a tool that "
                            "didn't update the FLdt size header.",
                ))
                return report

            payload_size = pos - payload_start

            # Specific event handling
            if evt_id == ID_FL_VERSION:
                # FL version string, e.g. "25.1.6.4997"
                try:
                    fl_version = data[payload_start:payload_start+payload_size].rstrip(b"\x00").decode("ascii", errors="replace")
                    fl_major = int(fl_version.split(".")[0])
                except Exception:
                    pass
            elif evt_id == ID_CHANNEL_NEW:
                channel_ids.add(struct.unpack("<H", data[payload_start:payload_start+2])[0])
            elif evt_id == ID_PATTERN_NEW:
                cur_pat = struct.unpack("<H", data[payload_start:payload_start+2])[0]
                pattern_ids.add(cur_pat)
            elif evt_id == ID_PATTERN_COLOR and cur_pat is not None:
                patterns_with_color.add(cur_pat)
            elif evt_id == ID_ARRANGEMENT_NEW:
                cur_arr_count += 1
                cur_track_idx = -1
            elif evt_id == ID_TRACK_DATA:
                cur_track_idx += 1
                track_count += 1
                track_data_sizes_seen.add(payload_size)
            elif evt_id == ID_TRACK_NAME and cur_track_idx >= 0:
                tracks_with_name.add(cur_track_idx)

        # Populate report stats
        report.fl_version = fl_version
        report.arrangement_count = cur_arr_count
        report.channel_count = len(channel_ids)
        report.pattern_count = len(pattern_ids)
        report.track_count = track_count
        report.patterns_without_color = len(pattern_ids) - len(patterns_with_color)
        report.tracks_without_name = track_count - len(tracks_with_name)

        # 4. Build issues from collected stats

        # 4a. Track data payload size
        if track_data_sizes_seen:
            sizes_str = ", ".join(str(s) for s in sorted(track_data_sizes_seen))
            primary = max(track_data_sizes_seen, key=lambda s: 1 if s in KNOWN_TRACK_DATA_SIZES else 0)
            report.track_data_payload_size = primary
            unknown_sizes = track_data_sizes_seen - KNOWN_TRACK_DATA_SIZES
            if unknown_sizes:
                report.issues.append(CompatibilityIssue(
                    severity=SEVERITY_WARNING,
                    code="unknown_track_data_size",
                    message=f"Unusual TRACK_DATA payload size(s) detected: {sizes_str} bytes.",
                    details="The tool has been tested with 66-byte (FL 24) and 70-byte (FL 25) "
                            "payloads. Other sizes might come from a different FL Studio version "
                            "and the writer hasn't been tested against them. Auto-rename in "
                            "particular relies on accurate payload boundaries.",
                ))

        # 4b. FL version
        if fl_version:
            major = fl_version.split(".")[0] if "." in fl_version else ""
            if major and major not in KNOWN_GOOD_FL_VERSIONS:
                report.issues.append(CompatibilityIssue(
                    severity=SEVERITY_WARNING,
                    code="untested_fl_version",
                    message=f"FL Studio version {fl_version} hasn't been tested by this tool.",
                    details="Tested versions: FL 24 and FL 25. Other versions may work fine, "
                            "or may have a different .flp internal layout that the writer "
                            "doesn't handle correctly. We recommend keeping a backup.",
                ))

        # 4c. Multiple arrangements
        if cur_arr_count > 1:
            report.issues.append(CompatibilityIssue(
                severity=SEVERITY_WARNING,
                code="multiple_arrangements",
                message=f"Project has {cur_arr_count} arrangements.",
                details="The tool processes each arrangement, but multi-arrangement support "
                        "has had less testing than single-arrangement projects. Check that all "
                        "arrangements look correct in the output.",
            ))

        # 4d. No clips at all
        if track_count == 0:
            report.issues.append(CompatibilityIssue(
                severity=SEVERITY_WARNING,
                code="no_tracks",
                message="No playlist tracks found in this project.",
                details="There's nothing for the tool to reorganize.",
            ))

        # 4e. Lots of inserts coming
        if report.patterns_without_color > 200:
            report.issues.append(CompatibilityIssue(
                severity=SEVERITY_WARNING,
                code="many_pattern_inserts",
                message=f"{report.patterns_without_color} patterns lack a colour event.",
                details="Auto-color (if enabled) will insert that many new events into the "
                        "file. This is supported but produces a noticeably larger file. "
                        "Consider running auto-color only if you really want pattern colors.",
            ))

    except Exception as e:
        # Defensive: never let validation itself raise — the user can still
        # run analyze() at their own risk.
        report.issues.append(CompatibilityIssue(
            severity=SEVERITY_ERROR,
            code="validation_crash",
            message=f"Validator crashed unexpectedly: {e}",
            details="The file may have an unusual structure. Proceeding could be risky.",
        ))

    return report


# ----- Low-level parsing helpers -----

def _read_varint(data: bytes, offset: int) -> tuple[int, int]:
    result, shift = 0, 0
    while True:
        b = data[offset]
        offset += 1
        result |= (b & 0x7F) << shift
        if (b & 0x80) == 0:
            return result, offset
        shift += 7


def _decode_str(raw: bytes, is_unicode: bool) -> str:
    try:
        if is_unicode:
            return raw.decode("utf-16-le").rstrip("\0")
        return raw.decode("latin-1").rstrip("\0")
    except Exception:
        return raw.decode("utf-16-le", errors="replace").rstrip("\0")


def _detect_item_size(data: bytes, offset: int, total_size: int) -> Optional[int]:
    """Picks the item size where every item's pattern_base field equals 20480.

    Returns None if no candidate matches cleanly.
    """
    # Prefer perfect matches first
    for candidate in ITEM_SIZE_CANDIDATES:
        if total_size % candidate != 0:
            continue
        n = total_size // candidate
        if n == 0:
            continue
        ok = True
        for k in range(n):
            pb = struct.unpack_from("<H", data, offset + k * candidate + 4)[0]
            if pb != PATTERN_BASE_VAL:
                ok = False
                break
        if ok:
            return candidate

    # Fallback: best ratio above 50%
    best, best_ratio = None, 0.5
    for candidate in ITEM_SIZE_CANDIDATES:
        if total_size % candidate != 0:
            continue
        n = total_size // candidate
        if n == 0:
            continue
        good = sum(
            1 for k in range(n)
            if struct.unpack_from("<H", data, offset + k * candidate + 4)[0] == PATTERN_BASE_VAL
        )
        ratio = good / n
        if ratio > best_ratio:
            best_ratio, best = ratio, candidate
    return best


def _group_name(pattern_base: int, item_index: int,
                channels: dict, patterns: dict) -> tuple[Optional[str], Optional[str]]:
    if pattern_base != PATTERN_BASE_VAL:
        return None, None
    if item_index > PATTERN_BASE_VAL:
        pnum = item_index - PATTERN_BASE_VAL
        p = patterns.get(pnum, {})
        name = p.get("name") or f"Pattern {pnum}"
        return name, "pattern"

    ch = channels.get(item_index, {})
    nm = ch.get("name")
    if nm:
        return nm, "channel"
    sp = ch.get("sample_path")
    if sp:
        return Path(sp.replace("\\", "/")).name, "channel"
    dn = ch.get("default_name")
    if dn:
        return dn, "channel"
    return f"Chan#{item_index}", "channel"


def _is_default_color(rgb: int) -> bool:
    """Heuristic: returns True for colours FL Studio auto-assigns to channels
    that the user hasn't manually customized.

    These are the dark, low-saturation greys (R, G, B all in roughly 0x10-0x60
    range with low spread). User-picked colours have high saturation and/or
    higher luminosity.
    """
    R = (rgb >> 16) & 0xFF
    G = (rgb >> 8)  & 0xFF
    B =  rgb        & 0xFF
    # Very dark — probably default
    if R < 0x70 and G < 0x70 and B < 0x70:
        # Low saturation? compute max-min
        spread = max(R, G, B) - min(R, G, B)
        if spread < 0x40:   # less than 64 of variance between channels = grey-ish
            return True
    return False


# Channel and pattern colours that FL Studio uses when nothing is set by
# the user. Kept as exact set of well-known defaults; the heuristic in
# _is_default_color extends this for cases like 0x39352f (auto-assigned greys).
DEFAULT_CHANNEL_COLORS = {0x3c4043, 0x494742, 0x000000, 0x141414}
DEFAULT_PATTERN_COLORS = {0x3c4043, 0x494742, 0x000000, 0x141414}


def _hsv_to_rgb(h_deg: float, s: float, v: float) -> int:
    """Convert HSV (h in degrees 0-360, s/v in 0-1) to a 24-bit RGB int 0xRRGGBB."""
    h = (h_deg % 360.0) / 60.0
    c = v * s
    x = c * (1 - abs(h % 2 - 1))
    m = v - c
    if 0 <= h < 1:   r, g, b = c, x, 0
    elif 1 <= h < 2: r, g, b = x, c, 0
    elif 2 <= h < 3: r, g, b = 0, c, x
    elif 3 <= h < 4: r, g, b = 0, x, c
    elif 4 <= h < 5: r, g, b = x, 0, c
    else:            r, g, b = c, 0, x
    R = int(round((r + m) * 255))
    G = int(round((g + m) * 255))
    B = int(round((b + m) * 255))
    return (R << 16) | (G << 8) | B


def _rainbow_color_for(group_index: int, total_groups: int) -> int:
    """Return a rainbow colour for the i-th group out of total_groups, 0xRRGGBB.
    Uses HSV with hue spread evenly, mid-high saturation/value for visibility.
    """
    if total_groups <= 0:
        return 0x808080
    hue = (group_index * 360.0 / total_groups) % 360.0
    return _hsv_to_rgb(hue, s=0.65, v=0.95)


def _dominant_color(clips: list[ClipInfo]) -> Optional[int]:
    """Return the most frequent non-None colour in the clip list, or None."""
    counts: dict[int, int] = {}
    for c in clips:
        if c.color is not None:
            counts[c.color] = counts.get(c.color, 0) + 1
    if not counts:
        return None
    # Highest count wins; if tie, the lowest RGB value wins (deterministic)
    return max(counts.items(), key=lambda kv: (kv[1], -kv[0]))[0]


def _rgb_to_hue_key(rgb: Optional[int]) -> tuple[int, float, float]:
    """Convert a 0xRRGGBB colour to a sort key that produces a perceptual
    rainbow order (red → orange → yellow → green → cyan → blue → magenta).
    Greys and blacks (low saturation) go at the end.

    Returns (bucket, hue, brightness) where:
        bucket = 0 for coloured values, 1 for greys (sorted last)
        hue    = 0-360 degree wheel position
        brightness = tiebreaker within the same hue
    """
    if rgb is None:
        return (2, 0.0, 0.0)   # no colour info -> absolute last
    r = ((rgb >> 16) & 0xFF) / 255.0
    g = ((rgb >> 8)  & 0xFF) / 255.0
    b = ( rgb        & 0xFF) / 255.0
    mx = max(r, g, b)
    mn = min(r, g, b)
    delta = mx - mn
    # Saturation used to separate greys from colours
    sat = 0 if mx == 0 else delta / mx

    if delta < 1e-6 or sat < 0.1:
        # Grey: bucket 1, sorted by brightness
        return (1, 0.0, mx)

    if mx == r:
        h = ((g - b) / delta) % 6
    elif mx == g:
        h = (b - r) / delta + 2
    else:
        h = (r - g) / delta + 4
    hue_deg = h * 60.0
    return (0, hue_deg, mx)


def _assign_lanes(clips: list[ClipInfo], base_track_0: int) -> int:
    """Classic lane-assignment. Mutates each clip's .new_track (1-based).
    Returns the number of lanes used.
    """
    order = sorted(range(len(clips)), key=lambda i: clips[i].position)
    lanes_end: list[int] = []
    for idx in order:
        c = clips[idx]
        placed = False
        for li, le in enumerate(lanes_end):
            if c.position >= le:
                lanes_end[li] = c.position + c.length
                c.new_track = base_track_0 + li + 1  # convert to 1-based
                placed = True
                break
        if not placed:
            lanes_end.append(c.position + c.length)
            c.new_track = base_track_0 + len(lanes_end)  # 1-based
    return len(lanes_end)


# ----- Main entry points -----

SORT_ALPHABETICAL_OLD = "alphabetical"  # legacy alias kept for compatibility


def analyze(flp_path: Path | str,
            sort_mode: str = SORT_ALPHABETICAL,
            sub_sort: Optional[list[str]] = None,
            apply_auto_color: bool = False,
            apply_auto_rename: bool = False) -> AnalysisResult:
    """Parse the .flp file and compute the grouping plan. Does not modify anything.

    sort_mode:
        SORT_ALPHABETICAL        - groups ordered A-Z, case-insensitive (default)
        SORT_BY_FIRST_APPEARANCE - groups ordered by the earliest position of
                                   any clip in that group

    sub_sort: optional list of sub-sort modifiers that refine the order within
              the primary sort. They are applied as tie-breakers in the listed
              order. Available modifiers:
                SUB_BY_LENGTH - groups with longer average clip length first
                SUB_BY_TYPE   - audio clips first, then patterns
                SUB_BY_COLOR  - groups ordered by perceptual rainbow hue
                                (red → orange → yellow → green → blue → purple).
                                Greys and uncoloured groups go at the end.
    """
    path = Path(flp_path)
    data = path.read_bytes()

    if data[:4] != b"FLhd":
        raise ValueError("Not a valid FLP file (missing 'FLhd' header)")
    if data[14:18] != b"FLdt":
        raise ValueError("Not a valid FLP file (missing 'FLdt' chunk)")

    ppq = struct.unpack("<H", data[12:14])[0]
    events_size = struct.unpack("<I", data[18:22])[0]
    if len(data) != events_size + 22:
        raise ValueError("Corrupted FLP: data chunk size mismatch")

    result = AnalysisResult(ppq=ppq, _data=data)

    # Parse events
    pos = 22
    end = 22 + events_size
    is_unicode = True
    channels: dict[int, dict] = {}
    patterns: dict[int, dict] = {}
    arrangements: list[dict] = []
    cur_ch: Optional[int] = None
    cur_pat: Optional[int] = None
    cur_arr: Optional[dict] = None
    cur_track_idx = -1
    # Per-arrangement mapping: cur_track_idx -> {"mute_off", "color_off", "override_off", "mute_val", "color"}
    track_info_state: dict[int, dict] = {}
    # Last seen ID 43 event (track colour override flag) before a TRACK_DATA
    last_43_offset: Optional[int] = None
    fl_major = 0   # FL Studio major version, read from the version event

    while pos < end:
        evt_id = data[pos]
        pos += 1
        payload_start = pos
        if evt_id < WORD_BASE:
            payload_size = 1; pos += 1
        elif evt_id < DWORD_BASE:
            payload_size = 2; pos += 2
        elif evt_id == ID_FL26_FIVE_BYTE and fl_major >= 26:
            payload_size = FL26_FIVE_BYTE_SIZE; pos += FL26_FIVE_BYTE_SIZE
        elif evt_id < TEXT_BASE:
            payload_size = 4; pos += 4
        else:
            length, new_pos = _read_varint(data, pos)
            payload_size = length
            payload_start = new_pos
            pos = new_pos + length
        payload = data[payload_start:payload_start + payload_size]

        if evt_id == ID_FL_VERSION:
            try:
                ver = payload.decode("ascii", errors="replace").rstrip("\0")
                result.fl_version = ver
                parts = ver.split(".")
                major = int(parts[0])
                fl_major = major
                minor = int(parts[1]) if len(parts) > 1 else 0
                is_unicode = (major >= 12) or (major == 11 and minor >= 5)
            except Exception:
                pass

        elif evt_id == ID_CHANNEL_NEW:
            cur_ch = struct.unpack("<H", payload)[0]
            channels.setdefault(cur_ch, {"name": None, "sample_path": None,
                                          "default_name": None, "color": None})

        elif evt_id == ID_CHANNEL_NAME and cur_ch is not None:
            channels[cur_ch]["name"] = _decode_str(payload, is_unicode)

        elif evt_id == ID_CHANNEL_SAMPLE and cur_ch is not None:
            channels[cur_ch]["sample_path"] = _decode_str(payload, is_unicode)

        elif evt_id == ID_DEFAULT_NAME and cur_ch is not None:
            if not channels[cur_ch].get("default_name"):
                channels[cur_ch]["default_name"] = _decode_str(payload, is_unicode)

        elif evt_id == ID_CHANNEL_COLOR and cur_ch is not None:
            # Decode 4-byte payload (R, G, B, alpha) into canonical 0xRRGGBB
            channels[cur_ch]["color"] = _decode_color_payload(payload)
            # Remember offset so the writer can update the channel colour
            channels[cur_ch]["color_offset"] = payload_start

        elif evt_id == ID_PATTERN_NEW:
            cur_pat = struct.unpack("<H", payload)[0]
            patterns.setdefault(cur_pat, {"name": None, "color": None,
                                           "color_offset": None,
                                           "insert_color_at": None})
            # Remember the byte right after this PATTERN_NEW event so a
            # missing PATTERN_COLOR event can be inserted there.
            # PATTERN_NEW (id 65) is a WORD-size event: 1 byte id + 2 bytes payload.
            patterns[cur_pat]["insert_color_at"] = pos  # pos is already past the payload

        elif evt_id == ID_PATTERN_NAME and cur_pat is not None:
            patterns[cur_pat]["name"] = _decode_str(payload, is_unicode)

        elif evt_id == ID_PATTERN_COLOR and cur_pat is not None:
            patterns[cur_pat]["color"] = _decode_color_payload(payload)
            patterns[cur_pat]["color_offset"] = payload_start
            # An existing color event makes the insert position obsolete
            patterns[cur_pat]["insert_color_at"] = None

        elif evt_id == ID_ARRANGEMENT_NEW:
            iid = struct.unpack("<H", payload)[0]
            cur_arr = {
                "iid": iid, "name": None, "playlist_offset": None,
                "playlist_size": 0, "item_size": 0,
            }
            arrangements.append(cur_arr)
            cur_track_idx = -1

        elif evt_id == ID_ARRANGEMENT_NAME and cur_arr is not None:
            cur_arr["name"] = _decode_str(payload, is_unicode)

        elif evt_id == ID_PLAYLIST and cur_arr is not None:
            cur_arr["playlist_offset"] = payload_start
            cur_arr["playlist_size"] = payload_size
            detected = _detect_item_size(data, payload_start, payload_size)
            cur_arr["item_size"] = detected or 0
            if detected is None:
                result.warnings.append(
                    f"Could not detect playlist item size for arrangement '{cur_arr['name']}'."
                )

        elif evt_id == ID_TRACK_DATA:
            cur_track_idx += 1
            # Record the original "enabled" byte (1=active, 0=muted), colour,
            # and the offset of the preceding "colour override enabled" flag
            # (event ID 43). The writer uses these to restore mute state,
            # apply auto-colour, etc.
            if payload_size > TRACK_DATA_MUTE_OFFSET:
                info = {
                    "mute_off": payload_start + TRACK_DATA_MUTE_OFFSET,
                    "mute_val": data[payload_start + TRACK_DATA_MUTE_OFFSET],
                    "color_off": payload_start + TRACK_DATA_COLOR_OFFSET,
                    "color_val": (data[payload_start + TRACK_DATA_COLOR_OFFSET] << 16) |
                                 (data[payload_start + TRACK_DATA_COLOR_OFFSET + 1] << 8) |
                                 data[payload_start + TRACK_DATA_COLOR_OFFSET + 2],
                    "override_off": last_43_offset,
                    # The actual end of this TRACK_DATA's payload — varies by FL
                    # Studio version (we've seen both 66- and 70-byte payloads).
                    # Using `pos` here is correct because the parser has just
                    # advanced `pos` to the byte right after the payload.
                    "payload_end": pos,
                    "has_name": False,
                }
                track_info_state[cur_track_idx] = info

        elif evt_id == 43:
            # Byte-sized event. This is the "use custom track colour" flag
            # which precedes each TRACK_DATA. Remember its payload offset so
            # a later writer can flip it to 0x01 to activate custom colours.
            last_43_offset = payload_start

        elif evt_id == ID_TRACK_NAME and cur_arr is not None and cur_track_idx >= 0:
            # Track has a custom name — mark it so auto-rename skips it,
            # otherwise we'd insert a second TRACK_NAME event causing the
            # parser (and FL Studio) to misalign.
            if cur_track_idx in track_info_state:
                track_info_state[cur_track_idx]["has_name"] = True
                # Also record the byte range of this TRACK_NAME so the writer
                # could replace it later (currently we just skip).
                track_info_state[cur_track_idx]["name_range"] = (
                    payload_start - 2,  # back up over varint(1B) + id(1B)
                    pos,
                )

    result.channel_count = len(channels)
    result.pattern_count = len(patterns)
    result.arrangement_count = len(arrangements)

    # Collect clips and compute the plan
    all_clips: list[ClipInfo] = []
    total_tracks = 0

    for arr in arrangements:
        off = arr["playlist_offset"]
        isz = arr["item_size"]
        if off is None or isz <= 0:
            continue
        n = arr["playlist_size"] // isz

        arr_clips: list[ClipInfo] = []
        for i in range(n):
            base = off + i * isz
            position     = struct.unpack_from("<I", data, base + 0)[0]
            pattern_base = struct.unpack_from("<H", data, base + 4)[0]
            item_index   = struct.unpack_from("<H", data, base + 6)[0]
            length       = struct.unpack_from("<I", data, base + 8)[0]
            rvidx        = struct.unpack_from("<H", data, base + 12)[0]
            u1           = bytes(data[base + 16:base + 18])
            u2           = bytes(data[base + 20:base + 24])

            # Skip ghost items and any record that doesn't match the known
            # signature of real playlist items. Real items have fixed "magic"
            # bytes at offsets 16-17 and 20-23.
            if (pattern_base != PATTERN_BASE_VAL
                    or rvidx >= MAX_TRACKS
                    or length > 10_000_000
                    or u1 != b"\x78\x00"
                    or u2 != b"\x40\x64\x80\x80"):
                continue
            name, kind = _group_name(pattern_base, item_index, channels, patterns)
            if name is None:
                continue
            # Look up source colour and source index
            if kind == "channel":
                src_idx = item_index
                src_color = channels.get(item_index, {}).get("color")
            else:
                src_idx = item_index - PATTERN_BASE_VAL
                src_color = patterns.get(src_idx, {}).get("color")
            old_track_1based = (MAX_TRACKS - 1 - rvidx) + 1  # 1-based
            info = track_info_state.get(old_track_1based - 1)
            was_muted_flag = bool(info and info["mute_val"] == 0)
            # Clip-level mute: bit 5 of byte 19
            clip_flags_byte_abs = base + CLIP_ITEM_FLAGS_OFFSET
            clip_flags = data[clip_flags_byte_abs]
            was_clip_muted_flag = bool(clip_flags & CLIP_MUTED_BIT)
            clip = ClipInfo(
                name=name, kind=kind,
                position=position, length=length,
                old_track=old_track_1based,
                color=src_color,
                was_muted=was_muted_flag,
                was_clip_muted=was_clip_muted_flag,
                _source_idx=src_idx,
                item_offset=base,
                rvidx_offset=base + 12,
                old_rvidx=rvidx,
                clip_mute_offset=clip_flags_byte_abs,
            )
            arr_clips.append(clip)

        # Group by name
        # ---------- Split clips into two pools based on original track mute state ----------
        # This is the "preserve muted tracks" feature: clips that came from
        # muted tracks stay in a separate group of tracks at the bottom, which
        # will be set to muted in the output. Clips from active tracks fill
        # the tracks on top (which will be set to active).
        active_clips = [c for c in arr_clips if not c.was_muted]
        muted_clips  = [c for c in arr_clips if c.was_muted]

        def build_groups_and_order(clip_list: list[ClipInfo]):
            g: dict[str, list[ClipInfo]] = defaultdict(list)
            for c in clip_list:
                g[c.name].append(c)

            def _primary(nm: str):
                if sort_mode == SORT_BY_FIRST_APPEARANCE:
                    return min(c.position for c in g[nm])
                return nm.lower()

            if not sub_sort:
                if sort_mode == SORT_BY_FIRST_APPEARANCE:
                    order = sorted(g.keys(),
                                   key=lambda n: min(c.position for c in g[n]))
                else:
                    order = sorted(g.keys(), key=lambda s: s.lower())
                return order, g

            def _grouping_key(nm: str):
                cl = g[nm]
                parts = []
                for modifier in sub_sort:
                    if modifier == SUB_BY_TYPE:
                        parts.append(0 if cl[0].kind == "channel" else 1)
                    elif modifier == SUB_BY_LENGTH:
                        avg_len = sum(c.length for c in cl) / len(cl)
                        parts.append(-avg_len)
                    elif modifier == SUB_BY_COLOR:
                        dom = _dominant_color(cl)
                        parts.append(_rgb_to_hue_key(dom))
                parts.append(_primary(nm))
                return tuple(parts)
            return sorted(g.keys(), key=_grouping_key), g

        active_order, active_groups = build_groups_and_order(active_clips)
        muted_order,  muted_groups  = build_groups_and_order(muted_clips)

        # Build a map from destination track (0-based) -> dominant color and group name
        # Active zone first, then muted zone. Also remember which was muted.
        dest_track_info: dict[int, dict] = {}
        base_track_0 = 0
        muted_zone_start = None

        def _lay_out(order, groups_dict, muted_flag: bool):
            nonlocal base_track_0
            for name in order:
                group_clips = groups_dict[name]
                first_track_0 = base_track_0
                used = _assign_lanes(group_clips, base_track_0)
                dom = _dominant_color(group_clips)
                # Associate color/name with each destination track (lanes)
                for lane_0 in range(first_track_0, first_track_0 + used):
                    dest_track_info[lane_0] = {
                        "name": name,
                        "color": dom,
                        "muted": muted_flag,
                    }
                result.groups.append(GroupPlan(
                    name=(f"[muted] {name}" if muted_flag else name),
                    first_track=first_track_0 + 1,
                    lanes_used=used,
                    clip_count=len(group_clips),
                ))
                base_track_0 += used

        # Lay out active pool first
        _lay_out(active_order, active_groups, muted_flag=False)
        # Remember boundary
        muted_zone_start = base_track_0 if muted_order else None
        # Lay out muted pool right after
        _lay_out(muted_order, muted_groups, muted_flag=True)

        last_used_track_0 = base_track_0   # exclusive

        # ---------- Compute per-track patches ----------
        # For each destination track index we know:
        #   - Whether it's in the active zone or muted zone
        #   - Which group (if any) lives on it, and thus its dominant colour
        # We write:
        #   1. The "enabled" byte (mute) -> restore correct state
        #   2. The 3 colour bytes + the override flag -> auto-colour (opt-in)
        for track_0, info in track_info_state.items():
            dest = dest_track_info.get(track_0)
            # --- mute byte ---
            if dest is not None:
                new_mute = 0x00 if dest["muted"] else 0x01
            else:
                # Track is unoccupied in the new layout — reset to active
                new_mute = 0x01
            if new_mute != data[info["mute_off"]]:
                result._mute_patches.append((info["mute_off"], new_mute))

            # --- track colour bytes are NOT written anymore ---
            # FL Studio doesn't reliably honour the per-track colour bytes
            # we used to write here (the override flag isn't always present
            # for the very first track). Auto-colour now operates on the
            # source channels/patterns instead — see the block below.

        # ---------- Auto-color (rainbow): paint each group's source channels/patterns ----------
        # Strategy:
        #   1. Order groups in the same order as result.groups (organized order).
        #   2. Assign a rainbow hue to each group based on its position.
        #   3. For every channel/pattern that contributes a clip to that group,
        #      overwrite its ID_CHANNEL_COLOR (event 128) or ID_PATTERN_COLOR
        #      (event 149) with the rainbow colour — but ONLY if it's currently
        #      a default/unset colour, so user-set custom colours are preserved.
        if apply_auto_color and result.groups:
            # Map group_name -> set of (kind, item_index) sources
            group_sources: dict[str, set[tuple[str, int]]] = defaultdict(set)
            # Map (kind, item_index) -> the parsed channels/patterns dict info
            for cl in arr_clips:
                # Reverse-engineer the source: clips with kind == 'channel' map
                # to a channel id; 'pattern' clips map to (item_index - 0x5000)
                # which is the pattern's logical id. We don't have item_index
                # cached here easily, but we DO have the source colour and the
                # channels/patterns dicts from the parser.
                pass  # We fill this map below using a different approach

            # Simpler: walk the clips again and use channel/pattern dicts
            # directly. Note: we need to know whether a clip name corresponds
            # to a channel or a pattern; ClipInfo.kind tells us that.
            for cl in arr_clips:
                # We need the source id. Reconstruct it from clip context:
                # in the original parsing the source was identified by either
                # the channel id (item_index) or pattern id. ClipInfo doesn't
                # carry item_index, so we infer from kind+name+color.
                # The simpler path: any channel/pattern whose colour matches
                # cl.color belongs to this group's potential sources. But we
                # actually want the *exact* source. Use: cl.kind tells channel
                # vs pattern, and we match by colour OR fall back to "any".
                # For correctness, store item_index in ClipInfo (next step).
                pass

            # Use the pre-stored mapping: see below where we set
            # cl._source_idx during parsing.
            # Strip the [muted] prefix to match against arr_clips names
            ordered_group_names = []
            for g in result.groups:
                clean_name = g.name[len("[muted] "):] if g.name.startswith("[muted] ") else g.name
                ordered_group_names.append(clean_name)
            total_groups = len(ordered_group_names)

            # Collect source ids per group name
            for cl in arr_clips:
                if hasattr(cl, "_source_idx") and cl._source_idx is not None:
                    group_sources[cl.name].add((cl.kind, cl._source_idx))

            for group_pos, gname in enumerate(ordered_group_names):
                rainbow = _rainbow_color_for(group_pos, total_groups)
                R, G, B = (rainbow >> 16) & 0xFF, (rainbow >> 8) & 0xFF, rainbow & 0xFF
                for kind, src_idx in group_sources.get(gname, set()):
                    if kind == "channel":
                        ch_info = channels.get(src_idx, {})
                        cur = ch_info.get("color")
                        off = ch_info.get("color_offset")
                        if off is None:
                            continue   # Channel never had a colour event we can patch
                        if cur is not None and not _is_default_color(cur):
                            continue   # User picked a custom colour — preserve it
                        # Write 4 bytes (R,G,B,0x00) at off
                        if data[off] != R:
                            result._color_patches.append((off, R))
                        if data[off + 1] != G:
                            result._color_patches.append((off + 1, G))
                        if data[off + 2] != B:
                            result._color_patches.append((off + 2, B))
                        # 4th byte stays 0x00 (alpha)
                    else:  # pattern
                        pat_info = patterns.get(src_idx, {})
                        cur = pat_info.get("color")
                        off = pat_info.get("color_offset")
                        if off is not None:
                            # Pattern has an existing PATTERN_COLOR event we can patch
                            if cur is not None and not _is_default_color(cur):
                                continue
                            if data[off] != R:
                                result._color_patches.append((off, R))
                            if data[off + 1] != G:
                                result._color_patches.append((off + 1, G))
                            if data[off + 2] != B:
                                result._color_patches.append((off + 2, B))
                        else:
                            # Pattern has no PATTERN_COLOR event — insert one
                            insert_at = pat_info.get("insert_color_at")
                            if insert_at is None:
                                continue
                            # Build PATTERN_COLOR event:
                            #   1 byte  = 0x96 (150) event id
                            #   4 bytes = R, G, B, alpha (alpha=0)
                            evt = bytes([ID_PATTERN_COLOR, R, G, B, 0])
                            result._color_inserts.append((insert_at, evt))

        # ---------- Auto-rename: insert ID_TRACK_NAME events after TRACK_DATA ----------
        # Event 239 (ID_TRACK_NAME) is a TEXT-type event. Encoding:
        #   1 byte  = 0xEF  (239)
        #   varint  = payload length (in bytes)
        #   payload = UTF-16LE encoded string, null-terminated (0x00 0x00)
        # We insert one such event after the TRACK_DATA of each track that
        # has a destination group, using the group name as the new track name.
        if apply_auto_rename:
            for track_0, info in track_info_state.items():
                dest = dest_track_info.get(track_0)
                if dest is None:
                    continue
                # Skip if the track already has a name event — inserting a
                # second one would corrupt the file structure (FL Studio
                # parser expects exactly one TRACK_NAME per TRACK_DATA).
                if info.get("has_name"):
                    continue
                # Strip any "coming soon" prefixes like "[muted]"
                new_name = dest["name"]
                # Clip to a reasonable length so FL Studio doesn't choke
                new_name = new_name[:120]
                # UTF-16LE encode + null terminator (2 bytes of 0x00)
                name_bytes = new_name.encode("utf-16-le") + b"\x00\x00"
                # Build event: 0xEF + varint_len + name_bytes
                evt_bytes = bytearray([ID_TRACK_NAME])
                # Varint encoding of length
                n = len(name_bytes)
                while True:
                    byte = n & 0x7F
                    n >>= 7
                    if n:
                        evt_bytes.append(byte | 0x80)
                    else:
                        evt_bytes.append(byte)
                        break
                evt_bytes.extend(name_bytes)
                # Insert position: right after the TRACK_DATA payload ends.
                # Use the actual payload size we recorded during parsing —
                # it's not always 70 bytes (older/newer FL versions vary).
                insert_at = info["payload_end"]
                result._name_inserts.append((insert_at, bytes(evt_bytes)))

        if base_track_0 > MAX_TRACKS:
            result.warnings.append(
                f"Arrangement '{arr['name']}' needs {base_track_0} tracks "
                f"but the playlist has only {MAX_TRACKS}. Some clips will not be moved."
            )

        # Build patches: one per clip where track changed
        for c in arr_clips:
            new_track_0 = c.new_track - 1
            new_rvidx = MAX_TRACKS - 1 - new_track_0
            if new_rvidx != c.old_rvidx and 0 <= new_track_0 < MAX_TRACKS:
                result._patches.append((c.rvidx_offset, new_rvidx))

        total_tracks = max(total_tracks, base_track_0)
        all_clips.extend(arr_clips)

    result.total_clips = len(all_clips)
    result.total_tracks_needed = total_tracks
    result.clips = all_clips
    return result


def apply_plan(result: AnalysisResult, output_path: Path | str,
               progress: Optional[Callable[[int, int], None]] = None) -> None:
    """Writes the reorganized file using the patches computed by analyze()."""
    out = bytearray(result._data)
    total = (len(result._patches) + len(result._mute_patches)
             + len(result._color_patches) + len(result._color_inserts)
             + len(result._name_inserts))
    step = 0

    # 1) Clip-to-track index patches
    for offset, new_rvidx in result._patches:
        struct.pack_into("<H", out, offset, new_rvidx)
        step += 1
        if progress and (step % 50 == 0 or step == total):
            progress(step, total)

    # 2) Track mute-state patches (preserve muted tracks)
    for offset, new_value in result._mute_patches:
        out[offset] = new_value
        step += 1
        if progress and (step % 20 == 0 or step == total):
            progress(step, total)

    # 3) Track colour patches (auto-colour)
    for offset, new_value in result._color_patches:
        out[offset] = new_value
        step += 1
        if progress and (step % 20 == 0 or step == total):
            progress(step, total)

    # 4) Inserts (auto-color new pattern color events + auto-rename track names)
    # These GROW the file; we apply them in reverse offset order so that
    # earlier offsets remain valid while we splice.
    all_inserts = list(result._color_inserts) + list(result._name_inserts)
    if all_inserts:
        # Build a new bytearray by splicing in the new events in reverse order
        inserts_sorted = sorted(all_inserts, key=lambda x: x[0], reverse=True)
        total_inserted = sum(len(b) for _, b in inserts_sorted)
        for insert_at, payload in inserts_sorted:
            out[insert_at:insert_at] = payload
            step += 1
            if progress and (step % 20 == 0 or step == total):
                progress(step, total)
        # Update the FLdt chunk size in the header so FL Studio knows the new
        # data length. FLdt size is a little-endian uint32 at offset 4 of the
        # FLdt chunk header (which itself starts after the FLhd chunk).
        pos = 0
        assert out[:4] == b'FLhd'
        hdr_size = struct.unpack_from("<I", out, 4)[0]
        fldt_pos = 8 + hdr_size
        assert out[fldt_pos:fldt_pos+4] == b'FLdt'
        old_size = struct.unpack_from("<I", out, fldt_pos + 4)[0]
        struct.pack_into("<I", out, fldt_pos + 4, old_size + total_inserted)

    Path(output_path).write_bytes(bytes(out))


def reorganize(flp_path: Path | str, output_path: Path | str,
               progress: Optional[Callable[[int, int], None]] = None,
               sort_mode: str = SORT_ALPHABETICAL,
               sub_sort: Optional[list[str]] = None,
               apply_auto_color: bool = False,
               apply_auto_rename: bool = False) -> AnalysisResult:
    """One-shot: analyze + write. Returns the AnalysisResult."""
    result = analyze(flp_path, sort_mode=sort_mode, sub_sort=sub_sort,
                     apply_auto_color=apply_auto_color,
                     apply_auto_rename=apply_auto_rename)
    apply_plan(result, output_path, progress=progress)
    return result
