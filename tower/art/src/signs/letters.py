#!/usr/bin/env python3
"""The tower's own sign lettering: a vector alphabet designed in code.

    import letters
    blk = letters.fit(["FOR RENT"], box_w=20, box_h=5, cap=3.4, weight="bold")
    svg_fragment = blk.svg(x, y, "#c04040")      # x, y = top-left of the box

Nothing here comes from a font.  Every glyph is a skeleton: one or more
polylines drawn on a design grid (cap height 10, y down, 0 the centre of
the top stroke, 10 the centre of the baseline stroke, x from 0 to the
glyph's skeleton width).  A vertex may carry a corner radius, so the round
letters are rounded rectangles (a squared sign-writer's hand, which holds
its shape at 12 pixels a cap where ovals go to mush).  The skeleton is
drawn with a round-capped, round-joined stroke whose width is part of the
weight; ink therefore reaches exactly 0 and the cap height, and the glyph's
ink box is its skeleton width plus one stroke.

Two weights:

  regular   stroke 1.65 / 10 of the cap, full width; for wall plates,
            neon tubes and banners with room to spare.
  bold      stroke 2.3 / 10 of the cap, widths x 0.74 (bold condensed);
            for narrow plates, floor numbers and door signs.

Condensing scales the skeleton's x (and the tracking) but never the stroke,
so a squeezed word keeps its weight.  `layout` squeezes down to
`MIN_SQUEEZE` and then, if the word still does not fit, makes it smaller.

Accented capitals are built from a base letter and a mark (acute, grave,
circumflex, diaeresis, tilde, ring above, cedilla), so any of A E I O U N C
with any mark exists; the marks sit 1.5 to 3.4 above the cap line (about
0.4 of the cap counting the stroke) and the cedilla below the baseline.
"""

import math

CAP = 10.0          # design cap height
MIN_SQUEEZE = 0.68  # narrowest a word may be condensed before it shrinks

WEIGHTS = {
    #           stroke  width   tracking  space
    "regular": (1.65,   1.00,   1.90,     3.4),
    "bold":    (2.30,   0.74,   1.45,     2.6),
}

R = 3.2   # radius of the round letters' corners
r = 2.4   # radius of the smaller bowls (B P R 2 3 5 6 9)


# --- the glyphs -----------------------------------------------------------
# Each entry: (skeleton width, [stroke, ...], [dot, ...]).
# A stroke is a list of vertices (x, y) or (x, y, radius); a stroke whose
# last vertex repeats the first is closed.  An x may be (x, d): d is a
# length in unsqueezed units, for a part that must not close up when the
# letter is condensed.  A dot is (x, y): a filled disc
# of 1.2 strokes' diameter at that skeleton point.

def _a(w):
    # crossbar at y = 6.6, on both legs
    return [[(0, 10), (w / 2, 0), (w, 10)], [(w / 2 * 0.34, 6.6), (w - w / 2 * 0.34, 6.6)]]


def _k(w):
    jx = 0.30 * w
    jy = 6.4 * (1 - jx / w)
    return [[(0, 0), (0, 10)], [(w, 0), (0, 6.4)], [(jx, jy), (w, 10)]]


def _ring(w):
    return [(0, 0, R), (w, 0, R), (w, 10, R), (0, 10, R), (0, 0, R)]


