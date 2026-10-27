#!/usr/bin/env python3
"""Draw the tower's signage for one language, or all of them.

    python3 art/src/signs/gen.py            # every <lang>.json here
    python3 art/src/signs/gen.py en fr      # just these

For a language table `<lang>.json` (sign key -> words) this writes
art/src/signs/<lang>/<file>.svg, plus `.emit.svg` for signs lit at night,
and prints how each sign's words fitted (cap height, squeeze).  The words
are drawn by letters.py, our own lettering; nothing here is a font.

A sign's style, size and colours are in SIGNS below and are the same in
every language: a translation only changes the words, which are condensed
or made smaller to fit the plate.  NOTES.md says where each sign goes.

Units: a cell is 4 wide, a floor 16 tall, y down.
"""

import json
import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import letters  # noqa: E402

CELL, FLOOR = 4, 16

# Colours (STYLE.md families, and its emission colours)
CLOTH = "#f0f0e8"
RED, RED_DK = "#d04040", "#8a2030"
BLUE, BLUE_DK = "#2a5090", "#1a3466"
GREEN, GREEN_DK = "#3a7a3a", "#265426"
GOLD = "#d0a848"
HAZARD, HAZARD_DK = "#d8c040", "#2a2a2a"
CORD = "#4a4448"
NEON_BOARD, NEON_FRAME = "#1c1418", "#3a3038"


def f(v):
    s = ("%.3f" % v).rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def rect(x, y, w, h, fill, rx=None, op=None, extra=""):
    a = ' rx="%s"' % f(rx) if rx else ""
    o = ' opacity="%s"' % f(op) if op is not None else ""
    return '<rect x="%s" y="%s" width="%s" height="%s" fill="%s"%s%s%s/>' % (f(x), f(y), f(w), f(h), fill, a, o, extra)


def circ(cx, cy, r, fill, op=None):
    o = ' opacity="%s"' % f(op) if op is not None else ""
    return '<circle cx="%s" cy="%s" r="%s" fill="%s"%s/>' % (f(cx), f(cy), f(r), fill, o)


def line(x1, y1, x2, y2, colour, w=0.25):
    return ('<line x1="%s" y1="%s" x2="%s" y2="%s" stroke="%s" stroke-width="%s" stroke-linecap="round"/>'
            % (f(x1), f(y1), f(x2), f(y2), colour, f(w)))


def poly(pts, fill, op=None):
    o = ' opacity="%s"' % f(op) if op is not None else ""
    return '<polygon points="%s" fill="%s"%s/>' % (" ".join("%s,%s" % (f(x), f(y)) for x, y in pts), fill, o)


def plate(x, y, w, h, face, border, b=0.45, rx=0.7):
    """A flat plate with a darker border (STYLE.md: Sign)."""
    return [rect(x, y, w, h, border, rx), rect(x + b, y + b, w - 2 * b, h - 2 * b, face, max(0.1, rx - b))]


def shadow(x, y, w, h, rx=0.7):
    return rect(x + 0.35, y + 0.45, w, h, "#000000", rx, 0.16)


def doc(w, h, body, size_comment, key, lang, words):
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %s %s">\n'
            '  <!-- Drawn for the BOX port of tower-together by art/src/signs/gen.py; letters by letters.py. -->\n'
            '  <!-- %s -->\n'
            '  <!-- sign %s (%s): %s -->\n'
            '%s\n</svg>\n'
            % (f(w), f(h), size_comment, key, lang, words.replace("--", "- -").replace("\n", " / "),
               "\n".join("  " + b for b in body if b)))


def split2(text):
    """Two lines from one: an explicit newline, else the most even space."""
    if "\n" in text:
        return text.split("\n")[:2]
    sp = [i for i, c in enumerate(text) if c == " "]
    if not sp:
        return [text]
    i = min(sp, key=lambda i: abs(len(text[:i]) - len(text[i + 1:])))
    return [text[:i], text[i + 1:]]


def one(text):
    return [text.replace("\n", " ")]


# --- styles --------------------------------------------------------------
# Each returns (width, height, size_comment, day_body, emit_body or None, block)

