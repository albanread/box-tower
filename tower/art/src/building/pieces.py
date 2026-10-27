#!/usr/bin/env python3
"""Draw the building, world and event pieces of the tower's art.

    python3 art/src/building/pieces.py

Writes every SVG in art/src/building, art/src/world and art/src/events.
The SVGs are the source the presenters use; this program is how they were
drawn, so a change to a piece is made here and the program run again.
Scattered things (stones, skyline, lit windows) come from fixed seeds, so a
run gives the same files every time.

Units: a cell is 4 wide, a floor 16 tall; y grows downwards and a floor's
own walking line (top of the slab) is y = 15.
"""

import math
import os
import random

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.dirname(HERE)
DIRS = {d: os.path.join(SRC, d) for d in ("building", "world", "events")}

CELL, FLOOR = 4, 16


def f(v):
    s = ("%.2f" % v).rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def svg(w, h, body, slot=None, units=None, note=None):
    head = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %s %s">\n' % (f(w), f(h))
    c = ["<!-- Drawn for the BOX port of tower-together by art/src/building/pieces.py. -->"]
    if slot:
        c.append("<!-- slot: %gx%g -->" % slot)
    if units:
        c.append("<!-- units: %gx%g -->" % units)
    if note:
        c.append("<!-- %s -->" % note)
    return head + "\n".join("  " + x for x in c) + "\n" + "\n".join("  " + b for b in body if b) + "\n</svg>\n"


def rect(x, y, w, h, fill, rx=None, op=None):
    a = ' rx="%s"' % f(rx) if rx else ""
    o = ' opacity="%s"' % f(op) if op is not None else ""
    return '<rect x="%s" y="%s" width="%s" height="%s" fill="%s"%s%s/>' % (f(x), f(y), f(w), f(h), fill, a, o)


def circ(cx, cy, r, fill, op=None):
    o = ' opacity="%s"' % f(op) if op is not None else ""
    return '<circle cx="%s" cy="%s" r="%s" fill="%s"%s/>' % (f(cx), f(cy), f(r), fill, o)


def ell(cx, cy, rx, ry, fill, op=None):
    o = ' opacity="%s"' % f(op) if op is not None else ""
    return '<ellipse cx="%s" cy="%s" rx="%s" ry="%s" fill="%s"%s/>' % (f(cx), f(cy), f(rx), f(ry), fill, o)


def poly(pts, fill, op=None):
    o = ' opacity="%s"' % f(op) if op is not None else ""
    return '<polygon points="%s" fill="%s"%s/>' % (" ".join("%s,%s" % (f(x), f(y)) for x, y in pts), fill, o)


def path(d, fill, op=None):
    o = ' opacity="%s"' % f(op) if op is not None else ""
    return '<path d="%s" fill="%s"%s/>' % (d, fill, o)


def bar(x0, y0, x1, y1, w, fill):
    """A straight bar of width w from (x0,y0) to (x1,y1), as a polygon."""
    dx, dy = x1 - x0, y1 - y0
    n = math.hypot(dx, dy) or 1
    ox, oy = -dy / n * w / 2, dx / n * w / 2
    return poly([(x0 + ox, y0 + oy), (x1 + ox, y1 + oy), (x1 - ox, y1 - oy), (x0 - ox, y0 - oy)], fill)


def mirror(body, w):
    return ['<g transform="translate(%s 0) scale(-1 1)">' % f(w)] + body + ["</g>"]


OUTPUT = []


def write(d, name, text):
    p = os.path.join(DIRS[d], name)
    with open(p, "w") as fh:
        fh.write(text)
    OUTPUT.append((d, name))


def emit_empty(w, h, **kw):
    return svg(w, h, [], **kw)


# ─── Palette ────────────────────────────────────────────────────────────────

SLAB, SLAB_LIP, SLAB_SHADE = "#575b60", "#72777d", "#45494d"
BARE, BARE_DARK, BEAM, COLUMN, COLUMN_SH = "#4c5157", "#41454a", "#5d6268", "#686d73", "#3f4348"
FACADE, FACADE_HI, FACADE_SH = "#7d8690", "#98a1aa", "#5a6169"
PARAPET, COPING, ROOF_SH = "#6f7780", "#8f98a1", "#59606a"
GRANITE, GRANITE_HI, GRANITE_SH = "#6b6560", "#857e77", "#55504b"
BASEMENT, BASEMENT_JOINT, BASEMENT_HOLE, BASEMENT_PIPE = "#58534d", "#4b4742", "#3e3a36", "#6e6963"
LAMP_WARM, LAMP_COOL, LAMP_RED = "#ffd98a", "#e6f2ff", "#ff4a3a"


# ─── building/ ──────────────────────────────────────────────────────────────

def slab_body():
    return [rect(0, 15, 4, 1, SLAB), rect(0, 15, 4, 0.3, SLAB_LIP), rect(0, 15.75, 4, 0.25, SLAB_SHADE)]