GLYPHS = {
    "A": (6.8, _a(6.8), []),
    "B": (5.6, [[(0, 4.8), (5.6, 4.8, r), (5.6, 10, r), (0, 10), (0, 0), (5.0, 0, r), (5.0, 4.8, r), (0, 4.8)]], []),
    "C": (5.6, [[(5.6, 0), (0, 0, R), (0, 10, R), (5.6, 10)]], []),
    "D": (5.8, [[(0, 0), (5.8, 0, R), (5.8, 10, R), (0, 10), (0, 0)]], []),
    "E": (5.0, [[(5.0, 0), (0, 0), (0, 10), (5.0, 10)], [(0, 5), (4.2, 5)]], []),
    "F": (5.0, [[(5.0, 0), (0, 0), (0, 10)], [(0, 5), (4.2, 5)]], []),
    "G": (6.0, [[(6.0, 0), (0, 0, R), (0, 10, R), (6.0, 10), (6.0, 5.0), ((6.0, -1.9), 5.0)]], []),
    "H": (5.8, [[(0, 0), (0, 10)], [(5.8, 0), (5.8, 10)], [(0, 5), (5.8, 5)]], []),
    "I": (0.0, [[(0, 0), (0, 10)]], []),
    "J": (4.8, [[(4.8, 0), (4.8, 10, R), (0, 10, R), (0, 7)]], []),
    "K": (5.8, _k(5.8), []),
    "L": (4.8, [[(0, 0), (0, 10), (4.8, 10)]], []),
    "M": (8.0, [[(0, 10), (0, 0), (4.0, 6.4), (8.0, 0), (8.0, 10)]], []),
    "N": (6.0, [[(0, 10), (0, 0), (6.0, 10), (6.0, 0)]], []),
    "O": (6.4, [_ring(6.4)], []),
    "P": (5.6, [[(0, 10), (0, 0), (5.6, 0, r), (5.6, 5.4, r), (0, 5.4)]], []),
    "Q": (6.4, [_ring(6.4), [(3.8, 6.6), (6.9, 10.6)]], []),
    "R": (5.6, [[(0, 10), (0, 0), (5.6, 0, r), (5.6, 5.4, r), (0, 5.4)], [(2.6, 5.4), (5.6, 10)]], []),
    "S": (5.6, [[(5.6, 0), (0, 0, 2.5), (0, 5, 2.5), (5.6, 5, 2.5), (5.6, 10, 2.5), (0, 10)]], []),
    "T": (6.0, [[(0, 0), (6.0, 0)], [(3.0, 0), (3.0, 10)]], []),
    "U": (5.8, [[(0, 0), (0, 10, R), (5.8, 10, R), (5.8, 0)]], []),
    "V": (6.6, [[(0, 0), (3.3, 10), (6.6, 0)]], []),
    "W": (9.4, [[(0, 0), (2.35, 10), (4.7, 3.2), (7.05, 10), (9.4, 0)]], []),
    "X": (6.2, [[(0, 0), (6.2, 10)], [(6.2, 0), (0, 10)]], []),
    "Y": (6.4, [[(0, 0), (3.2, 5.6), (6.4, 0)], [(3.2, 5.6), (3.2, 10)]], []),
    "Z": (5.8, [[(0, 0), (5.8, 0), (0, 10), (5.8, 10)]], []),
    # capital sharp s: a stem, a flat top, a diagonal into a lower bowl
    "ß": (5.8, [[(0, 10), (0, 0, R), (5.8, 0), (1.9, 4.6), (5.8, 4.6, 2.8), (5.8, 10, 2.8), (2.0, 10)]], []),
    # figures: the same squared hand, a touch narrower than the letters
    "0": (5.4, [_ring(5.4)], []),
    "1": (2.8, [[(0, 2.4), (2.8, 0), (2.8, 10)]], []),
    "2": (5.2, [[(0, 0), (5.2, 0, r), (5.2, 5, r), (0, 5, r), (0, 10), (5.2, 10)]], []),
    "3": (5.2, [[(0, 0), (5.2, 0, r), (5.2, 10, r), (0, 10)], [(1.6, 5), (5.2, 5)]], []),
    "4": (5.4, [[(4.0, 10), (4.0, 0), (0, 6.8), (5.4, 6.8)]], []),
    "5": (5.2, [[(5.2, 0), (0, 0), (0, 4.6), (5.2, 4.6, r), (5.2, 10, r), (0, 10)]], []),
    "6": (5.2, [[(5.2, 0), (0, 0, r), (0, 10, r), (5.2, 10, r), (5.2, 4.6, r), (0, 4.6)]], []),
    "7": (5.2, [[(0, 0), (5.2, 0), (1.6, 10)]], []),
    "8": (5.4, [[(0.4, 0, r), (5.0, 0, r), (5.0, 4.7, r), (0.4, 4.7, r), (0.4, 0, r)],
                [(0, 4.7, r), (5.4, 4.7, r), (5.4, 10, r), (0, 10, r), (0, 4.7, r)]], []),
    "9": (5.2, [[(0, 10), (5.2, 10, r), (5.2, 0, r), (0, 0, r), (0, 5.4, r), (5.2, 5.4)]], []),
    # punctuation
    "&": (6.4, [[(6.4, 10), (1.0, 3.6, 0.8), (1.0, 0, 1.8), (4.4, 0, 1.8), (4.4, 3.4, 1.4),
                 (0, 6.4, 1.6), (0, 10, 2.4), (4.2, 10, 2.4), (6.4, 6.4)]], []),
    "-": (3.0, [[(0, 5.6), (3.0, 5.6)]], []),
    "'": (0.0, [[(0, 0), (0, 3.2)]], []),
    ".": (0.0, [], [(0, 10)]),
    "!": (0.0, [[(0, 0), (0, 6.4)]], [(0, 10)]),
    "/": (4.4, [[(4.4, 0), (0, 10)]], []),
    " ": (None, [], []),
}

