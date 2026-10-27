#!/usr/bin/env python3
"""Render the tower's art and check it.

    python3 art/tools/render.py [--scales 4,8,16] [--no-sheets] [--strict]

For every SVG under art/src:

  * render it with resvg at each scale (pixels a unit; a cell is 4 units
    wide, a floor 16 units tall) into art/out/<scale>/<same path>.png;
  * check its viewBox against the slot it fills (the SLOTS table, or a
    `<!-- slot: WxH -->` / `<!-- units: WxH -->` comment in the file);
  * report any <text> (words are drawn by the program, never baked in);
  * check a .emit.svg layer has the same viewBox as its day picture.

Then it writes art/out/contact-<scale>.png, one sheet a scale (and
contact-<scale>-<directory>.png, one a top-level directory), grouped by
directory and labelled by file name.  Emission layers are shown on a
night-blue ground so they can be seen, each followed by a night preview
(the day picture darkened, the emission screened over it).  The sheets are composed as SVG and
rendered by resvg, so nothing beyond Python and resvg is needed.

Exit status is 1 if any check failed (with --strict, warnings count too).
"""

import argparse
import os
import shutil
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from xml.sax.saxutils import escape

RESVG = os.environ.get("RESVG") or shutil.which("resvg") or os.path.expanduser("~/.cargo/bin/resvg")
ART = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ART, "src")
OUT = os.path.join(ART, "out")

CELL = 4    # units a cell
FLOOR = 16  # units a floor

# Slot sizes, in cells x floors, keyed by the base name of the file (the
# part before the first dot: `hotel-single.1.dirty.svg` -> `hotel-single`).
# A key may be qualified by its directory (`rooms/lobby`) when the same
# base name means different things in different directories.  Widths are
# TILE_WIDTHS in sim/src/resources.rs; heights are the stacks recorded in
# sim/src/commands.rs (cinema and party hall 2, recycling centre 2 as an
# upper and lower slice, metro 3, cathedral 5).
SLOTS = {
    # rooms
    "office": (9, 1),
    "condo": (16, 1),
    "hotel-single": (4, 1),
    "hotel-twin": (6, 1),
    "hotel-suite": (10, 1),
    "restaurant": (24, 1),
    "fast-food": (16, 1),
    "retail": (12, 1),
    "cinema": (31, 2),
    "party-hall": (27, 2),
    "cathedral": (28, 5),
    "recycling": (25, 2),
    "parking": (4, 1),
    "parking-ramp": (15, 1),
    "security": (16, 1),
    "metro": (30, 3),
    "housekeeping": (15, 1),
    "medical": (26, 1),
    # BOX RULES' amenities (sim box_ext::amenities)
    "laundry": (12, 1),
    "guest-services": (10, 1),
    "premier-lounge": (20, 1),
    "pool": (30, 1),
    "gym": (16, 1),
    "convention": (30, 2),
    "kindergarten": (12, 1),
    "school": (30, 2),
    "sprinklers": (8, 1),
    # The lobby is one cell a tile but is painted as a repeating
    # 8-cell strip (upstream's texture is 2 floor-heights wide).
    "lobby": (8, 1),
    # transport
    "stairs": (8, 1),
    "escalator": (8, 1),
    "elevator": (4, 1),
    "elevator-express": (6, 1),
    "elevator-service": (4, 1),
    # building
    "floor": (1, 1),
}

# Sprites that are not slots, in units (width x height).
UNITS = {
    # upstream draws a cockroach 0.55 of a cell wide, 10:8
    "cockroach": (2.5, 2),
}

SLOT_RE = re.compile(r"<!--\s*slot:\s*([\d.]+)\s*x\s*([\d.]+)\s*-->")
UNITS_RE = re.compile(r"<!--\s*units:\s*([\d.]+)\s*x\s*([\d.]+)\s*-->")
VIEWBOX_RE = re.compile(r'<svg\b[^>]*?\bviewBox="([^"]*)"', re.S)
TEXT_RE = re.compile(r"<(text|tspan|textPath)\b")
COMMENT_RE = re.compile(r"<!--.*?-->", re.S)
PAR_NONE_RE = re.compile(r'preserveAspectRatio="none"')


def base_name(rel):
    stem = os.path.basename(rel)[:-4]
    return stem.split(".")[0]


def is_emit(rel):
    return rel.endswith(".emit.svg")