def building():
    S = (1, 1)
    write("building", "slab.svg", svg(4, 16, slab_body(), slot=S,
          note="the slab under every built cell, drawn over the room; top of slab y=15"))

    # The empty built floor: bare structure.
    def bare(column, lamp):
        b = [rect(0, 0, 4, 15, BARE),
             rect(0, 0, 4, 1.4, BEAM), rect(0, 1.4, 4, 0.4, BARE_DARK),
             rect(0, 2.6, 4, 0.5, BARE_DARK),                 # conduit
             rect(0, 12.6, 4, 2.4, BARE_DARK, op=0.55)]       # floor shadow
        if column:
            b += [rect(0, 0, 1.1, 15, COLUMN), rect(1.1, 1.8, 0.5, 13.2, COLUMN_SH),
                  rect(0, 0, 1.1, 1.4, COPING)]
        if lamp:
            b += [rect(2.65, 1.8, 0.2, 1.6, BARE_DARK), circ(2.75, 4.0, 0.6, "#c9c2a8")]
        return b + slab_body()

    write("building", "floor.svg", svg(4, 16, bare(False, False), slot=S, note="empty built floor, plain"))
    write("building", "floor-column.svg", svg(4, 16, bare(True, True), slot=S,
          note="empty built floor with a column and a work lamp"))
    write("building", "floor-column.emit.svg", svg(4, 16, [
        circ(2.75, 4.0, 1.2, LAMP_WARM, op=0.15), circ(2.75, 4.0, 0.5, LAMP_WARM)], slot=S))

    # Outer walls at the two ends of a run, drawn over the end cell.
    wall = [rect(0, 0, 1.0, 16, FACADE), rect(0, 0, 0.35, 16, FACADE_HI), rect(1.0, 0, 0.35, 15, FACADE_SH),
            rect(0, 14.7, 1.35, 1.3, SLAB), rect(0, 14.7, 1.35, 0.3, SLAB_LIP)]
    write("building", "wall-left.svg", svg(4, 16, wall, slot=S, note="outer wall, drawn over the leftmost cell of a run"))
    write("building", "wall-right.svg", svg(4, 16, mirror(wall, 4), slot=S, note="outer wall, drawn over the rightmost cell of a run"))

    # Roof: drawn in the floor ABOVE the topmost floor of a column.
    def parapet():
        return [rect(0, 12.8, 4, 3.2, PARAPET), rect(0, 12.3, 4, 0.8, COPING),
                rect(0, 13.1, 4, 0.35, ROOF_SH), rect(0, 15.4, 4, 0.6, ROOF_SH)]

    write("building", "roof.svg", svg(4, 16, parapet(), slot=S, note="roof parapet; placed one floor above the top of a run"))
    lamp = parapet() + [rect(1.4, 11.1, 1.2, 1.3, "#4d545c", rx=0.3), rect(1.6, 11.3, 0.8, 0.7, "#e9dcb0", rx=0.2)]
    write("building", "roof-light.svg", svg(4, 16, lamp, slot=S, note="roof parapet with a lamp; one in every four roof cells"))
    write("building", "roof-light.emit.svg", svg(4, 16, [
        ell(2, 11.7, 2.0, 1.2, LAMP_WARM, op=0.3), rect(1.6, 11.3, 0.8, 0.7, LAMP_WARM, rx=0.2)], slot=S))
    end = parapet() + [rect(0, 11.2, 1.3, 4.8, PARAPET), rect(0, 10.8, 1.5, 0.7, COPING), rect(0, 11.2, 0.35, 4.8, FACADE_HI)]
    write("building", "roof-left.svg", svg(4, 16, end, slot=S, note="roof corner over the leftmost cell"))
    write("building", "roof-right.svg", svg(4, 16, mirror(end, 4), slot=S, note="roof corner over the rightmost cell"))

    # Water tank and plant room, two cells.
    tank = [rect(0, 12.8, 8, 3.2, PARAPET), rect(0, 12.3, 8, 0.8, COPING), rect(0, 13.1, 8, 0.35, ROOF_SH),
            rect(0, 15.4, 8, 0.6, ROOF_SH),
            rect(1.0, 8.5, 0.5, 4.0, "#535a62"), rect(6.5, 8.5, 0.5, 4.0, "#535a62"),
            bar(1.25, 8.8, 6.75, 12.2, 0.3, "#535a62"),
            rect(0.6, 3.0, 6.8, 5.8, "#8a7a68", rx=0.6), rect(0.6, 3.0, 6.8, 1.0, "#9d8d7a", rx=0.5),
            rect(0.6, 5.2, 6.8, 0.35, "#6f6253"), rect(0.6, 7.2, 6.8, 0.35, "#6f6253"),
            poly([(0.4, 3.1), (4, 1.4), (7.6, 3.1)], "#6f6253")]
    write("building", "roof-tank.svg", svg(8, 16, tank, slot=(2, 1), note="roof with a water tank; two cells, instead of two roof cells"))
    plant = [rect(0, 12.8, 8, 3.2, PARAPET), rect(0, 12.3, 8, 0.8, COPING), rect(0, 13.1, 8, 0.35, ROOF_SH),
             rect(0, 15.4, 8, 0.6, ROOF_SH),
             rect(0.8, 6.6, 6.4, 5.8, "#7b838c"), rect(0.8, 6.6, 6.4, 0.7, "#929aa3"),
             circ(2.6, 9.5, 1.5, "#5f666e"), circ(2.6, 9.5, 0.5, "#7b838c"),
             circ(5.4, 9.5, 1.5, "#5f666e"), circ(5.4, 9.5, 0.5, "#7b838c")]
    write("building", "roof-plant.svg", svg(8, 16, plant, slot=(2, 1), note="roof with a plant room; two cells"))

    # Beacon on the highest roof: blinks.
    for i in range(2):
        b = parapet() + [rect(1.75, 3.5, 0.5, 9.0, "#5a6168"), bar(2, 6, 0.8, 12.3, 0.25, "#5a6168"),
                         bar(2, 6, 3.2, 12.3, 0.25, "#5a6168"),
                         circ(2, 3.2, 0.85, "#c8402f" if i == 0 else "#7a3a33")]
        write("building", "roof-beacon.%d.svg" % i, svg(4, 16, b, slot=S, note="aviation beacon; frame %d of 2" % i))
        e = [circ(2, 3.2, 2.2, LAMP_RED, op=0.25), circ(2, 3.2, 0.85, LAMP_RED)] if i == 0 else [circ(2, 3.2, 0.5, LAMP_RED, op=0.4)]
        write("building", "roof-beacon.%d.emit.svg" % i, svg(4, 16, e, slot=S))

    # Fire escapes (BOX): a steel stair two cells wide outside each end of
    # the floors, drawn here for the left end (the building to the right of
    # x = 8) and mirrored for the right.  `flight`: the floor's landing at
    # y = 15 and the two flights that climb from it through this storey,
    # outer end to inner and back, to the landing above (y = -1);
    # `top`: a landing with its rail and nothing above.  Each is cut into
    # its two cells, `out` and `in`, because a back piece repeats every its
    # own width: the publisher lays them cell by cell.
    def escape(top):
        steel, steel_d, rail, grate = "#3e454d", "#2c3238", "#5b646d", "#4c545d"
        b = [rect(7.55, 0 if not top else 8.5, 0.45, 16 if not top else 7.5, steel_d)]   # wall brackets' upright
        b += [rect(0.25, 0 if not top else 9.2, 0.4, 16 if not top else 6.8, steel)]     # outer post
        if not top:
            # the flights: a stringer and treads, outer foot to inner half-landing to outer top
            for (x0, y0, x1, y1) in ((1.2, 15, 6.8, 7), (6.8, 7, 1.2, -1)):
                b.append(bar(x0, y0, x1, y1, 0.55, steel))
                for k in range(1, 8):
                    t = k / 8.0
                    x, y = x0 + (x1 - x0) * t, y0 + (y1 - y0) * t
                    b.append(rect(x - 0.45, y - 0.15, 0.9, 0.3, grate))
                b.append(bar(x0, y0 - 5.0, x1, y1 - 5.0, 0.22, rail))                     # handrail
            b += [rect(5.2, 6.8, 2.8, 0.5, grate), rect(5.2, 7.3, 2.8, 0.25, steel_d),   # half-landing
                  rect(5.3, 2.0, 0.25, 4.8, rail)]
        # the landing at the floor, with its rail
        b += [rect(0, 14.6, 8, 0.55, grate), rect(0, 15.15, 8, 0.3, steel_d),
              rect(0, 9.6, 7.6, 0.3, rail), rect(0, 12.3, 7.6, 0.22, rail)]
        b += [rect(x, 9.6, 0.22, 5.0, rail) for x in (2.4, 4.6)]
        return b

    for name, top in (("flight", False), ("top", True)):
        body = escape(top)
        for side, pic in (("left", body), ("right", mirror(body, 8))):
            for part in ("out", "in"):
                # the left stair's outer cell is its left one; the right's, its right one
                shift = 0 if (part == "out") == (side == "left") else -4
                write("building", "escape-%s-%s-%s.svg" % (name, side, part),
                      svg(4, 16, ['<g transform="translate(%d 0)">' % shift] + pic + ["</g>"], slot=S,
                          note="fire escape, %s, %s end of the floors, %s cell" % (name, side, part)))

    # Street-level base of the ground floor.
    base = [rect(0, 13.4, 4, 2.6, GRANITE), rect(0, 13.4, 4, 0.45, GRANITE_HI), rect(0, 15.5, 4, 0.5, GRANITE_SH),
            rect(3.8, 13.85, 0.2, 1.65, GRANITE_SH)]
    write("building", "base.svg", svg(4, 16, base, slot=S, note="granite plinth along the ground floor, drawn over the slab"))

    # Underground.
    def basement(light):
        b = [rect(0, 0, 4, 15, BASEMENT), rect(0, 0, 0.25, 15, BASEMENT_JOINT), rect(0, 7.4, 4, 0.25, BASEMENT_JOINT),
             circ(1.0, 4.0, 0.35, BASEMENT_HOLE), circ(3.0, 4.0, 0.35, BASEMENT_HOLE),
             circ(1.0, 11.0, 0.35, BASEMENT_HOLE), circ(3.0, 11.0, 0.35, BASEMENT_HOLE),
             rect(0, 0, 4, 1.2, "#4a4641"), rect(0, 1.5, 4, 0.8, BASEMENT_PIPE), rect(0, 1.5, 4, 0.25, "#827c75"),
             rect(0, 13.0, 4, 2.0, "#4e4a45")]
        if light:
            b += [rect(0.6, 2.3, 2.8, 0.25, "#3e3a36"), rect(0.7, 2.55, 2.6, 0.55, "#d9e2e6", rx=0.25)]
        return b + slab_body()

    write("building", "basement.svg", svg(4, 16, basement(False), slot=S, note="empty underground floor"))
    write("building", "basement-light.svg", svg(4, 16, basement(True), slot=S, note="empty underground floor with a strip light; one cell in three"))
    write("building", "basement-light.emit.svg", svg(4, 16, [
        ell(2, 3.4, 1.9, 0.9, LAMP_COOL, op=0.15), rect(0.7, 2.55, 2.6, 0.55, LAMP_COOL, rx=0.25)], slot=S))
    rw = [rect(0, 0, 1.2, 16, "#4e4944"), rect(1.2, 0, 0.35, 15, "#3d3935"), rect(0, 0, 0.3, 16, "#625c56"),
          rect(0, 5, 1.2, 0.25, "#3d3935"), rect(0, 10.5, 1.2, 0.25, "#3d3935")]
    write("building", "basement-left.svg", svg(4, 16, rw, slot=S, note="retaining wall, leftmost underground cell"))
    write("building", "basement-right.svg", svg(4, 16, mirror(rw, 4), slot=S, note="retaining wall, rightmost underground cell"))

    # Construction: scaffold over bare structure; a beam is hoisted, a welder sparks.
    POLE, PLANK, PLANK_SH, NET = "#8c949c", "#b48d58", "#8e6c40", "#5f8a6a"
    for i, (load_y, spark) in enumerate([(11.5, False), (7.5, True), (3.5, False)]):
        b = bare(False, False)
        b += [rect(0, 0, 4, 15, NET, op=0.35)]
        b += [rect(0.15, 0, 0.45, 16, POLE),
              bar(0.4, 14.6, 4.0, 8.4, 0.3, POLE), bar(0.4, 7.6, 4.0, 1.4, 0.3, POLE),
              rect(0, 7.6, 4, 0.7, PLANK), rect(0, 8.3, 4, 0.3, PLANK_SH),
              rect(0, 0.6, 4, 0.7, PLANK), rect(0, 1.3, 4, 0.3, PLANK_SH),
              rect(0, 7.3, 4, 0.3, POLE), rect(0, 0.3, 4, 0.3, POLE)]
        b += [rect(2.9, 0, 0.12, load_y, "#2f3236"), rect(2.1, load_y, 1.7, 0.8, "#a0522d"),
              rect(2.1, load_y, 1.7, 0.25, "#c06a3e")]
        if spark:
            b += [rect(1.2, 13.4, 1.0, 1.6, "#3b3f44", rx=0.3), circ(2.3, 13.4, 0.45, "#ffe9a0")]
        write("building", "construct.%d.svg" % i, svg(4, 16, b, slot=S, note="under construction; frame %d of 3" % i))
        if spark:
            write("building", "construct.%d.emit.svg" % i, svg(4, 16, [
                circ(2.3, 13.4, 1.5, "#bfe3ff", op=0.35), circ(2.3, 13.4, 0.45, "#ffffff")], slot=S))

    # Demolition: rubble left behind, and dust while it comes down.
    RUB = ["#7b7570", "#69635e", "#8c867f", "#5b5651"]
    for v, seed in (("rubble", 11), ("rubble-b", 23)):
        r = random.Random(seed)
        b = [rect(0, 0, 4, 15, BARE_DARK), rect(0, 0, 4, 1.4, BEAM)]
        b += [poly([(0, 15), (0, 11.5), (1.2, 10.2), (2.4, 11.0), (4, 9.6), (4, 15)], "#5f5954")]
        for _ in range(9):
            cx, cy, s = r.uniform(0.4, 3.6), r.uniform(10.5, 14.6), r.uniform(0.5, 1.1)
            pts = [(cx + s * math.cos(a), cy + s * 0.7 * math.sin(a))
                   for a in sorted(r.uniform(0, 2 * math.pi) for _ in range(5))]
            b.append(poly(pts, r.choice(RUB)))
        for _ in range(2):
            x = r.uniform(0.8, 3.2)
            b.append(bar(x, 11.5, x + r.uniform(-1.0, 1.0), 8.2, 0.22, "#8a4a2c"))
        b += slab_body()
        write("building", v + ".svg", svg(4, 16, b, slot=S, note="demolished cell (rubble)"))
    DUST = ["#b5aea6", "#a39b92", "#c8c2bb"]
    for i in range(3):
        r = random.Random(40 + i)
        b = []
        for k in range(7):
            b.append(circ(r.uniform(0.6, 3.4), 14.2 - i * 2.2 - r.uniform(0, 5.5), 0.9 + i * 0.35 + r.uniform(0, 0.5),
                          DUST[k % 3], op=0.9 - i * 0.2))
        write("building", "dust.%d.svg" % i, svg(4, 16, b, slot=S, note="demolition dust over a cell; frame %d of 3" % i))