# Marks, drawn about the base letter's centre (cx) in the same grid.
MARKS = {
    "acute":      ([[(-0.8, -1.4), (0.9, -3.0)]], []),
    "grave":      ([[(0.8, -1.4), (-0.9, -3.0)]], []),
    "circumflex": ([[(-1.7, -1.4), (0, -3.0), (1.7, -1.4)]], []),
    "diaeresis":  ([], [(-1.6, -2.2), (1.6, -2.2)]),
    "tilde":      ([[(-2.0, -1.5), (-0.8, -2.9, 0.4), (0.8, -1.6, 0.4), (2.0, -3.0)]], []),
    "ring":       ([[(-1.1, -1.2, 0.9), (1.1, -1.2, 0.9), (1.1, -3.0, 0.9), (-1.1, -3.0, 0.9), (-1.1, -1.2, 0.9)]], []),
    "cedilla":    ([[(0, 10), (0, 11.2), (1.0, 11.2, 0.6), (1.0, 12.6, 0.6), (-0.9, 12.6)]], []),
}
MARK_RISE = 3.0   # skeleton top of the highest mark above the cap line


def _lift(sw):
    """How much further up the marks sit for a heavier stroke, so the gap
    between a mark and its letter stays open."""
    return max(0.0, (sw - 1.65) * 0.8)
MARK_DROP = 2.6         # skeleton bottom of the cedilla below the baseline

ACCENTED = {}
for _ch, _base, _mark in [
    ("Á", "A", "acute"), ("À", "A", "grave"), ("Â", "A", "circumflex"), ("Ä", "A", "diaeresis"),
    ("Ã", "A", "tilde"), ("Å", "A", "ring"),
    ("É", "E", "acute"), ("È", "E", "grave"), ("Ê", "E", "circumflex"), ("Ë", "E", "diaeresis"),
    ("Í", "I", "acute"), ("Ì", "I", "grave"), ("Î", "I", "circumflex"), ("Ï", "I", "diaeresis"),
    ("Ó", "O", "acute"), ("Ò", "O", "grave"), ("Ô", "O", "circumflex"), ("Ö", "O", "diaeresis"),
    ("Õ", "O", "tilde"),
    ("Ú", "U", "acute"), ("Ù", "U", "grave"), ("Û", "U", "circumflex"), ("Ü", "U", "diaeresis"),
    ("Ñ", "N", "tilde"), ("Ç", "C", "cedilla"), ("Ý", "Y", "acute"),
]:
    ACCENTED[_ch] = (_base, _mark)

# Kerning, in design units at full width (scaled with the squeeze): the
# pairs whose shapes leave a hole a plain advance would show.
_K = {}
for _pair, _v in [
    ("AV", -1.6), ("VA", -1.6), ("AW", -1.2), ("WA", -1.2), ("AY", -1.4), ("YA", -1.4),
    ("AT", -1.4), ("TA", -1.4), ("LT", -2.2), ("LV", -2.0), ("LW", -1.6), ("LY", -2.0),
    ("TO", -0.6), ("OT", -0.6), ("TC", -0.6), ("TG", -0.6), ("FA", -1.2), ("PA", -1.2),
    ("T.", -1.6), ("P.", -1.4), ("F.", -1.4), ("V.", -1.6), ("Y.", -1.6), ("W.", -1.2),
    ("L'", -2.4), ("'A", -1.4), ("A'", -1.0), ("AJ", 0.4), ("LO", -0.6), ("LU", -0.4),
    ("AC", -0.4), ("AG", -0.4), ("AO", -0.4), ("AU", -0.3), ("AQ", -0.4),
    ("OA", -0.4), ("OV", -0.4), ("VO", -0.4), ("YO", -0.6), ("OY", -0.6),
    ("TT", 0.0), ("RT", -0.6), ("RV", -0.6), ("RY", -0.6), ("KO", -0.4),
    ("17", -0.8), ("71", -1.0), ("74", -1.0), ("-1", -0.4),
]:
    _K[_pair] = _v