def banner(words, cells, cloth, hem, ink):
    """A cloth banner hung across a room: the whole room slot, transparent
    but for the banner.  4 cells: a two-line placard; 6: a band lying
    diagonally; 9 and wider: a band straight across."""
    W, H = cells * CELL, FLOOR
    body = []
    if cells <= 4:
        pw, ph = W - 3, 10.6
        px, py = (W - pw) / 2, 2.6
        blk = letters.fit(split2(words), pw - 2.0, ph - 2.4, 3.6, "bold", gap=0.3, spill=0.5)
        body += [line(W / 2, 0.2, px + 1.2, py + 0.3, CORD), line(W / 2, 0.2, px + pw - 1.2, py + 0.3, CORD),
                 circ(W / 2, 0.4, 0.45, CORD), shadow(px, py, pw, ph)]
        body += plate(px, py, pw, ph, cloth, hem, b=0.7, rx=0.6)
        body.append(blk.svg(px + 1.0, py + 1.2, ink))
        return W, H, "slot: %dx1" % cells, body, None, blk
    if cells <= 6:
        L, bh, ang = 21.0, 5.8, -24.0
        cx, cy = W / 2, 7.8
        blk = letters.fit(one(words), L - 3.0, bh - 1.8, 3.4, "bold", spill=0.4)
        a = math.radians(ang)

        def rot(x, y):
            return (cx + x * math.cos(a) - y * math.sin(a), cy + x * math.sin(a) + y * math.cos(a))
        tl, tr = rot(-L / 2 + 1.0, -bh / 2), rot(L / 2 - 1.0, -bh / 2)
        body += [line(tl[0], 0, tl[0], tl[1] + 0.3, CORD), line(tr[0], 0, tr[0], tr[1] + 0.3, CORD)]
        g = ['<g transform="translate(%s %s) rotate(%s)">' % (f(cx), f(cy), f(ang))]
        g.append(shadow(-L / 2, -bh / 2, L, bh, 0.4))
        g += plate(-L / 2, -bh / 2, L, bh, cloth, hem, b=0.6, rx=0.4)
        g.append(blk.svg(-L / 2 + 1.5, -bh / 2 + 0.9, ink))
        g.append("</g>")
        body += g
        return W, H, "slot: %dx1" % cells, body, None, blk
    bx, bw, by, bh = 1.6, W - 3.2, 3.0, 7.0
    blk = letters.fit(one(words), bw - 4.0, bh - 2.0, 3.8, "regular" if cells >= 10 else "bold", spill=0.5)
    body += [line(bx + 1.2, 0, bx + 1.2, by + 0.4, CORD), line(bx + bw - 1.2, 0, bx + bw - 1.2, by + 0.4, CORD),
             shadow(bx, by, bw, bh, 0.4)]
    body += plate(bx, by, bw, bh, cloth, hem, b=0.6, rx=0.4)
    # swallow-tail notches at the ends, cut back to the wall colour behind:
    # drawn as small triangles of the hem so the cloth reads as cloth
    for ex, d in ((bx, 1), (bx + bw, -1)):
        body.append(poly([(ex, by + bh / 2 - 0.9), (ex + d * 1.0, by + bh / 2), (ex, by + bh / 2 + 0.9)], hem))
    body.append(blk.svg(bx + 2.0, by + 1.0, ink))
    return W, H, "slot: %dx1" % cells, body, None, blk


def wall_plate(words, w, h, face, border, ink, cap=3.0, weight="regular", lines=1, mark=None, emit=None):
    """A plate fixed to a wall (mark: a small pictogram at its left)."""
    body = [shadow(0, 0, w - 0.4, h - 0.5)]
    body += plate(0, 0, w - 0.4, h - 0.5, face, border)
    x0, iw = 1.0, w - 0.4 - 2.0
    if mark == "cross":
        cx, cy, s = 1.0 + (h - 0.5) * 0.32, (h - 0.5) / 2, (h - 0.5) * 0.52
        body += [rect(cx - s / 6, cy - s / 2, s / 3, s, RED, 0.15), rect(cx - s / 2, cy - s / 6, s, s / 3, RED, 0.15)]
        x0, iw = cx + s / 2 + 0.7, w - 0.4 - (cx + s / 2 + 0.7) - 1.0
    if mark == "noentry":
        cy, rr = (h - 0.5) / 2, (h - 0.5) * 0.36
        cx = 1.0 + rr
        body += [circ(cx, cy, rr, CLOTH), rect(cx - rr * 0.65, cy - rr * 0.2, rr * 1.3, rr * 0.4, RED, 0.1)]
        x0, iw = cx + rr + 0.7, w - 0.4 - (cx + rr + 0.7) - 1.0
    texts = split2(words) if lines == 2 else one(words)
    blk = letters.fit(texts, iw, h - 0.5 - 1.5, cap, weight, gap=0.32, spill=0.45)
    body.append(blk.svg(x0, 0.75, ink))
    em = None
    if emit:
        em = [rect(0.45, 0.45, w - 1.3, h - 1.4, emit[0], 0.3, 0.45), blk.svg(x0, 0.75, emit[1])]
    return w, h, "units: %gx%g" % (w, h), body, em, blk