# ─── world/ ─────────────────────────────────────────────────────────────────

PAVE, PAVE_FACE, PAVE_JOINT = "#b6b0a6", "#9c968d", "#8a847b"


def pavement(w=4):
    b = [rect(0, 13.6, w, 2.4, PAVE_FACE), rect(0, 13.6, w, 0.6, PAVE), rect(0, 15.6, w, 0.4, "#857f77")]
    for x in range(0, int(w), 4):
        b.append(rect(x, 14.2, 0.2, 1.4, PAVE_JOINT))
    return b


def world():
    S = (1, 1)
    write("world", "street.svg", svg(4, 16, pavement(), slot=S, note="pavement on floor 10 outside the building; ground line y=16"))
    road = [rect(0, 14.4, 4, 1.6, "#4e5156"), rect(0, 14.4, 4, 0.3, "#62666b"), rect(0.6, 15.1, 2.2, 0.35, "#d8d2c2")]
    write("world", "road.svg", svg(4, 16, road, slot=S, note="road surface (in front of the parking ramp and beyond the pavement)"))

    lamp = pavement() + [rect(1.75, 4.2, 0.5, 9.6, "#4b5258"), rect(1.5, 13.0, 1.0, 0.8, "#4b5258"),
                         rect(1.75, 4.0, 1.9, 0.4, "#4b5258"), rect(2.6, 4.3, 1.2, 0.6, "#3c4247", rx=0.2),
                         rect(2.75, 4.85, 0.9, 0.3, "#eadfb8")]
    write("world", "street-lamp.svg", svg(4, 16, lamp, slot=S, note="pavement with a street lamp"))
    write("world", "street-lamp.emit.svg", svg(4, 16, [
        poly([(2.75, 5.1), (3.65, 5.1), (4.0, 13.6), (1.6, 13.6)], LAMP_WARM, op=0.18),
        rect(2.75, 4.85, 0.9, 0.3, LAMP_WARM)], slot=S))

    tree = pavement(8) + [rect(3.7, 9.0, 0.6, 4.8, "#6a4f36"), bar(4, 10.5, 5.4, 8.6, 0.35, "#6a4f36"),
                          rect(3.0, 13.4, 2.0, 0.3, "#7a6650"),
                          circ(4.0, 6.0, 2.9, "#5f8a4f"), circ(2.4, 7.6, 2.0, "#5f8a4f"), circ(5.7, 7.4, 2.1, "#5f8a4f"),
                          circ(3.5, 5.0, 1.8, "#76a160"), circ(5.0, 6.0, 1.4, "#76a160"), circ(2.2, 7.0, 1.1, "#76a160")]
    write("world", "street-tree.svg", svg(8, 16, tree, slot=(2, 1), note="pavement with a tree; two cells"))

    bench = pavement() + [rect(0.5, 11.3, 3.0, 0.5, "#9a6c42"), rect(0.5, 10.1, 3.0, 0.45, "#9a6c42"),
                          rect(0.5, 10.6, 3.0, 0.3, "#9a6c42"),
                          rect(0.8, 11.8, 0.35, 1.8, "#3f4549"), rect(2.85, 11.8, 0.35, 1.8, "#3f4549")]
    write("world", "street-bench.svg", svg(4, 16, bench, slot=S, note="pavement with a bench"))
    hyd = pavement() + [rect(1.4, 10.6, 1.2, 3.0, "#c2452f", rx=0.3), rect(1.2, 10.2, 1.6, 0.6, "#a3392a", rx=0.3),
                        rect(1.0, 11.5, 2.0, 0.6, "#a3392a", rx=0.25)]
    write("world", "street-hydrant.svg", svg(4, 16, hyd, slot=S, note="pavement with a hydrant"))
    bush = pavement() + [rect(0.4, 12.0, 3.2, 1.6, "#7b6b58"), ell(2.0, 11.2, 1.7, 1.3, "#5f8a4f"),
                         ell(1.4, 10.9, 0.9, 0.7, "#76a160"), ell(2.7, 10.7, 0.7, 0.5, "#76a160")]
    write("world", "street-planter.svg", svg(4, 16, bush, slot=S, note="pavement with a planter"))

    # Underground ground: soil (floors 1-9), rock (floor 0 and below ~5), the top layer under the street.
    SOIL, ROCK = "#6e583f", "#5b5753"
    stones = ["#7f6a52", "#5f4c37", "#887358", "#57524c"]
    for i, seed in enumerate((3, 5, 8, 13)):
        r = random.Random(seed)
        b = [rect(0, 0, 4, 16, SOIL)]
        for _ in range(r.randint(3, 5)):
            cx, cy = r.uniform(0.7, 3.3), r.uniform(1, 15)
            b.append(ell(cx, cy, r.uniform(0.25, 0.7), r.uniform(0.2, 0.45), r.choice(stones)))
        cy = r.uniform(3, 13)
        b.append(ell(r.uniform(1.2, 2.8), cy, 1.0, 0.25, "#64503a"))
        write("world", "soil-%s.svg" % "abcd"[i], svg(4, 16, b, slot=S, note="soil, variant %s; pick by a hash of (cell, floor)" % "abcd"[i]))
    for i, seed in enumerate((101, 103, 107, 109)):
        r = random.Random(seed)
        # Two strata lines every variant shares, so they run on across cells.
        b = [rect(0, 0, 4, 16, ROCK), rect(0, 5.2, 4, 0.5, "#4f4b47"), rect(0, 11.6, 4, 0.4, "#66625d")]
        for _ in range(r.randint(1, 2)):
            cx, cy = r.uniform(1.4, 2.6), r.choice([r.uniform(1.6, 3.8), r.uniform(7.2, 10.2), r.uniform(13.2, 14.6)])
            rx, ry = r.uniform(0.8, 1.3), r.uniform(0.6, 1.0)
            b += [ell(cx, cy + 0.2, rx, ry, "#4a4743"), ell(cx - 0.15, cy, rx * 0.85, ry * 0.8, r.choice(["#6a665f", "#625e58"]))]
        b.append(circ(r.uniform(0.6, 3.4), r.uniform(6.5, 10.5), 0.3, "#4a4743"))
        write("world", "rock-%s.svg" % "abcd"[i], svg(4, 16, b, slot=S, note="rock, variant %s; strata at y=5.2 and y=11.6 run on across variants" % "abcd"[i]))
    for i, seed in enumerate((201, 203)):
        r = random.Random(seed)
        b = [rect(0, 0, 4, 16, SOIL), rect(0, 0, 4, 2.2, "#857c6f"), rect(0, 2.2, 4, 1.4, "#76664f")]
        for _ in range(4):
            b.append(circ(r.uniform(0.3, 3.7), r.uniform(0.4, 1.9), r.uniform(0.18, 0.3), "#9a9184"))
        for _ in range(3):
            b.append(ell(r.uniform(0.7, 3.3), r.uniform(5, 15), r.uniform(0.25, 0.6), r.uniform(0.2, 0.4), r.choice(stones)))
        write("world", "soil-top-%s.svg" % "ab"[i], svg(4, 16, b, slot=S, note="the floor just under the street (floor 9): road base over soil"))

    # Clouds: flat, a shaded underside and a lit top. Free sprites in units.
    CL, CL_SH = "#f4f7fa", "#d9e2ec"
    shapes = [
        (40, 12, [(10, 7, 5), (17, 5, 6), (25, 6, 5.5), (32, 8, 4)]),
        (28, 10, [(8, 6, 4), (14, 4.5, 4.5), (20, 6, 4)]),
        (56, 14, [(9, 9, 5), (17, 7, 6.5), (27, 5.5, 7), (37, 7, 6), (46, 9, 4.5)]),
        (20, 8, [(6, 5, 3), (11, 4, 3.5), (15, 5.5, 2.5)]),
        (48, 10, [(8, 7, 3.5), (16, 6, 4.5), (25, 5, 4.5), (33, 6, 4), (40, 7, 3)]),
        (34, 13, [(10, 8, 5), (18, 6, 6.5), (25, 8, 4.5)]),
        (64, 10, [(8, 6.5, 3), (14, 5.5, 3.8), (21, 5, 4.2), (29, 4.2, 4.6), (37, 4.8, 4.2), (45, 5.4, 3.8), (52, 6, 3.4), (57, 6.8, 2.6)]),
    ]
    for i, (w, h, puffs) in enumerate(shapes):
        x0 = min(cx - r for cx, cy, r in puffs) + 1
        x1 = max(cx + r for cx, cy, r in puffs) - 1
        b = [rect(x0, h - 3.2, x1 - x0, 3.2, CL_SH, rx=1.6)]
        b += [circ(cx, cy + 0.9, r, CL_SH) for cx, cy, r in puffs]
        b += [circ(cx - 0.3, cy, r * 0.92, CL) for cx, cy, r in puffs]
        b += [rect(x0 + 0.6, h - 4.6, x1 - x0 - 1.2, 1.6, CL, rx=0.8)]
        write("world", "cloud-%s.svg" % "abcdefg"[i], svg(w, h, b, units=(w, h), note="cloud; free sprite drifting in the sky"))

    # Skyline: two depths, each tiles horizontally (period 400 units).
    def skyline(seed, color, roofc, hmin, hmax, wmin, wmax, H, windows):
        r = random.Random(seed)
        W = 400
        blds = []
        x = 0
        while x < W:
            w = r.uniform(wmin, wmax)
            h = r.uniform(hmin, hmax)
            blds.append((x, w, h, r.random()))
            x += w + r.uniform(-2, 3)
        b, e = [], []
        for bx, bw, bh, k in blds:
            for off in (0, -W):
                if bx + off + bw < 0 or bx + off > W:
                    continue
                X = bx + off
                b.append(rect(X, H - bh, bw, bh, color))
                if k < 0.25:
                    b.append(rect(X + bw * 0.3, H - bh - 4, bw * 0.4, 4, color))
                elif k < 0.4:
                    b.append(rect(X + bw / 2 - 0.3, H - bh - 9, 0.6, 9, color))
                elif k < 0.5:
                    b.append(poly([(X, H - bh), (X + bw / 2, H - bh - bw * 0.35), (X + bw, H - bh)], color))
                elif k < 0.6:
                    b.append(rect(X + bw * 0.15, H - bh - 2.5, bw * 0.25, 2.5, roofc))
                if windows:
                    wr = random.Random(int(bx * 100) + seed)
                    for wy in range(int(H - bh + 3), int(H - 2), 4):
                        for wx in range(int(X + 1.5), int(X + bw - 1.5), 3):
                            if wr.random() < windows:
                                e.append(rect(wx, wy, 1.2, 1.6, wr.choice([LAMP_WARM, "#ffe7b0", "#cfe6ff"])))
        return b, e

    for name, args in (("skyline-far", (7, "#b7c4d2", "#a6b4c4", 18, 70, 10, 26, 96, 0.10)),
                       ("skyline-near", (9, "#8e9eb0", "#7d8ea1", 12, 52, 12, 30, 96, 0.22))):
        b, e = skyline(*args)
        note = "parallax backdrop, tiles horizontally every 400 units; bottom edge on the ground line"
        write("world", name + ".svg", svg(400, 96, b, units=(400, 96), note=note))
        write("world", name + ".emit.svg", svg(400, 96, e, units=(400, 96)))

    sun = [circ(12, 12, 11, "#ffe9a8", op=0.5), circ(12, 12, 8.5, "#ffd86b"), circ(10.5, 10.5, 4, "#ffe58f")]
    write("world", "sun.svg", svg(24, 24, sun, units=(24, 24), note="the sun; free sprite"))
    write("world", "sun.emit.svg", svg(24, 24, [circ(12, 12, 8.5, "#fff0b8")], units=(24, 24)))
    moon = [path("M 9 1.5 A 7.5 7.5 0 1 0 15.5 12 A 6 6 0 1 1 9 1.5 Z", "#e7ebf0"),
            circ(5.5, 10, 0.9, "#cfd6de"), circ(7.5, 13.3, 0.6, "#cfd6de")]
    write("world", "moon.svg", svg(16, 16, moon, units=(16, 16), note="the moon; free sprite, shown at night"))
    write("world", "moon.emit.svg", svg(16, 16, [
        circ(8, 9, 7.8, "#dfe8ff", op=0.18),
        path("M 9 1.5 A 7.5 7.5 0 1 0 15.5 12 A 6 6 0 1 1 9 1.5 Z", "#f2f5ff")], units=(16, 16)))
    for i, (x, y) in enumerate(((1.5, 1.5), (0.8, 0.8))):
        s = [poly([(2, 0), (2.4, 1.6), (4, 2), (2.4, 2.4), (2, 4), (1.6, 2.4), (0, 2), (1.6, 1.6)], "#e8eefc")]
        write("world", "star.%d.svg" % i, svg(4, 4, s if i == 0 else [circ(2, 2, 0.8, "#e8eefc")], units=(4, 4),
              note="a night-sky star; frame %d of 2 (twinkle)" % i))
        write("world", "star.%d.emit.svg" % i, svg(4, 4, s if i == 0 else [circ(2, 2, 0.8, "#ffffff")], units=(4, 4)))