def _base_of(ch):
    return ACCENTED[ch][0] if ch in ACCENTED else ch


def kern(a, b):
    return _K.get(_base_of(a) + _base_of(b), 0.0)


def missing(text):
    """Characters of `text` this alphabet cannot draw."""
    return sorted({c for c in text if c not in GLYPHS and c not in ACCENTED and c != "\n"})


def has_mark_above(text):
    return any(c in ACCENTED and ACCENTED[c][1] != "cedilla" for c in text)


def has_mark_below(text):
    return any(c in ACCENTED and ACCENTED[c][1] == "cedilla" for c in text)


# --- geometry -----------------------------------------------------------

def _num(v):
    s = ("%.3f" % v).rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def _rounded(pts):
    """SVG path data for a polyline whose vertices may carry a radius."""
    closed = len(pts) > 2 and abs(pts[0][0] - pts[-1][0]) < 1e-9 and abs(pts[0][1] - pts[-1][1]) < 1e-9
    P = [(p[0], p[1]) for p in pts]
    rad = [p[2] if len(p) > 2 else 0.0 for p in pts]
    if closed:
        P, rad = P[:-1], rad[:-1]
    n = len(P)
    out = []

    def corner(i):
        """(enter, ctrl, leave) for vertex i, or None if sharp."""
        if rad[i] <= 0:
            return None
        if not closed and (i == 0 or i == n - 1):
            return None
        a, p, b = P[(i - 1) % n], P[i], P[(i + 1) % n]
        la = math.hypot(a[0] - p[0], a[1] - p[1])
        lb = math.hypot(b[0] - p[0], b[1] - p[1])
        if la < 1e-9 or lb < 1e-9:
            return None
        # a radius may use at most half of each neighbouring segment
        rr = min(rad[i], la / 2 if rad[(i - 1) % n] > 0 else la, lb / 2 if rad[(i + 1) % n] > 0 else lb)
        e = (p[0] + (a[0] - p[0]) / la * rr, p[1] + (a[1] - p[1]) / la * rr)
        l = (p[0] + (b[0] - p[0]) / lb * rr, p[1] + (b[1] - p[1]) / lb * rr)
        return e, p, l

    cs = [corner(i) for i in range(n)]
    k = 0.5523  # cubic for a quarter circle; exact for right angles
    for i in range(n):
        c = cs[i]
        if c is None:
            out.append(("M" if i == 0 else "L") + _num(P[i][0]) + " " + _num(P[i][1]))
            continue
        e, p, l = c
        out.append(("M" if i == 0 else "L") + _num(e[0]) + " " + _num(e[1]))
        c1 = (e[0] + (p[0] - e[0]) * k, e[1] + (p[1] - e[1]) * k)
        c2 = (l[0] + (p[0] - l[0]) * k, l[1] + (p[1] - l[1]) * k)
        out.append("C" + " ".join(_num(v) for v in (c1[0], c1[1], c2[0], c2[1], l[0], l[1])))
    if closed:
        out.append("Z")
    return "".join(out)