def hazard_plate(words, w, h):
    """Yellow and black hazard border round a pale plate: OUT OF ORDER."""
    body = [shadow(0, 0, w - 0.4, h - 0.5), rect(0, 0, w - 0.4, h - 0.5, HAZARD_DK, 0.5)]
    pw, ph = w - 0.4, h - 0.5
    clip = "hz%d%d" % (w, h)
    body.append('<clipPath id="%s"><rect x="0.2" y="0.2" width="%s" height="%s" rx="0.4"/></clipPath>' % (clip, f(pw - 0.4), f(ph - 0.4)))
    stripes = ['<g clip-path="url(#%s)">' % clip]
    x = -ph
    while x < pw + ph:
        stripes.append(poly([(x, ph), (x + 1.2, ph), (x + 1.2 + ph, 0), (x + ph, 0)], HAZARD))
        x += 2.4
    stripes.append("</g>")
    body += stripes
    body += [rect(1.0, 1.0, pw - 2.0, ph - 2.0, CLOTH, 0.3)]
    blk = letters.fit(split2(words), pw - 3.0, ph - 3.0, 3.2, "bold", gap=0.3, spill=0.3)
    body.append(blk.svg(1.5, 1.5, HAZARD_DK))
    return w, h, "units: %gx%g" % (w, h), body, None, blk


def hanging(words, face, border, ink, w=14, h=9):
    """A card hung on a door by a cord from a hook: OPEN / CLOSED."""
    py, ph = 2.6, h - 2.6 - 0.4
    body = [line(w / 2, 0.6, 1.6, py + 0.3, CORD, 0.22), line(w / 2, 0.6, w - 1.6, py + 0.3, CORD, 0.22),
            circ(w / 2, 0.6, 0.5, "#8a8478"), shadow(0.2, py, w - 0.6, ph)]
    body += plate(0.2, py, w - 0.6, ph, face, border, b=0.45, rx=0.8)
    blk = letters.fit(one(words), w - 0.6 - 2.2, ph - 1.6, 3.4, "bold", spill=0.4)
    body.append(blk.svg(1.3, py + 0.8, ink))
    return w, h, "units: %gx%g" % (w, h), body, None, blk


def neon(words, w, h, day, bright, core, frame_tube=None, lines=1, cap=3.4):
    """Tubes on a dark board.  By day the tubes show their colour; at night
    the emit layer adds the lit tube, its hot core and two halos."""
    body = [shadow(0, 0, w - 0.4, h - 0.5, 0.8)]
    body += plate(0, 0, w - 0.4, h - 0.5, NEON_BOARD, NEON_FRAME, b=0.35, rx=0.8)
    texts = split2(words) if lines == 2 else one(words)
    inset = 2.2 if frame_tube else 1.4
    blk = letters.fit(texts, w - 0.4 - 2 * inset, h - 0.5 - 2 * inset + 0.4, cap, "regular", gap=0.3,
                      spill=0.3 if frame_tube else 0.6)
    tx, ty = inset, inset - 0.2
    em = []
    if frame_tube:
        fx, fy, fw, fh = 1.0, 1.0, w - 2.4, h - 2.5
        tube = '<rect x="%s" y="%s" width="%s" height="%s" rx="0.8" fill="none" stroke="%%s" stroke-width="%%s"%%s/>' % (f(fx), f(fy), f(fw), f(fh))
        body.append(tube % (frame_tube[0], "0.35", ""))
        em += [tube % (frame_tube[1], "1.6", ' opacity="0.16"'), tube % (frame_tube[1], "0.45", "")]
    body.append(blk.svg(tx, ty, day))
    em += [blk.svg(tx, ty, bright, 1.8, 0.14), blk.svg(tx, ty, bright, 0.8, 0.3),
           blk.svg(tx, ty, bright), blk.svg(tx, ty, core, -blk.lines[0].sw * blk.lines[0].s * 0.55, 0.85)]
    return w, h, "units: %gx%g" % (w, h), body, em, blk