# ─── events/ ────────────────────────────────────────────────────────────────

FL_R, FL_O, FL_Y, FL_W = "#d9452b", "#f17a2b", "#ffb43f", "#ffe48e"


def tongue(cx, base, w, h, lean):
    return "M %s %s Q %s %s %s %s Q %s %s %s %s Z" % (
        f(cx - w / 2), f(base), f(cx - w / 2 + lean * 0.3), f(base - h * 0.55), f(cx + lean), f(base - h),
        f(cx + w / 2 + lean * 0.3), f(base - h * 0.55), f(cx + w / 2), f(base))


def flames(frame, n, scale=1.0, xs=None, emit=False):
    """Flames along the bottom of a cell. Each tongue's height and lean cycle with the frame."""
    xs = xs or [0.9, 2.2, 3.3]
    b = []
    layers = [(FL_R, 1.0, 1.0), (FL_O, 0.72, 0.78), (FL_Y, 0.48, 0.55), (FL_W, 0.26, 0.3)]
    if emit:
        layers = [("#ff6a2a", 1.0, 1.0), ("#ff9a3a", 0.72, 0.78), ("#ffd060", 0.48, 0.55), ("#fff2b0", 0.26, 0.3)]
    for li, (col, ws, hs) in enumerate(layers):
        for k, cx in enumerate(xs):
            ph = (frame / n) * 2 * math.pi + k * 2.1
            h = (8.5 + 2.8 * math.sin(ph) + (1.5 if k == 1 else 0)) * scale
            lean = 0.55 * math.sin(ph * 2 + k)
            w = (1.9 if k != 1 else 2.2) * scale
            b.append(path(tongue(cx + lean * 0.2, 15.2, w * ws, h * hs, lean * hs), col))
        if li == 0:
            b.append(rect(0, 12.6 + (1 - scale) * 2, 4, 2.6 - (1 - scale) * 2, col))
    return b