class Line:
    """One line of lettering laid out at a cap height and squeeze."""

    def __init__(self, text, cap, weight="regular", squeeze=1.0):
        self.text, self.cap, self.weight, self.squeeze = text, cap, weight, squeeze
        sw, wf, track, space = WEIGHTS[weight]
        self.s = cap / CAP                      # design units -> output units
        self.sw = sw                            # design stroke
        self.kx = wf * squeeze                  # skeleton x factor
        self.pen = []                           # (x origin in design units, char)
        x = 0.0
        prev = None
        for ch in text:
            if prev is not None:
                x += kern(prev, ch) * self.kx
            self.pen.append((x, ch))
            if ch == " ":
                x += space * squeeze
                prev = None
                continue
            w = GLYPHS[_base_of(ch)][0]
            x += w * self.kx + sw + track * squeeze
            prev = ch
        if text and text[-1] != " ":
            x -= track * squeeze
        self.width_d = x
        self.width = x * self.s                 # ink width in output units
        self.above = (MARK_RISE + _lift(sw) + sw / 2) * self.s if has_mark_above(text) else 0.0
        self.below = (MARK_DROP + sw / 2) * self.s if has_mark_below(text) else 0.0

    def _map(self, ox, x, y):
        # skeleton (x, y) of a glyph at pen ox -> design-space ink coordinates;
        # x may be (x, d): d is a length that does not squeeze (G's short bar)
        if isinstance(x, tuple):
            x = x[0] + x[1] / self.kx
        return ox + self.sw / 2 + x * self.kx, self.sw / 2 + y * (CAP - self.sw) / CAP

    def shapes(self, x0, y0):
        """(path data, [(cx, cy, r)]) with the cap top-left at (x0, y0)."""
        d, dots = [], []
        s = self.s
        for ox, ch in self.pen:
            if ch == " ":
                continue
            base, mark = ACCENTED.get(ch, (ch, None))
            w, strokes, pts = GLYPHS[base]
            parts = [(strokes, pts, 0.0)]
            if mark:
                ms, mp = MARKS[mark]
                parts.append((ms, mp, w / 2))
            for strokes, pts, dx in parts:
                for st in strokes:
                    q = []
                    for v in st:
                        if dx:
                            # marks keep their own width: unsqueezed about the centre
                            X = ox + self.sw / 2 + dx * self.kx + v[0] * min(1.0, self.kx + 0.15)
                            Y = self.sw / 2 + v[1] * (CAP - self.sw) / CAP
                            if v[1] < 0:
                                Y -= _lift(self.sw)
                        else:
                            X, Y = self._map(ox, v[0], v[1])
                        q.append((x0 + X * s, y0 + Y * s) + ((v[2] * s,) if len(v) > 2 else ()))
                    d.append(_rounded(q))
                for v in pts:
                    if dx:
                        X = ox + self.sw / 2 + dx * self.kx + v[0] * min(1.0, self.kx + 0.15)
                        Y = self.sw / 2 + v[1] * (CAP - self.sw) / CAP - _lift(self.sw)
                    else:
                        X, Y = self._map(ox, v[0], v[1])
                    # a dot's centre sits so its ink meets the cap/base line
                    rr = self.sw * 0.62
                    if v[1] >= 10:
                        Y = CAP - rr
                    dots.append((x0 + X * s, y0 + Y * s, rr * s))
        return "".join(d), dots

    def svg(self, x0, y0, colour, extra_width=0.0, opacity=None):
        """The line as SVG with its cap top-left at (x0, y0).  `extra_width`
        thickens the stroke (in output units), for neon glows."""
        d, dots = self.shapes(x0, y0)
        sw = self.sw * self.s + extra_width
        op = ' opacity="%s"' % _num(opacity) if opacity is not None else ""
        out = []
        if d:
            out.append('<path d="%s" fill="none" stroke="%s" stroke-width="%s" '
                       'stroke-linecap="round" stroke-linejoin="round"%s/>' % (d, colour, _num(sw), op))
        for cx, cy, rr in dots:
            out.append('<circle cx="%s" cy="%s" r="%s" fill="%s"%s/>'
                       % (_num(cx), _num(cy), _num(rr + extra_width / 2), colour, op))
        return "\n".join(out)


def layout(text, cap, weight="regular", max_width=None):
    """Lay out one line at `cap`, squeezing (to MIN_SQUEEZE) and then
    shrinking until it is no wider than `max_width`.  Returns a Line."""
    ln = Line(text, cap, weight)
    if max_width is None or ln.width <= max_width + 1e-9:
        return ln
    # width(sq) = a * sq + b in design units: the stroke part does not squeeze
    n = sum(1 for c in text if c != " ")
    b = n * WEIGHTS[weight][0]
    a = ln.width_d - b
    sq = (max_width / ln.s - b) / a if a > 0 else 1.0
    if sq >= MIN_SQUEEZE:
        return Line(text, cap, weight, sq)
    ln = Line(text, cap, weight, MIN_SQUEEZE)
    return Line(text, cap * max_width / ln.width, weight, MIN_SQUEEZE)