def expected_size(rel, text):
    """(w, h, why) in units, or None if nothing says what it should be."""
    m = SLOT_RE.search(text)
    if m:
        c, f = float(m.group(1)), float(m.group(2))
        return c * CELL, f * FLOOR, "slot comment %gx%g" % (c, f)
    m = UNITS_RE.search(text)
    if m:
        return float(m.group(1)), float(m.group(2)), "units comment"
    b = base_name(rel)
    d = os.path.dirname(rel)
    for key in (d + "/" + b, b):
        if key in SLOTS:
            c, f = SLOTS[key]
            return c * CELL, f * FLOOR, "%d cells x %d floors" % (c, f)
        if key in UNITS:
            w, h = UNITS[key]
            return w, h, "sprite %gx%g units" % (w, h)
    return None


def parse_viewbox(text):
    m = VIEWBOX_RE.search(text)
    if not m:
        return None
    try:
        x, y, w, h = [float(v) for v in m.group(1).replace(",", " ").split()]
    except ValueError:
        return None
    return x, y, w, h


def collect():
    files = []
    for root, dirs, names in os.walk(SRC):
        dirs.sort()
        # each day picture first, its night layer straight after it
        for n in sorted(names, key=lambda n: (n.replace(".emit.svg", ".svg"), n.endswith(".emit.svg"))):
            if n.endswith(".svg"):
                files.append(os.path.relpath(os.path.join(root, n), SRC))
    return files


def check(files):
    errors, warnings = [], []
    sizes = {}
    for rel in files:
        with open(os.path.join(SRC, rel), encoding="utf-8") as fh:
            text = fh.read()
        bare = COMMENT_RE.sub("", text)  # checks look past comments
        vb = parse_viewbox(bare)
        if vb is None:
            errors.append("%s: no viewBox" % rel)
            continue
        x, y, w, h = vb
        sizes[rel] = (w, h)
        if x != 0 or y != 0:
            errors.append("%s: viewBox does not start at 0 0" % rel)
        if TEXT_RE.search(bare):
            errors.append("%s: contains <text> (signs are shapes, words are the program's)" % rel)
        if PAR_NONE_RE.search(bare):
            errors.append('%s: preserveAspectRatio="none" (draw to the slot instead)' % rel)
        exp = expected_size(rel, text)
        if exp is None:
            warnings.append("%s: no slot known for '%s' (add it to SLOTS or put <!-- slot: WxH --> in the file); viewBox %gx%g"
                            % (rel, base_name(rel), w, h))
        elif abs(exp[0] - w) > 1e-6 or abs(exp[1] - h) > 1e-6:
            errors.append("%s: viewBox %gx%g, its slot (%s) needs %gx%g" % (rel, w, h, exp[2], exp[0], exp[1]))
    for rel in files:
        if is_emit(rel):
            day = rel[: -len(".emit.svg")] + ".svg"
            if day not in sizes:
                errors.append("%s: no day picture %s" % (rel, day))
            elif rel in sizes and sizes[rel] != sizes[day]:
                errors.append("%s: viewBox differs from %s" % (rel, day))
    return sizes, errors, warnings


def render_one(rel, size, scale):
    w, h = size
    out = os.path.join(OUT, str(scale), rel[:-4] + ".png")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    pw, ph = max(1, round(w * scale)), max(1, round(h * scale))
    r = subprocess.run([RESVG, "-w", str(pw), "-h", str(ph), os.path.join(SRC, rel), out],
                       capture_output=True, text=True)
    if r.returncode != 0:
        return "%s @%d: resvg failed: %s" % (rel, scale, r.stderr.strip())
    return None