def figure(frame, coat, legs, head="#e0b08c", hat=None, extra=None, skirt=None):
    """A walking person 3 units wide, about 11 tall, feet on y=15. Two-frame stride."""
    sw = 0.7 if frame == 0 else -0.7
    b = []
    if skirt:
        b.append(poly([(1.2, 8.0), (2.8, 8.0), (3.3, 12.6), (0.7, 12.6)], skirt))
        b += [bar(1.6, 12.4, 1.6 - sw * 0.4, 15, 0.55, legs), bar(2.4, 12.4, 2.4 + sw * 0.4, 15, 0.55, legs)]
    else:
        b += [bar(1.7, 10.8, 1.7 - sw, 15, 0.75, legs), bar(2.3, 10.8, 2.3 + sw, 15, 0.75, legs)]
    b += [rect(1.15, 6.4, 1.7, 5.0 if not skirt else 2.0, coat, rx=0.5)]
    b += [bar(1.3, 6.9, 1.3 + sw * 0.6, 10.2, 0.6, coat), bar(2.7, 6.9, 2.7 - sw * 0.6, 10.2, 0.6, coat)]
    b += [rect(1.75, 5.6, 0.5, 0.9, head), circ(2, 4.6, 1.05, head)]
    if hat:
        b += hat
    if extra:
        b += extra
    return b