def lit_box(words, w, h, face, ink, glow, ink_lit, cap=3.4, weight="regular", bulbs=None, lines=1, lit=True):
    """A box sign lit from inside (bulbs: a marquee's border of lamps).
    glow None: the face is opaque and only the letters are lit."""
    body = [shadow(0, 0, w - 0.4, h - 0.5, 0.5)]
    border = "#2a2a30"
    body += plate(0, 0, w - 0.4, h - 0.5, face, border, b=0.5, rx=0.5)
    pw, ph = w - 0.4, h - 0.5
    inset = 1.0 + (1.0 if bulbs else 0.0)
    texts = split2(words) if lines == 2 else one(words)
    blk = letters.fit(texts, pw - 2 * inset, ph - 2 * inset + 0.4, cap, weight, gap=0.3, spill=0.4)
    body.append(blk.svg(inset, inset - 0.2, ink))
    em = None
    if lit and glow:
        # a translucent face lit from behind: the whole face glows
        em = [rect(0.5, 0.5, pw - 1.0, ph - 1.0, glow, 0.2, 0.5), blk.svg(inset, inset - 0.2, ink_lit)]
    elif lit:
        # an opaque face with lit letters: only the letters, with a halo
        em = [blk.svg(inset, inset - 0.2, ink_lit, 1.4, 0.18), blk.svg(inset, inset - 0.2, ink_lit)]
    if bulbs:
        pts = []
        n = max(2, int(round((pw - 1.6) / 1.6)))
        for i in range(n + 1):
            x = 0.8 + (pw - 1.6) * i / n
            pts += [(x, 0.8), (x, ph - 0.8)]
        m = max(1, int(round((ph - 1.6) / 1.6)))
        for j in range(1, m):
            y = 0.8 + (ph - 1.6) * j / m
            pts += [(0.8, y), (pw - 0.8, y)]
        body += [circ(x, y, 0.32, bulbs[0]) for x, y in pts]
        if em is not None:
            em += [circ(x, y, 0.75, bulbs[1], 0.25) for x, y in pts] + [circ(x, y, 0.34, bulbs[1]) for x, y in pts]
    return w, h, "units: %gx%g" % (w, h), body, em, blk


def awning(words, w, stripe_a, stripe_b, valance, ink):
    """A street-level awning over a shop front, its name on the valance."""
    h = 9.0
    body = [rect(0, 0, w, 0.7, "#5a5650", 0.2)]
    # the canopy, seen from below and in front: a band of stripes
    top, bot = 0.7, 3.2
    n = int(round(w / 2.0))
    sw = w / n
    for i in range(n):
        body.append(poly([(i * sw, top), ((i + 1) * sw, top), ((i + 1) * sw, bot), (i * sw, bot)],
                         stripe_a if i % 2 == 0 else stripe_b))
    body.append(rect(0, bot - 0.5, w, 0.5, "#000000", None, 0.12))
    vy, vh = bot, 4.6
    body.append(rect(0, vy, w, vh, valance))
    # scallops along the bottom edge
    for i in range(n):
        body.append('<path d="M%s %s a%s %s 0 0 0 %s 0 z" fill="%s"/>'
                    % (f(i * sw), f(vy + vh), f(sw / 2), f(sw / 2.6), f(sw), valance))
    blk = letters.fit(one(words), w - 3.0, vh - 1.0, 3.2, "bold", spill=0.4)
    body.append(blk.svg(1.5, vy + 0.5, ink))
    return w, h, "units: %gx%g" % (w, h), body, None, blk