def contact_sheet(files, sizes, scale, name=None):
    """Compose an SVG sheet of the PNGs at `scale` and render it."""
    pad, label_h, head_h = 12, 18, 30
    max_w = max(1600, max(round(sizes[r][0] * scale) for r in files) + 2 * pad)
    groups = {}
    for rel in files:
        groups.setdefault(os.path.dirname(rel) or ".", []).append(rel)
    parts, y = [], pad
    for g in sorted(groups):
        parts.append('<text x="%d" y="%d" font-family="Helvetica, Arial, sans-serif" font-size="20" '
                     'font-weight="bold" fill="#222">%s</text>' % (pad, y + 20, escape(g)))
        y += head_h
        x, row_h = pad, 0
        for rel in groups[g]:
            w = round(sizes[rel][0] * scale)
            h = round(sizes[rel][1] * scale)
            cw = max(w, 7 * (len(os.path.basename(rel)) + (4 if is_emit(rel) else 0)))  # room for the label
            if x + cw + pad > max_w and x > pad:
                x, y = pad, y + row_h + pad
                row_h = 0
            png = os.path.join(OUT, str(scale), rel[:-4] + ".png")
            ground = "#1b2340" if is_emit(rel) else "#ffffff"
            parts.append('<rect x="%d" y="%d" width="%d" height="%d" fill="%s" stroke="#bbb" stroke-width="1"/>'
                         % (x - 1, y - 1, w + 2, h + 2, ground))
            parts.append('<image x="%d" y="%d" width="%d" height="%d" href="%s"/>'
                         % (x, y, w, h, escape(png)))
            parts.append('<text x="%d" y="%d" font-family="Helvetica, Arial, sans-serif" font-size="11" '
                         'fill="#333">%s</text>' % (x, y + h + 13, escape(os.path.basename(rel)[:-4])))
            x += cw + pad
            row_h = max(row_h, h + label_h)
            if is_emit(rel) and rel[: -len(".emit.svg")] + ".svg" in sizes:
                # A night preview: the day picture darkened, the emission added.
                if x + cw + pad > max_w and x > pad:
                    x, y = pad, y + row_h + pad
                    row_h = 0
                day = os.path.join(OUT, str(scale), rel[: -len(".emit.svg")] + ".png")
                parts.append('<g><image x="%d" y="%d" width="%d" height="%d" href="%s"/>'
                             '<rect x="%d" y="%d" width="%d" height="%d" fill="#0a1030" opacity="0.72"/>'
                             '<image x="%d" y="%d" width="%d" height="%d" href="%s" style="mix-blend-mode:screen"/></g>'
                             % (x, y, w, h, escape(day), x, y, w, h, x, y, w, h, escape(png)))
                parts.append('<text x="%d" y="%d" font-family="Helvetica, Arial, sans-serif" font-size="11" '
                             'fill="#333">%s</text>' % (x, y + h + 13, escape(os.path.basename(rel)[:-9] + " (night)")))
                x += cw + pad
                row_h = max(row_h, h + label_h)
        y += row_h + pad
    total_h = y + pad
    svg = ('<svg xmlns="http://www.w3.org/2000/svg" width="%d" height="%d" viewBox="0 0 %d %d">'
           '<rect width="100%%" height="100%%" fill="#e8e8e4"/>%s</svg>'
           % (max_w, total_h, max_w, total_h, "".join(parts)))
    stem = "contact-%d" % scale + ("-" + name if name else "")
    sheet_svg = os.path.join(OUT, stem + ".svg")
    sheet_png = os.path.join(OUT, stem + ".png")
    with open(sheet_svg, "w", encoding="utf-8") as fh:
        fh.write(svg)
    r = subprocess.run([RESVG, "--resources-dir", OUT, sheet_svg, sheet_png], capture_output=True, text=True)
    if r.returncode != 0:
        return "contact sheet @%d: resvg failed: %s" % (scale, r.stderr.strip())
    return None


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--scales", default="4,8,16")
    ap.add_argument("--no-sheets", action="store_true")
    ap.add_argument("--strict", action="store_true", help="warnings fail too")
    ap.add_argument("--only", help="only files whose path contains this")
    args = ap.parse_args()
    scales = [int(s) for s in args.scales.split(",") if s]

    files = collect()
    if args.only:
        files = [f for f in files if args.only in f]
    if not files:
        print("no SVGs under", SRC)
        return 0
    sizes, errors, warnings = check(files)
    good = [f for f in files if f in sizes]
    jobs = [(rel, sizes[rel], s) for s in scales for rel in good]
    with ThreadPoolExecutor(max_workers=os.cpu_count() or 4) as ex:
        for msg in ex.map(lambda j: render_one(*j), jobs):
            if msg:
                errors.append(msg)
    if not args.no_sheets:
        dirs = sorted({rel.split(os.sep)[0] for rel in good if os.sep in rel})
        for s in scales:
            sheets = [(good, None)] + [([r for r in good if r.startswith(d + os.sep)], d) for d in dirs]
            for subset, name in sheets:
                msg = contact_sheet(subset, sizes, s, name)
                if msg:
                    errors.append(msg)
    for w in warnings:
        print("warning:", w)
    for e in errors:
        print("error:", e)
    print("%d SVGs, %d rendered at %s px/unit, %d errors, %d warnings%s"
          % (len(files), len(good), ",".join(map(str, scales)), len(errors), len(warnings),
             "" if args.no_sheets else "; sheets in %s/contact-<scale>[-<dir>].png" % os.path.relpath(OUT)))
    return 1 if errors or (args.strict and warnings) else 0


if __name__ == "__main__":
    sys.exit(main())