class Block:
    """Lines of lettering fitted to a box.  The capitals are centred in the
    box; a mark above the first line may rise into `spill` above it."""

    def __init__(self, lines, box_w, box_h, gap, spill=0.0):
        self.lines, self.box_w, self.box_h, self.gap, self.spill = lines, box_w, box_h, gap, spill
        self.cap = min(l.cap for l in lines)
        self.squeeze = min(l.squeeze for l in lines)

    def caps_height(self):
        return sum(l.cap for l in self.lines) + self.gap * (len(self.lines) - 1)

    def top(self, y):
        """y of the first line's cap line, for a box whose top is at y."""
        ls = self.lines
        t = y + (self.box_h - self.caps_height()) / 2
        t = max(t, y - self.spill + ls[0].above)                     # marks inside box + spill
        t = min(t, y + self.box_h - self.caps_height() - ls[-1].below)  # cedilla inside the box
        return t

    def svg(self, x, y, colour, extra_width=0.0, opacity=None, align="centre"):
        out = []
        top = self.top(y)
        for l in self.lines:
            lx = x if align == "left" else x + (self.box_w - l.width) / 2
            out.append(l.svg(lx, top, colour, extra_width, opacity))
            top += l.cap + self.gap
        return "\n".join(out)


def fit(lines, box_w, box_h, cap, weight="regular", gap=0.45, spill=0.0):
    """Fit lines (a list of strings) into a box_w x box_h box at no more than
    `cap`; `gap` is the space between lines as a fraction of the cap.  Marks
    above the first line may use `spill` above the box (a plate's padding);
    otherwise accents and cedillas are kept inside the box, so a sign's size
    never depends on its language."""
    for t in lines:
        bad = missing(t)
        if bad:
            raise ValueError("no glyph for %s in %r" % (" ".join(bad), t))
    n = len(lines)
    sw = WEIGHTS[weight][0]
    up = (MARK_RISE + _lift(sw) + sw / 2) / CAP if has_mark_above(lines[0]) else 0.0
    down = (MARK_DROP + sw / 2) / CAP if has_mark_below(lines[-1]) else 0.0
    # marks on later lines go in the gap; widen it if they need more
    g = gap
    if any(has_mark_above(t) for t in lines[1:]):
        g = max(g, (MARK_RISE + _lift(sw) + sw / 2) / CAP + 0.12)
    rows = n + g * (n - 1)
    c = min(cap, box_h / (rows + down), (box_h + spill) / (rows + up + down))
    # (the capitals are centred when the marks leave room; otherwise they
    # sit lower in the box, which is better than smaller)
    ls = [layout(t, c, weight, box_w) for t in lines]
    c2 = min(l.cap for l in ls)
    if c2 < c - 1e-9:
        ls = [layout(t, c2, weight, box_w) for t in lines]
    return Block(ls, box_w, box_h, g * min(l.cap for l in ls), spill)


def specimen():
    """An SVG specimen of both weights (for checking the alphabet)."""
    rows = [
        "ABCDEFGHIJKLM", "NOPQRSTUVWXYZ", "0123456789 & - ' . ! /",
        "ÉÈÊÀÇÜÖÄÑß ÔÎÛËÏ", "AVATAR LT TA WAY L'AY", "FOR RENT  NO VACANCY",
    ]
    cap, y, body = 4.0, 2.0, []
    width = 0
    for wt in ("regular", "bold"):
        for t in rows:
            ln = Line(t, cap, wt)
            body.append(ln.svg(2, y + 1.6, "#1a1a20"))
            width = max(width, ln.width)
            y += cap + 2.6
        y += 1.0
    w, h = math.ceil(width + 4), math.ceil(y + 1)
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %d %d">\n'
            '<!-- The tower\'s sign lettering, both weights, cap 4 units: art/src/signs/letters.py -->\n'
            '<!-- units: %dx%d -->\n<rect width="%d" height="%d" fill="#f0f0e8"/>\n%s\n</svg>\n'
            % (w, h, w, h, w, h, "\n".join(body)))


if __name__ == "__main__":
    import sys
    sys.stdout.write(specimen())