def floor_plate(label, kind):
    """A landing's floor number."""
    w, h = 8, 5
    face, border, ink = {"basement": ("#5a6478", "#3a4252", CLOTH),
                         "ground": (GOLD, "#8a6a28", "#1a1a20"),
                         "above": ("#303038", "#18181c", CLOTH)}[kind]
    body = plate(0, 0, w, h, face, border, b=0.4, rx=0.6)
    blk = letters.fit(one(label), w - 1.6, h - 1.4, 3.2, "bold")
    body.append(blk.svg(0.8, 0.7, ink))
    return w, h, "units: %gx%g" % (w, h), body, None, blk


# --- the signs -------------------------------------------------------------
# file name -> (key in the language table, style function, arguments)

BANNER_CELLS = (4, 6, 9, 10, 12, 16)
BANNERS = {
    "for-rent": (CLOTH, RED, RED),
    "for-sale": (CLOTH, BLUE, BLUE),
    "sold": (RED, RED_DK, CLOTH),
    "let": ("#4a8a4a", GREEN_DK, CLOTH),
}

SIGNS = {}
for _k, (_c, _h, _i) in BANNERS.items():
    for _n in BANNER_CELLS:
        SIGNS["%s-%d" % (_k, _n)] = (_k, banner, (_n, _c, _h, _i))

SIGNS.update({
    # door cards and neon
    "open": ("open", hanging, (CLOTH, GREEN, GREEN)),
    "closed": ("closed", hanging, (RED, RED_DK, CLOTH)),
    "open-neon": ("open", neon, (16, 8, "#e07080", "#ff5070", "#ffe0e8", ("#e0b060", "#ffc050"))),
    "vacancy": ("vacancy", neon, (20, 6, "#60c890", "#50f090", "#e0fff0")),
    "no-vacancy": ("no-vacancy", neon, (24, 6, "#e07070", "#ff5050", "#ffe0e0")),
    # lit boxes
    "exit": ("exit", lit_box, (12, 6, "#1f8a4a", CLOTH, "#50f090", "#f0fff4", 3.0, "bold")),
    "full": ("full", lit_box, (14, 6, RED, CLOTH, "#ff5050", "#fff0f0", 3.4, "bold")),
    "hotel": ("hotel", lit_box, (24, 7, "#3a1a24", "#ecc070", None, "#ffe8b0", 4.0, "regular")),
    "cinema": ("cinema", lit_box, (28, 8, "#481018", "#ffe098", None, "#fff0c0", 4.0, "regular", ("#c8a860", "#ffe098"))),
    "now-showing": ("now-showing", lit_box, (32, 8, "#f8e8c0", "#481018", "#ffe098", "#481018", 3.6, "bold", ("#c8a860", "#ffe098"))),
    "parking": ("parking", lit_box, (20, 6, BLUE, CLOTH, "#80b0ff", "#f0f4ff", 3.0, "regular")),
    "metro": ("metro", lit_box, (16, 6, BLUE, CLOTH, "#80b0ff", "#f0f4ff", 3.0, "regular")),
    "ambulance": ("ambulance", lit_box, (28, 6, CLOTH, RED, "#fff4d0", "#ff5050", 3.4, "bold")),
    "police": ("police", lit_box, (20, 6, BLUE, CLOTH, "#80b0ff", "#f0f4ff", 3.4, "bold")),
    "fire": ("fire", lit_box, (16, 6, RED, CLOTH, "#ff5050", "#fff0f0", 3.4, "bold")),
    "evacuate.0": ("evacuate", lit_box, (24, 7, RED, CLOTH, "#ff5050", "#fff0f0", 3.4, "bold")),
    "evacuate.1": ("evacuate", lit_box, (24, 7, "#802828", "#e0c8c8", "#ff5050", "#fff0f0", 3.4, "bold", None, 1, False)),
    # wall plates
    "lobby": ("lobby", wall_plate, (16, 6, "#8f6d48", "#5a4430", "#fbf4e8")),
    "reception": ("reception", wall_plate, (22, 6, "#fbf4e8", "#8f6d48", "#5a4430")),
    "offices": ("offices", wall_plate, (18, 6, "#4a5c6c", "#2e3a46", CLOTH)),
    "party-hall": ("party-hall", wall_plate, (32, 6, "#3a1a4a", GOLD, GOLD)),
    "clinic": ("clinic", wall_plate, (20, 6, CLOTH, "#98b4a8", "#2a4a40", 3.0, "regular", 1, "cross")),
    "security": ("security", wall_plate, (20, 6, "#3a4858", "#202830", CLOTH)),
    "staff-only": ("staff-only", wall_plate, (20, 6, "#e8e4dc", "#6a6660", "#2a2a2a", 3.0, "bold")),
    "no-entry": ("no-entry", wall_plate, (28, 6, RED, RED_DK, CLOTH, 3.0, "bold", 1, "noentry")),
    "car-lift": ("car-lift", wall_plate, (16, 9, HAZARD, HAZARD_DK, HAZARD_DK, 3.0, "bold", 2)),
    "sale": ("sale", wall_plate, (12, 6, RED, "#f8c878", CLOTH, 3.8, "bold")),
    "out-of-order": ("out-of-order", hazard_plate, (14, 10)),
    # street-level awnings
    "shop": ("shop", awning, (20, BLUE, CLOTH, BLUE, CLOTH)),
    "cafe": ("cafe", awning, (16, "#4a8a4a", "#f0e4c8", "#3a6a3a", "#f0e4c8")),
    "restaurant": ("restaurant", awning, (28, "#6a2838", "#f0e4c8", "#6a2838", "#f8c878")),
})