def events():
    S = (1, 1)
    # Fire along a floor: the middle of the burning span, and its two ends.
    for i in range(6):
        soot = [rect(0, 0, 4, 2.2, "#3b302b", op=0.85), rect(0, 2.2, 4, 1.0, "#3b302b", op=0.45)]
        write("events", "fire.%d.svg" % i, svg(4, 16, soot + flames(i, 6), slot=S,
              note="fire, middle of the burning span; frame %d of 6, 10 frames a second" % i))
        write("events", "fire.%d.emit.svg" % i, svg(4, 16, [rect(0, 6, 4, 9.5, "#ff5a20", op=0.22)] + flames(i, 6, emit=True), slot=S))
        end = flames(i, 6, scale=0.65, xs=[2.4, 3.4])
        write("events", "fire-left.%d.svg" % i, svg(4, 16, end, slot=S, note="fire, the cell at the left end of the span; frame %d of 6" % i))
        write("events", "fire-left.%d.emit.svg" % i, svg(4, 16, flames(i, 6, scale=0.65, xs=[2.4, 3.4], emit=True), slot=S))
        write("events", "fire-right.%d.svg" % i, svg(4, 16, mirror(end, 4), slot=S, note="fire, the cell at the right end of the span; frame %d of 6" % i))
        write("events", "fire-right.%d.emit.svg" % i, svg(4, 16, mirror(flames(i, 6, scale=0.65, xs=[2.4, 3.4], emit=True), 4), slot=S))

    # Smoke rising from a burning floor through the floor above: 1 cell x 2 floors.
    SM = ["#5e5956", "#77716d", "#8f8985"]
    for i in range(4):
        b = []
        for k in range(9):
            t = (k + i / 4) / 9               # 0 at the fire, 1 at the top of the floor above
            y = 26 - t * 23
            r = 1.45 + t * 0.5
            x = 2 + 0.55 * math.sin(k * 2.3 + i * 0.6)
            r *= 1 + 0.15 * math.cos(k * 1.7)
            b.append(circ(x, y + 0.5, r, SM[0], op=0.9 - t * 0.45))
            b.append(circ(x - 0.25, y, r * 0.8, SM[1 + k % 2], op=0.9 - t * 0.45))
        write("events", "smoke.%d.svg" % i, svg(4, 32, b, slot=(1, 2),
              note="smoke over a burning cell; bottom floor is the burning one; frame %d of 4, 6 a second" % i))

    burnt = [rect(0, 0, 4, 15, "#2f2a27"), rect(0, 0, 4, 1.4, "#1f1b19"),
             poly([(0, 1.4), (4, 1.4), (4, 4.2), (2.8, 3.0), (1.6, 5.0), (0, 3.4)], "#1f1b19"),
             rect(0.6, 6.0, 2.8, 4.6, "#3d3633"), poly([(0.6, 6.0), (3.4, 6.0), (3.4, 7.2), (2.2, 8.8), (1.4, 7.0), (0.6, 8.2)], "#252120"),
             rect(0.6, 6.0, 0.25, 4.6, "#4a4340"), rect(3.15, 6.0, 0.25, 4.6, "#4a4340"),
             poly([(0, 15), (0, 13.8), (1.5, 13.2), (2.6, 13.9), (4, 13.4), (4, 15)], "#4a4441"),
             circ(1.2, 14.2, 0.35, "#6a2f22"), circ(3.1, 14.3, 0.3, "#6a2f22")] + slab_body()
    write("events", "burnt.svg", svg(4, 16, burnt, slot=S, note="a cell the fire has gutted; stays until rebuilt"))
    write("events", "burnt.emit.svg", svg(4, 16, [circ(1.2, 14.2, 0.35, "#ff6a2a", op=0.8), circ(3.1, 14.3, 0.3, "#ff6a2a", op=0.6)], slot=S))

    # Water poured by the helicopter.
    for i in range(3):
        b = [poly([(0.6, 0), (3.4, 0), (3.8, 15), (0.2, 15)], "#9fcbe6", op=0.55)]
        r = random.Random(300 + i)
        for _ in range(9):
            x, y = r.uniform(0.4, 3.6), r.uniform(0, 14)
            b.append(rect(x, y, 0.35, 1.4, "#e4f2fb"))
        b += [ell(2, 15.0, 2.0, 0.5, "#cfe6f4")]
        write("events", "water.%d.svg" % i, svg(4, 16, b, slot=S, note="water from the helicopter over a burning cell; frame %d of 3" % i))

    # Rescue helicopter: 8 cells x 1 floor, facing left (it works right to left).
    BODY, BODY_SH, STRIPE, GLASS, METAL = "#c8412f", "#a3321f", "#f1ece4", "#a9cbe0", "#4a5058"

    def heli(fr):
        b = [rect(15, 12.4, 0.4, 1.4, METAL), rect(8.2, 12.4, 0.4, 1.4, METAL),
             rect(5.0, 13.6, 13.8, 0.55, METAL, rx=0.27), rect(4.5, 13.2, 1.0, 0.7, METAL, rx=0.3),
             poly([(16, 7.8), (29, 8.6), (29, 9.7), (16, 10.8)], BODY),
             poly([(27.4, 5.4), (29.4, 5.4), (30.2, 9.7), (28.4, 9.7)], BODY_SH),
             path("M 5 10.3 Q 5 6.2 10.5 5.6 L 16 5.6 Q 18.8 5.8 18.8 9.2 L 18.8 11.4 Q 18.8 12.6 17.4 12.6 L 7 12.6 Q 5 12.6 5 10.3 Z", BODY),
             rect(5.1, 10.4, 13.7, 0.8, STRIPE), poly([(16, 9.4), (29, 9.0), (29, 9.4), (16, 10.2)], STRIPE),
             path("M 5.4 9.6 Q 5.6 6.8 9.8 6.3 L 9.8 9.6 Z", GLASS),
             rect(10.6, 6.6, 2.4, 2.6, GLASS, rx=0.3), rect(13.6, 6.6, 2.4, 2.6, GLASS, rx=0.3),
             rect(9.5, 4.4, 6.0, 1.3, BODY_SH, rx=0.5), rect(12.1, 3.2, 0.8, 1.4, METAL),
             circ(29.3, 7.5, 0.5, METAL)]
        blade = [13.0, 7.5, 1.0, 7.5][fr]
        b += [rect(12.5 - blade, 2.7, blade * 2, 0.55, "#2d3238", rx=0.27)]
        if fr in (1, 3):
            b += [rect(12.5 - 11, 2.75, 22, 0.4, "#2d3238", op=0.25)]
        tr = [(0, 2.4), (1.7, 1.7), (2.4, 0), (1.7, -1.7)][fr]
        b += [bar(29.3 - tr[0], 7.5 - tr[1], 29.3 + tr[0], 7.5 + tr[1], 0.35, "#2d3238")]
        b += [rect(17.0, 11.0, 0.6, 0.6, "#5a2018"), rect(5.6, 11.6, 0.5, 0.5, "#2e5a2a")]
        return b

    for i in range(4):
        write("events", "helicopter.%d.svg" % i, svg(32, 16, heli(i), slot=(8, 1),
              note="rescue helicopter facing left; rotor frame %d of 4, 20 frames a second" % i))
        e = [rect(17.0, 11.0, 0.6, 0.6, LAMP_RED), rect(5.6, 11.6, 0.5, 0.5, "#6aff7a"),
             path("M 5.4 9.6 Q 5.6 6.8 9.8 6.3 L 9.8 9.6 Z", "#cfe6ff", op=0.5)]
        if i % 2 == 0:
            e += [circ(29.3, 5.4, 0.9, LAMP_RED, op=0.5), circ(29.3, 5.4, 0.4, LAMP_RED)]
        write("events", "helicopter.%d.emit.svg" % i, svg(32, 16, e, slot=(8, 1)))

    # Firefighters walking to the fire (one per security office).
    for i in range(2):
        hat = [path("M 0.7 4.4 Q 0.9 2.8 2 2.8 Q 3.2 2.8 3.3 4.4 Z", "#f2c230"), rect(0.6, 4.1, 2.8, 0.45, "#d9a91f")]
        extra = [rect(1.15, 9.0, 1.7, 0.45, "#e8e0b0"), rect(2.6, 7.4, 0.8, 2.6, "#c8402f", rx=0.35)]
        write("events", "firefighter.%d.svg" % i, svg(4, 16, figure(i, "#3b4250", "#2c313a", hat=hat, extra=extra), slot=S,
              note="firefighter walking left to right is mirrored by the presenter; frame %d of 2, 4 a second" % i))
        write("events", "firefighter.%d.emit.svg" % i, svg(4, 16, [rect(1.15, 9.0, 1.7, 0.45, "#fff7c0")], slot=S))

    # Bomb: a case with a blinking lamp, then the blast.
    for i in range(2):
        b = [rect(0.7, 11.6, 2.6, 3.4, "#3b3f44", rx=0.4), rect(1.6, 11.0, 0.8, 0.8, "#2a2d31", rx=0.25),
             rect(0.9, 12.1, 2.2, 1.1, "#24272b", rx=0.2),
             bar(1.0, 13.6, 2.8, 14.4, 0.2, "#c8402f"), bar(1.0, 14.4, 2.8, 13.6, 0.2, "#d9b13a"),
             circ(2.5, 12.6, 0.35, "#ff4a3a" if i == 0 else "#5a2018")]
        write("events", "bomb.%d.svg" % i, svg(4, 16, b, slot=S, note="the planted bomb; frame %d of 2, blinking 2 a second" % i))
        e = [circ(2.5, 12.6, 1.0, LAMP_RED, op=0.4), circ(2.5, 12.6, 0.35, LAMP_RED)] if i == 0 else []
        write("events", "bomb.%d.emit.svg" % i, svg(4, 16, e, slot=S))

    def burst(cx, cy, r0, r1, n, rot):
        pts = []
        for k in range(n * 2):
            a = rot + k * math.pi / n
            r = r1 if k % 2 == 0 else r0
            pts.append((cx + r * math.cos(a), cy + r * 0.8 * math.sin(a)))
        return pts

    for i in range(4):
        s = [0.45, 0.85, 1.0, 0.8][i]
        cx, cy = 8, 22
        b = []
        if i >= 2:
            b += [circ(8 + dx, 12 - i * 2 + dy, 3.2 + i * 0.6, c, op=0.9) for dx, dy, c in
                  ((-3, 2, "#6c6662"), (2.5, 0, "#857f7a"), (0, -3, "#6c6662"))]
        if i < 3:
            b += [poly(burst(cx, cy, 4 * s, 9 * s, 9, 0.2 * i), FL_R), poly(burst(cx, cy, 2.8 * s, 6.5 * s, 9, 0.5 + 0.2 * i), FL_O),
                  poly(burst(cx, cy, 1.8 * s, 4.2 * s, 7, 0.1 * i), FL_Y), circ(cx, cy, 1.8 * s, "#fff8dc")]
        else:
            b += [circ(cx + dx, cy + dy, 2.5, c, op=0.85) for dx, dy, c in
                  ((-4, 2, "#5e5956"), (0, -1, "#77716d"), (4, 2, "#5e5956"), (0, 4, "#8f8985"))]
        write("events", "explosion.%d.svg" % i, svg(16, 32, b, slot=(4, 2),
              note="the bomb going off, centred on the bomb's cell; bottom floor is the bomb's; frame %d of 4, 8 a second" % i))
        e = [] if i == 3 else [circ(cx, cy, 9 * s, "#ff8a3a", op=0.35), poly(burst(cx, cy, 1.8 * s, 4.2 * s, 7, 0.1 * i), "#ffe080"),
                                circ(cx, cy, 1.8 * s, "#ffffff")]
        write("events", "explosion.%d.emit.svg" % i, svg(16, 32, e, slot=(4, 2)))

    # VIP: a long dark car at the kerb, and the visitor in a plum coat and silver hat.
    car = [rect(0.8, 10.6, 18.4, 3.0, "#23262c", rx=0.9), path("M 4.5 10.8 L 6.2 7.9 L 14.6 7.9 L 16.2 10.8 Z", "#23262c"),
           path("M 6.6 10.4 L 7.6 8.5 L 10.1 8.5 L 10.1 10.4 Z", "#566676"), rect(10.6, 8.5, 3.2, 1.9, "#566676"),
           path("M 14.3 8.5 L 15.0 8.5 L 15.7 10.4 L 14.3 10.4 Z", "#566676"),
           rect(0.8, 12.0, 18.4, 0.35, "#b9c0c8"), rect(0.6, 11.0, 0.6, 1.0, "#b9c0c8"), rect(18.8, 11.0, 0.6, 1.0, "#b9c0c8"),
           circ(4.0, 13.8, 1.3, "#1a1c20"), circ(4.0, 13.8, 0.55, "#9aa2ab"),
           circ(16.0, 13.8, 1.3, "#1a1c20"), circ(16.0, 13.8, 0.55, "#9aa2ab"),
           rect(0.9, 11.1, 0.6, 0.5, "#f5f0d8"), rect(18.5, 11.1, 0.6, 0.5, "#8a2a22"),
           rect(2.0, 8.6, 0.15, 2.1, "#b9c0c8"), poly([(2.15, 8.6), (3.3, 8.95), (2.15, 9.3)], "#c9a640")]
    write("events", "vip-car.svg", svg(20, 16, car, slot=(5, 1), note="the VIP's car at the kerb, facing left; wheels on the ground line"))
    write("events", "vip-car.emit.svg", svg(20, 16, [
        poly([(0.9, 11.1), (-0.0, 10.6), (0, 12.4), (0.9, 11.6)], "#fff3c8", op=0.5),
        rect(0.9, 11.1, 0.6, 0.5, "#fff3c8"), rect(18.5, 11.1, 0.6, 0.5, LAMP_RED),
        rect(10.6, 8.5, 3.2, 1.9, "#ffe7b0", op=0.4)], slot=(5, 1)))
    for i in range(2):
        hat = [rect(1.0, 3.5, 2.0, 0.4, "#c9ced6"), rect(1.35, 1.9, 1.3, 1.7, "#c9ced6")]
        extra = [rect(1.15, 6.4, 1.7, 0.5, "#e9e4da"), bar(1.4, 6.6, 2.7, 10.4, 0.45, "#d9b44a")]
        write("events", "vip.%d.svg" % i, svg(4, 16, figure(i, "#5b3a7a", "#2a2533", hat=hat, extra=extra), slot=S,
              note="the visiting VIP walking; frame %d of 2, 4 a second" % i))

    # Cathedral guests: festive clothes, flowers.
    for i in range(2):
        hat = [ell(2, 3.7, 1.8, 0.45, "#e8c3cf"), circ(2, 3.4, 0.8, "#e8c3cf")]
        extra = [circ(2.9, 9.3, 0.55, "#f3e9a8"), circ(3.2, 8.8, 0.35, "#e88aa0")]
        write("events", "guest-a.%d.svg" % i, svg(4, 16, figure(i, "#7fa6c9", "#e0b08c", hat=hat, extra=extra, skirt="#7fa6c9"), slot=S,
              note="cathedral guest in a dress and hat; frame %d of 2" % i))
        extra = [rect(1.75, 6.4, 0.5, 1.2, "#f4f1ea"), circ(1.6, 7.1, 0.3, "#e88aa0")]
        write("events", "guest-b.%d.svg" % i, svg(4, 16, figure(i, "#4f5a66", "#3c434d", head="#c58e66", extra=extra), slot=S,
              note="cathedral guest in a morning suit; frame %d of 2" % i))

    # The star-rating moment: a gold star and fireworks over the roof.
    def star(cx, cy, r0, r1):
        return [(cx + (r1 if k % 2 == 0 else r0) * math.cos(-math.pi / 2 + k * math.pi / 5),
                 cy + (r1 if k % 2 == 0 else r0) * math.sin(-math.pi / 2 + k * math.pi / 5)) for k in range(10)]

    st = [poly(star(8, 8.6, 3.2, 7.6), "#e0a92a"), poly(star(8, 8.6, 2.2, 5.6), "#f6c945"),
          poly([(8, 1.0), (8, 8.6), (3.8, 6.0)], "#fbdc75")]
    write("events", "rating-star.svg", svg(16, 16, st, units=(16, 16), note="the star shown when the tower gains a star; free sprite"))
    write("events", "rating-star.emit.svg", svg(16, 16, [circ(8, 8.6, 6.5, "#ffe9a0", op=0.18), poly(star(8, 8.6, 2.2, 5.6), "#fff0b0")],
          units=(16, 16)))
    FW = ["#ff6a5a", "#ffd166", "#7fd1ff", "#b88cff"]
    for i in range(5):
        b = []
        if i == 0:
            b.append(rect(15.7, 22, 0.6, 10, "#ffe2a0"))
        else:
            for c, (cx, cy) in enumerate(((16, 12), (8, 8), (24, 9))):
                if c >= i:
                    continue
                age = i - c
                rr = 2 + age * 2.2
                col = FW[(c + 1) % 4]
                for k in range(12):
                    a = k * math.pi / 6
                    b.append(circ(cx + rr * math.cos(a), cy + rr * math.sin(a), max(0.25, 0.75 - age * 0.12), col,
                                  op=max(0.3, 1 - age * 0.18)))
                if age == 1:
                    b.append(circ(cx, cy, 1.0, "#ffffff"))
        write("events", "fireworks.%d.svg" % i, svg(32, 32, b, units=(32, 32),
              note="fireworks over the roof when the tower gains a star; frame %d of 5, 5 a second" % i))
        write("events", "fireworks.%d.emit.svg" % i, svg(32, 32, b, units=(32, 32)))


if __name__ == "__main__":
    for d in DIRS.values():
        os.makedirs(d, exist_ok=True)
        for n in os.listdir(d):
            q = os.path.join(d, n)
            if n.endswith(".svg") and "pieces.py" in open(q).read(400):
                os.remove(q)
    building()
    world()
    events()
    print("%d files" % len(OUTPUT))