BASEMENTS, ABOVE = 10, 109


def build(lang):
    with open(os.path.join(HERE, lang + ".json"), encoding="utf-8") as fh:
        table = json.load(fh)
    out = os.path.join(HERE, lang)
    os.makedirs(out, exist_ok=True)
    # anything this run does not write is stale
    for n in os.listdir(out):
        if n.endswith(".svg"):
            os.remove(os.path.join(out, n))
    report, problems = [], []
    todo = []
    for name, (key, fn, args) in SIGNS.items():
        if key not in table:
            problems.append("%s: no words for '%s'" % (lang, key))
            continue
        todo.append((name, key, table[key], fn, args))
    pat = {k: table.get("floor-" + k) for k in ("basement", "ground", "above")}
    for k, v in pat.items():
        if not v:
            problems.append("%s: no pattern floor-%s" % (lang, k))
    if all(pat.values()):
        for n in range(BASEMENTS, 0, -1):
            todo.append(("floor-b%d" % n, "floor-basement", pat["basement"].replace("{n}", str(n)), floor_plate, ("basement",)))
        todo.append(("floor-g", "floor-ground", pat["ground"], floor_plate, ("ground",)))
        for n in range(1, ABOVE + 1):
            todo.append(("floor-%d" % n, "floor-above", pat["above"].replace("{n}", str(n)), floor_plate, ("above",)))
    for name, key, words, fn, args in todo:
        bad = letters.missing(words)
        if bad:
            problems.append("%s/%s: letters.py has no glyph for %s" % (lang, name, " ".join(bad)))
            continue
        w, h, size, day, emit, blk = fn(words, *args)
        with open(os.path.join(out, name + ".svg"), "w", encoding="utf-8") as fh:
            fh.write(doc(w, h, day, size, key, lang, words))
        if emit:
            with open(os.path.join(out, name + ".emit.svg"), "w", encoding="utf-8") as fh:
                fh.write(doc(w, h, emit, size, key, lang, words))
        report.append((name, words, w, h, blk.cap, blk.squeeze))
        if blk.cap < 2.4 and not name.startswith("floor-"):
            problems.append("%s/%s: '%s' fits only at cap %.2f (squeeze %.2f): too small to read at 4 px a unit"
                            % (lang, name, words.replace("\n", " / "), blk.cap, blk.squeeze))
    return report, problems


def main():
    langs = sys.argv[1:] or sorted(n[:-5] for n in os.listdir(HERE) if n.endswith(".json"))
    bad = []
    for lang in langs:
        report, problems = build(lang)
        print("%s: %d signs" % (lang, len(report)))
        for name, words, w, h, cap, sq in report:
            if name.startswith("floor-") and name not in ("floor-b10", "floor-g", "floor-109"):
                continue
            flag = "  <- small" if cap < 2.6 else ("  <- condensed" if sq < 0.8 else "")
            print("  %-16s %5gx%-3g cap %.2f squeeze %.2f  %s%s" % (name, w, h, cap, sq, words.replace("\n", " / "), flag))
        bad += problems
    for p in bad:
        print("problem:", p)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
