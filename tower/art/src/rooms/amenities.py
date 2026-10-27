#!/usr/bin/env python3
"""Draw the BOX amenities' rooms and the security office's further pictures.

    python3 art/src/rooms/amenities.py

Writes, in art/src/rooms: laundry, guest-services, premier-lounge, pool,
gym (one floor) and convention (two floors), each as it stands empty and
as `.busy` (people in it, shown in its hours), with a night layer; and
security.1 and security.2, the security office's other two pictures
(security.0 is upstream's).  Drawn here in our own units: a cell is 4
wide, a floor 16 tall, people stand on y = 15.2 of their floor (STYLE.md).
It replaces only the SVGs that carry its marker comment.  Figures and
furniture come from fixed seeds, so a run gives the same files each time.
"""

import os
import random

HERE = os.path.dirname(os.path.abspath(__file__))
MARK = "Drawn for the BOX port of tower-together by art/src/rooms/amenities.py."
FLOOR_Y = 15.2


def f(v):
    s = ("%.2f" % v).rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def R(x, y, w, h, fill, rx=None, op=None):
    a = ' rx="%s"' % f(rx) if rx else ""
    o = ' opacity="%s"' % f(op) if op is not None else ""
    return '<rect x="%s" y="%s" width="%s" height="%s" fill="%s"%s%s/>' % (f(x), f(y), f(w), f(h), fill, a, o)


def C(cx, cy, r, fill, op=None):
    o = ' opacity="%s"' % f(op) if op is not None else ""
    return '<circle cx="%s" cy="%s" r="%s" fill="%s"%s/>' % (f(cx), f(cy), f(r), fill, o)


def E(cx, cy, rx, ry, fill, op=None):
    o = ' opacity="%s"' % f(op) if op is not None else ""
    return '<ellipse cx="%s" cy="%s" rx="%s" ry="%s" fill="%s"%s/>' % (f(cx), f(cy), f(rx), f(ry), fill, o)


def P(pts, fill, op=None):
    o = ' opacity="%s"' % f(op) if op is not None else ""
    return '<polygon points="%s" fill="%s"%s/>' % (" ".join("%s,%s" % (f(x), f(y)) for x, y in pts), fill, o)


def svg(w, h, body, slot, note):
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %s %s" width="%s" height="%s">\n'
            '  <!-- %s -->\n  <!-- slot: %gx%g -->\n  <!-- %s -->\n  %s\n</svg>\n'
            % (f(w), f(h), f(w), f(h), MARK, slot[0], slot[1], note, "\n  ".join(body)))


def emit(name, w, h):
    """The night layer: the day picture under warm interior light, as the
    other rooms' (STYLE.md, "Night layers")."""
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %s %s" width="%s" height="%s">\n'
            '<!-- %s -->\n<!-- Night layer for %s.svg (lit interior), as the other rooms\'. -->\n'
            '<defs>\n  <filter id="warm" x="0" y="0" width="1" height="1" color-interpolation-filters="sRGB">\n'
            '    <feColorMatrix type="matrix" values="1 0 0 0 0  0 0.9 0 0 0  0 0 0.68 0 0  0 0 0 1 0"/>\n'
            '  </filter>\n</defs>\n<image href="%s.svg" x="0" y="0" width="%s" height="%s" filter="url(#warm)" opacity="0.6"/>\n</svg>\n'
            % (f(w), f(h), f(w), f(h), MARK, name, name, f(w), f(h)))


def write(name, w, h, body, slot, note):
    with open(os.path.join(HERE, name + ".svg"), "w") as fh:
        fh.write(svg(w, h, body, slot, note))
    with open(os.path.join(HERE, name + ".emit.svg"), "w") as fh:
        fh.write(emit(name, w, h))


# ─── People, small, in the rooms' style ────────────────────────────────────────

SKINS = ["#e0b08c", "#c4936a", "#8d5a3b", "#f0c9a6", "#a8714c"]
CLOTHES = ["#2f5d50", "#3b5b8a", "#6b5a7e", "#7a6a4f", "#4a4f57", "#a04848", "#3a6a8a"]
HAIRS = ["#3a2a20", "#1f1a17", "#7a5230", "#b08850", "#5a5a5a"]


def person(x, rnd, top=None, legs="#3b4250", base=FLOOR_Y, facing=1, pose="stand"):
    """A figure about 10 units tall, feet on `base`, centred on x."""
    skin, hair = rnd.choice(SKINS), rnd.choice(HAIRS)
    top = top or rnd.choice(CLOTHES)
    b = []
    if pose == "stand":
        b += [R(x - 0.75, base - 4.1, 0.65, 4.1, legs, rx=0.25), R(x + 0.1, base - 4.1, 0.65, 4.1, legs, rx=0.25)]
        b += [R(x - 1.0, base - 8.2, 2.0, 4.4, top, rx=0.6)]
        b += [R(x - 1.45, base - 7.9, 0.5, 3.2, top, rx=0.25), R(x + 0.95, base - 7.9, 0.5, 3.2, top, rx=0.25)]
        hy = base - 9.3
    elif pose == "walk":
        b += [P([(x - 0.3, base - 4.1), (x + 0.3, base - 4.1), (x - 1.0, base), (x - 1.6, base)], legs),
              P([(x - 0.3, base - 4.1), (x + 0.3, base - 4.1), (x + 1.6, base), (x + 1.0, base)], legs)]
        b += [R(x - 1.0, base - 8.2, 2.0, 4.4, top, rx=0.6)]
        hy = base - 9.3
    elif pose == "sit":  # on a seat whose top is at base - 3.4
        b += [R(x - 0.7 * facing - 0.35, base - 3.4, 2.2 * facing if facing > 0 else 2.2, 0.8, legs, rx=0.3)]
        b += [R(x - 1.0, base - 7.4, 2.0, 4.2, top, rx=0.6)]
        hy = base - 8.5
    elif pose == "swim":  # head and shoulders above the water at `base`
        b += [E(x, base + 0.2, 1.4, 0.5, top)]
        hy = base - 0.7
    else:  # "run" on a treadmill
        b += [P([(x - 0.3, base - 4.1), (x + 0.3, base - 4.1), (x + 1.5, base - 0.6), (x + 0.9, base - 0.3)], legs),
              P([(x - 0.3, base - 4.1), (x + 0.3, base - 4.1), (x - 1.1, base - 1.6), (x - 1.6, base - 2.2)], legs)]
        b += [R(x - 1.0, base - 8.2, 2.0, 4.4, top, rx=0.6)]
        hy = base - 9.3
    b += [C(x, hy, 0.98, skin), P([(x - 1.0, hy - 0.2), (x - 0.7, hy - 1.0), (x + 0.6, hy - 1.05), (x + 1.0, hy - 0.4)], hair)]
    return b


def shell(w, wall, skirting, floor, h=16):
    return [R(0, 0, w, h, wall), R(0, h - 1.6, w, 0.8, skirting), R(0, h - 0.8, w, 0.8, floor)]


def lights(w, every, fill="#d8d4c0", cap="#7a7a72"):
    b = []
    x = every / 2
    while x < w:
        b += [R(x - 1.5, 0, 3, 0.5, cap), R(x - 1.0, 0.5, 2, 0.35, fill)]
        x += every
    return b


# ─── Laundry ───────────────────────────────────────────────────────────────────

def laundry(busy):
    rnd = random.Random(11)
    w = 48
    b = shell(w, "#b8c8d0", "#8a9aa4", "#9aa8b0") + lights(w, 12)
    # shelves of folded linen on the back wall
    for sx in (2, 30):
        b += [R(sx, 2.0, 14, 0.4, "#7a8a94"), R(sx, 5.2, 14, 0.4, "#7a8a94")]
        for k in range(5):
            c = ["#f4f4ee", "#e8d0d4", "#f4f4ee", "#d8e4ec", "#f4f4ee"][k]
            b += [R(sx + 0.6 + k * 2.7, 0.9, 2.3, 1.1, c, rx=0.2), R(sx + 0.6 + k * 2.7, 4.1, 2.3, 1.1, c, rx=0.2)]
    # washers and dryers, front-loading, along the left
    for i in range(4):
        x = 2 + i * 5.2
        b += [R(x, 8.6, 4.6, 5.8, "#eef2f4", rx=0.4), R(x, 8.6, 4.6, 1.0, "#d4dce0", rx=0.3),
              C(x + 2.3, 11.9, 1.6, "#5a6a74"), C(x + 2.3, 11.9, 1.2, "#3a8ac0" if busy and i % 2 == 0 else "#c8d4da")]
        if busy and i % 2 == 0:
            b += [E(x + 2.3, 12.4, 0.9, 0.35, "#e8f4fa", op=0.8)]
        b += [C(x + 3.8, 9.1, 0.25, "#60c060" if busy else "#7a8a90")]
    # folding table and a cart
    b += [R(25, 10.4, 12, 0.7, "#c8b088"), R(25.6, 11.1, 0.5, 3.3, "#8a7458"), R(36, 11.1, 0.5, 3.3, "#8a7458")]
    b += [R(39.5, 10.2, 6.5, 3.6, "#5a7a9a", rx=0.4), R(39.8, 9.4, 5.9, 1.0, "#f4f4ee", rx=0.4)]
    b += [C(40.3, 14.1, 0.45, "#2a2a2a"), C(45.2, 14.1, 0.45, "#2a2a2a")]
    if busy:
        b += [R(27, 9.6, 2.6, 0.8, "#f4f4ee", rx=0.2), R(31, 9.6, 2.6, 0.8, "#d8e4ec", rx=0.2),
              R(39.8, 8.4, 5.9, 1.2, "#e8d0d4", rx=0.4)]
        b += person(31, rnd, top="#b9a3d6") + person(23, rnd, top="#b9a3d6", pose="walk")
    return w, b


# ─── Guest services ────────────────────────────────────────────────────────────

def guest_services(busy):
    rnd = random.Random(23)
    w = 40
    b = shell(w, "#e8d8b8", "#a88858", "#c8a878") + lights(w, 10, "#f4e4b0")
    # panelled back wall, a clock-less wall plaque, two plants
    for x in range(2, 38, 6):
        b += [R(x, 1.5, 5, 9.5, "#e0cca8", rx=0.3)]
    b += [R(16, 2.2, 8, 3.2, "#b08840", rx=0.4), R(16.6, 2.8, 6.8, 2.0, "#d8b860", rx=0.3)]
    for px in (2.5, 37.5):
        b += [R(px - 1.0, 11.6, 2.0, 2.8, "#8a5a3a", rx=0.3), C(px, 10.2, 1.8, "#4a8a4a"), C(px - 0.9, 9.0, 1.1, "#5a9a5a")]
    # the concierge's desk with its bell
    b += [R(11, 9.2, 18, 5.2, "#7a4a2a", rx=0.4), R(10.6, 8.6, 18.8, 0.8, "#5a3420", rx=0.3),
          R(12, 10.2, 16, 3.4, "#8a5a36", rx=0.3), E(20, 8.4, 0.9, 0.55, "#d8b040"), R(19.1, 8.4, 1.8, 0.25, "#a88028")]
    # a luggage trolley, brass
    b += [R(31.5, 13.6, 5.5, 0.45, "#c8a040"), R(31.6, 6.5, 0.35, 7.2, "#c8a040"), R(36.6, 6.5, 0.35, 7.2, "#c8a040"),
          P([(31.6, 6.6), (34.3, 5.0), (37.0, 6.6), (36.6, 6.6), (34.3, 5.5), (31.9, 6.6)], "#c8a040"),
          C(32.2, 14.3, 0.45, "#2a2a2a"), C(36.3, 14.3, 0.45, "#2a2a2a")]
    if busy:
        b += [R(32, 10.4, 2.8, 3.2, "#b5543a", rx=0.3), R(34.9, 11.2, 1.6, 2.4, "#3a5a7a", rx=0.3)]
        # the concierge, behind the desk
        b += [R(19.0, 3.6, 2.0, 5.0, "#5a2a3a", rx=0.6), C(20, 2.7, 0.98, rnd.choice(SKINS)),
              R(19.4, 4.2, 1.2, 0.5, "#f4f4ee")]
        b += person(7, rnd, facing=1) + person(26.5, rnd, pose="walk")
    return w, b


# ─── Premier lounge ────────────────────────────────────────────────────────────

def premier_lounge(busy):
    rnd = random.Random(37)
    w = 80
    b = shell(w, "#3a2a20", "#24180f", "#5a3a26") + lights(w, 16, "#f0d090", "#5a4a3a")
    # wood panels and a long window on the city at night or day
    for x in range(1, 80, 8):
        b += [R(x, 1.2, 7, 12.2, "#4a3426", rx=0.3)]
    b += [R(26, 2.0, 28, 7.5, "#1a2a3a", rx=0.4), R(26.6, 2.6, 26.8, 6.3, "#2a4a6a", rx=0.3)]
    for i, (bx, bh) in enumerate([(27, 3.5), (30, 5.2), (33.5, 2.8), (36, 6.0), (40, 4.0), (44, 5.5), (48, 3.2), (50.5, 4.6)]):
        b += [R(bx, 8.9 - bh, 2.4, bh, "#1a3048")]
        b += [R(bx + 0.5, 8.9 - bh + 0.8, 0.5, 0.5, "#f0d070"), R(bx + 1.4, 8.9 - bh + 2.0, 0.5, 0.5, "#f0d070")]
    # the bar
    b += [R(60, 8.4, 17, 6.0, "#2a1a10", rx=0.4), R(59.6, 7.9, 17.8, 0.7, "#8a6a3a", rx=0.3),
          R(61, 2.0, 15, 0.4, "#8a6a3a"), R(61, 4.6, 15, 0.4, "#8a6a3a")]
    for k in range(8):
        c = ["#3a7a4a", "#8a2a2a", "#c8a040", "#3a5a8a"][k % 4]
        b += [R(61.8 + k * 1.8, 0.6, 0.7, 1.4, c, rx=0.2), R(61.8 + k * 1.8, 3.2, 0.7, 1.4, c, rx=0.2)]
    # armchairs and low tables, and two lamps
    chairs = [5, 13, 21, 36, 44]
    for cx in chairs:
        b += [R(cx - 2.2, 10.4, 4.4, 4.0, "#6a3a2a", rx=1.0), R(cx - 2.2, 9.0, 1.2, 5.4, "#5a2e20", rx=0.6),
              R(cx + 1.0, 9.0, 1.2, 5.4, "#5a2e20", rx=0.6)]
    for tx in (9, 40.5):
        b += [R(tx - 1.8, 12.0, 3.6, 0.5, "#2a1a10"), R(tx - 0.2, 12.5, 0.4, 2.0, "#2a1a10")]
    for lx in (17, 57):
        b += [R(lx - 0.15, 7.0, 0.3, 7.4, "#2a2a2a"), P([(lx - 1.3, 7.2), (lx + 1.3, 7.2), (lx + 0.8, 5.2), (lx - 0.8, 5.2)], "#f0d8a0")]
    if busy:
        for cx in (5, 21, 44):
            b += person(cx, rnd, base=14.4, pose="sit")
        b += [R(8.4, 11.2, 0.5, 0.8, "#c8a040"), R(40.0, 11.2, 0.5, 0.8, "#8a2a2a")]
        b += [R(67.0, 3.4, 2.0, 4.6, "#f4f4ee", rx=0.6), C(68, 2.6, 0.98, rnd.choice(SKINS)), R(67.6, 3.6, 0.8, 0.5, "#1a1a1a")]
        b += person(30, rnd, top="#2a3a5a") + person(56, rnd, top="#4a4f57", pose="walk")
    return w, b


# ─── Swimming pool ─────────────────────────────────────────────────────────────

def pool(busy):
    rnd = random.Random(53)
    w = 120
    b = shell(w, "#d8eef0", "#9ac0c8", "#b8d8de") + lights(w, 15, "#e6f2ff", "#8aa4ac")
    # tiles on the wall: a grout grid, a band of blue
    for x in range(0, 120, 3):
        b.append(R(x, 0, 0.15, 13.2, "#c0dade"))
    for y in (3.0, 6.0, 9.0):
        b.append(R(0, y, w, 0.15, "#c0dade"))
    b += [R(0, 7.4, w, 0.9, "#4a90b8")]
    # the pool: water across the middle, its edge, the lane ropes
    b += [R(10, 10.8, 100, 4.4, "#3aa0d0"), R(10, 10.4, 100, 0.5, "#e8f4f6"), R(10, 10.9, 100, 0.6, "#5ab8e0")]
    for x in range(12, 110, 6):
        b += [R(x, 12.3, 3.0, 0.25, "#8ad0f0")]
    for k in range(9):
        b += [C(12 + k * 12, 11.2, 0.3, "#d84a3a"), C(18 + k * 12, 11.2, 0.3, "#f0f0f0")]
    # a ladder, the lifeguard's chair, loungers
    b += [R(104, 7.2, 0.35, 4.0, "#c8d0d4"), R(106, 7.2, 0.35, 4.0, "#c8d0d4"),
          R(104, 8.2, 2.35, 0.25, "#c8d0d4"), R(104, 9.4, 2.35, 0.25, "#c8d0d4")]
    b += [R(3.0, 5.0, 4.0, 0.6, "#d84a3a"), R(3.2, 5.6, 0.4, 9.0, "#f4f4ee"), R(6.4, 5.6, 0.4, 9.0, "#f4f4ee"),
          R(3.2, 9.0, 3.6, 0.35, "#f4f4ee"), R(3.0, 3.0, 4.0, 2.0, "#d84a3a", rx=0.4)]
    for lx in (112.5,):
        b += [P([(lx - 3.5, 13.4), (lx + 2.0, 13.4), (lx + 3.5, 11.6), (lx + 3.0, 11.4), (lx + 1.6, 12.8), (lx - 3.5, 12.8)], "#f0f0e8"),
              R(lx - 3.2, 13.4, 0.3, 1.0, "#a0a8ac"), R(lx + 1.6, 13.4, 0.3, 1.0, "#a0a8ac")]
    if busy:
        for sx in (20, 37, 51, 70, 88):
            b += person(sx, rnd, pose="swim", base=11.6, top=rnd.choice(["#d84a3a", "#2a5a9a", "#1a1a1a"]))
            b += [E(sx + 2.2, 11.7, 1.4, 0.25, "#e8f8ff", op=0.7)]
        b += [R(3.8, 5.6, 2.4, 3.4, "#d84a3a", rx=0.6), C(5, 4.7, 0.98, rnd.choice(SKINS))]
        b += [R(108.6, 12.0, 4.4, 0.8, "#3a6a8a", rx=0.4), C(114.6, 11.6, 0.9, rnd.choice(SKINS))]
    return w, b


# ─── Gym ───────────────────────────────────────────────────────────────────────

def gym(busy):
    rnd = random.Random(67)
    w = 64
    b = shell(w, "#c8ccd0", "#5a5e64", "#3a3a40") + lights(w, 12, "#f0f4f8")
    # a mirror along the wall, a stripe of the gym's colour
    b += [R(2, 1.5, 60, 7.2, "#a8c0cc"), R(2, 1.5, 60, 0.4, "#e0eaee")]
    for x in (10, 26, 41, 55):
        b += [P([(x, 1.9), (x + 2.5, 1.9), (x - 1.0, 8.7), (x - 3.5, 8.7)], "#c8dae2", op=0.6)]
    b += [R(0, 9.0, w, 0.6, "#e07a2a")]
    # treadmills
    for tx in (4, 12, 20):
        b += [P([(tx, 14.4), (tx + 6.0, 14.4), (tx + 6.0, 13.4), (tx, 13.6)], "#2a2a30"),
              R(tx + 5.4, 9.4, 0.5, 4.2, "#5a5e64"), R(tx + 4.6, 9.0, 2.0, 1.0, "#2a2a30", rx=0.3),
              R(tx + 4.9, 9.2, 1.4, 0.5, "#4ac0e0" if busy else "#3a4a54")]
    # a weights rack, a bench with a bar, a bike
    b += [R(30, 10.2, 7, 0.4, "#5a5e64"), R(30, 12.4, 7, 0.4, "#5a5e64"), R(30.2, 10.2, 0.4, 4.2, "#5a5e64"),
          R(36.4, 10.2, 0.4, 4.2, "#5a5e64")]
    for k in range(5):
        b += [C(31.2 + k * 1.25, 11.8, 0.55, "#2a2a30"), C(31.2 + k * 1.25, 14.0, 0.45, "#2a2a30")]
    b += [R(41, 12.0, 6.5, 0.9, "#a03a2a", rx=0.3), R(42, 12.9, 0.4, 1.5, "#5a5e64"), R(46.2, 12.9, 0.4, 1.5, "#5a5e64"),
          R(46.8, 9.0, 0.4, 5.4, "#5a5e64"), R(40.6, 9.0, 0.4, 5.4, "#5a5e64")]
    b += [R(39.0, 8.6, 10.0, 0.35, "#9aa0a6"), R(39.0, 7.8, 0.6, 1.9, "#2a2a30"), R(48.4, 7.8, 0.6, 1.9, "#2a2a30")]
    b += [C(53.5, 13.4, 1.0, "#2a2a30"), C(58.5, 13.4, 1.0, "#2a2a30"), P([(53.5, 13.4), (56.5, 10.6), (58.5, 13.4)], "#e07a2a"),
          R(55.6, 10.2, 1.8, 0.4, "#2a2a30"), R(57.4, 9.6, 0.4, 1.5, "#5a5e64")]
    if busy:
        for tx in (4, 12):
            b += person(tx + 2.8, rnd, base=13.6, pose="run", top=rnd.choice(["#d84a3a", "#2a7a5a", "#3b5b8a"]))
        b += [R(42.0, 10.9, 5.0, 1.2, rnd.choice(CLOTHES), rx=0.5), C(47.4, 11.4, 0.9, rnd.choice(SKINS))]
        b += person(56.0, rnd, base=12.0, pose="sit", top="#e07a2a")
        b += person(28, rnd, top="#4a4f57", pose="walk")
    return w, b


# ─── Convention centre (two floors) ────────────────────────────────────────────

def convention(busy):
    rnd = random.Random(79)
    w, h = 120, 32
    b = shell(w, "#5a6478", "#3a4252", "#8a6a4a", h=h)
    base = h - 0.8
    # the hall: a coffered ceiling, chandeliers, a stage and its screen
    for x in range(0, 120, 10):
        b += [R(x + 0.5, 0.5, 9, 2.2, "#4a5266", rx=0.3)]
    for cx in (30, 60, 90):
        b += [R(cx - 0.15, 2.7, 0.3, 2.5, "#a89060"), E(cx, 5.8, 3.2, 1.0, "#d8b040"), E(cx, 5.6, 2.6, 0.5, "#f8e0a0")]
    b += [R(2, 20.6, 34, 10.6, "#3a2a20"), R(2, 20.0, 34, 0.8, "#8a6a3a")]           # the stage
    b += [R(4, 6.0, 30, 12.0, "#1a1a20", rx=0.4), R(5, 7.0, 28, 10.0, "#4a6a9a" if busy else "#2a3040", rx=0.3)]
    if busy:  # the screen: a chart and a picture, no lettering
        for k, hh in enumerate([3, 5, 4, 7, 6]):
            b += [R(7 + k * 3, 15.6 - hh, 2, hh, "#f0b429")]
        b += [P([(23, 15.5), (27, 10.0), (29.5, 13.0), (31, 11.5), (31, 15.5)], "#3d9b4f"), C(28.5, 9.6, 1.0, "#f4f4ee")]
    b += [R(26, 15.2, 4.0, 5.0, "#5a3a26"), R(25.4, 14.8, 5.2, 0.7, "#7a5a3a")]       # the lectern
    for x in (0.5, 117):
        b += [R(x, 3.0, 2.5, 28.2, "#8a2a3a"), R(x + 0.4, 3.0, 0.4, 28.2, "#6a1a2a")]  # drapes
    # rows of chairs, five deep, climbing gently towards the back
    rows = [(40, 0.0), (55, 0.0), (70, 0.0), (85, 0.0), (100, 0.0)]
    for rx, _ in rows:
        for k in range(4):
            x = rx + k * 3.2 - 4.8
            b += [R(x, base - 3.4, 2.4, 0.5, "#a03a4a", rx=0.2), R(x + 1.9, base - 6.4, 0.6, 3.5, "#a03a4a", rx=0.2),
                  R(x + 0.2, base - 2.9, 0.3, 2.9, "#3a3a40"), R(x + 1.9, base - 2.9, 0.3, 2.9, "#3a3a40")]
    if busy:
        b += person(28, rnd, top="#2a3a5a", base=20.0)
        for rx, _ in rows:
            for k in range(4):
                if rnd.random() < 0.8:
                    x = rx + k * 3.2 - 4.8 + 1.1
                    b += person(x, rnd, base=base, pose="sit", facing=-1)
    return w, b


# ─── Kindergarten and primary school ──────────────────────────────────────────

def child(x, rnd, base=FLOOR_Y, pose="stand", top=None):
    """A child: a person at three fifths, feet on `base`."""
    t = '<g transform="translate(%s %s) scale(0.6) translate(%s %s)">' % (f(x), f(base), f(-x), f(-base))
    return [t] + person(x, rnd, top=top or rnd.choice(["#e0503a", "#3a8ad0", "#f0b429", "#3d9b4f", "#9a5ac8"]),
                        legs=rnd.choice(["#3b4250", "#2a5a9a", "#a04848"]), base=base, pose=pose) + ["</g>"]


def kindergarten(busy):
    rnd = random.Random(131)
    w = 48
    b = shell(w, "#f6e7b8", "#e0a040", "#c8d870") + lights(w, 12, "#fff6d0")
    # a frieze of coloured shapes, windows, a cloth play mat, little tables
    for i, c in enumerate(["#e0503a", "#f0b429", "#3d9b4f", "#3a8ad0", "#9a5ac8"] * 4):
        b.append(R(1 + i * 2.35, 1.2, 1.6, 1.6, c, rx=0.8 if i % 2 else 0.2))
    for wx in (4, 30):
        b += [R(wx, 4.0, 10, 5.0, "#a8d8f0", rx=0.4), R(wx + 4.8, 4.0, 0.4, 5.0, "#e8d8a8"), R(wx, 6.4, 10, 0.3, "#e8d8a8")]
    b += [R(16, 13.6, 12, 0.8, "#e0503a", rx=0.3), R(18, 13.6, 2, 0.8, "#f0b429"), R(22, 13.6, 2, 0.8, "#3a8ad0")]
    for tx in (4, 34):
        b += [R(tx, 11.6, 8, 0.6, "#f0b429", rx=0.2), R(tx + 0.6, 12.2, 0.4, 2.2, "#c88a20"), R(tx + 7.0, 12.2, 0.4, 2.2, "#c88a20")]
        for cx in (tx - 1.2, tx + 9.0):
            b += [R(cx, 12.6, 1.4, 0.4, "#3a8ad0"), R(cx + (0 if cx < tx else 1.0), 11.0, 0.4, 1.8, "#3a8ad0"),
                  R(cx + 0.2, 13.0, 0.3, 1.4, "#2a6aa0"), R(cx + 1.0, 13.0, 0.3, 1.4, "#2a6aa0")]
    # an easel, a box of toys, a rocking horse
    b += [P([(28.5, 14.4), (29.6, 8.5), (30.7, 14.4), (30.3, 14.4), (29.6, 10.0), (28.9, 14.4)], "#a07040"),
          R(28.4, 8.6, 2.6, 3.0, "#ffffff"), C(29.2, 9.6, 0.5, "#e0503a"), C(30.0, 10.5, 0.4, "#3a8ad0")]
    b += [R(44, 12.0, 3.4, 2.4, "#3d9b4f", rx=0.3), C(44.8, 11.6, 0.5, "#e0503a"), R(45.6, 11.0, 0.9, 1.0, "#f0b429")]
    if busy:
        for cx in (6.0, 9.5, 36.5, 39.5):
            b += child(cx, rnd, base=13.0, pose="sit")
        for cx in (18.5, 21.0, 25.0):
            b += child(cx, rnd, base=13.6, pose="sit")
        b += child(43.0, rnd) + child(14.0, rnd, pose="walk")
        b += person(31.5, rnd, top="#6a8a4a") + person(23.0, rnd, top="#a04878")      # the teachers
    return w, b


def classroom(rnd, y0, busy, board):
    """One floor of the school: desks in rows facing a board."""
    b = []
    base = y0 + FLOOR_Y
    b += [R(2, y0 + 2.5, 22, 7.5, "#2e5a40", rx=0.3), R(2, y0 + 10.0, 22, 0.5, "#a07040")] if board else \
         [R(2, y0 + 2.0, 26, 8.0, "#e8ecef", rx=0.3), R(2.6, y0 + 2.6, 24.8, 6.8, "#c8d8e0")]
    if board:
        # chalk: lines of writing (marks, no letters) and a chart's axes
        b += [R(4, y0 + 4.0, 7, 0.3, "#e8ecef"), R(4, y0 + 5.2, 9, 0.3, "#e8ecef"), R(4, y0 + 6.4, 5, 0.3, "#e8ecef"),
              R(15, y0 + 4.2, 0.3, 4.0, "#e8ecef"), R(15, y0 + 8.0, 6, 0.3, "#e8ecef")]
    for wx in (64, 90):
        b += [R(wx, y0 + 2.5, 20, 7.0, "#a8d8f0", rx=0.4), R(wx + 9.8, y0 + 2.5, 0.4, 7.0, "#e8ecef")]
    for row, dx in enumerate(range(30, 112, 9)):
        b += [R(dx, base - 3.6, 6, 0.6, "#c8a070", rx=0.2), R(dx + 0.4, base - 3.0, 0.4, 3.0, "#8a6a40"),
              R(dx + 5.2, base - 3.0, 0.4, 3.0, "#8a6a40")]
        if busy and (row * 7 + int(y0)) % 5 != 3:
            b += child(dx + 1.8, rnd, base=base - 0.5, pose="sit") + child(dx + 4.2, rnd, base=base - 0.5, pose="sit")
    if busy:
        b += person(25.5 if board else 29.0, rnd, top=rnd.choice(["#6a5a8a", "#3a6a8a", "#8a4a3a"]), base=base)
    return b


def school(busy):
    rnd = random.Random(149)
    w, h = 120, 32
    b = shell(w, "#e8dcc0", "#a08a60", "#b8a078", h=h)
    b += [R(0, 15.2, w, 0.8, "#a08a60"), R(0, 16.0, w, 0.4, "#8a7450")]                   # the floor between
    b += lights(w, 15, "#f8f0d8")
    b += [R(x - 1.5, 16.4, 3, 0.5, "#7a7a72") for x in range(8, 120, 15)]
    b += classroom(rnd, 0.0, busy, True) + classroom(rnd, 16.0, busy, False)
    return w, h, b


# ─── Sprinkler room ────────────────────────────────────────────────────────────

def sprinklers():
    w = 32
    b = shell(w, "#c4c8cc", "#8a9096", "#9aa0a6") + lights(w, 16)
    # the water tank on the left, its gauge
    b += [R(1.5, 2.5, 9, 12, "#7a8a9a", rx=1.2), R(1.5, 4.5, 9, 0.5, "#5a6a7a"), R(1.5, 12.0, 9, 0.5, "#5a6a7a"),
          C(6, 8.2, 1.4, "#f4f4ee"), C(6, 8.2, 1.1, "#e8ecef"), P([(6, 8.2), (6.8, 7.4), (6.2, 8.4)], "#d84a3a")]
    # the main pipe along the ceiling, risers down to two pumps
    b += [R(10.5, 1.4, 21, 1.2, "#c83a2e"), R(13.4, 2.6, 1.0, 7.4, "#c83a2e"), R(21.4, 2.6, 1.0, 7.4, "#c83a2e")]
    for px in (11.4, 19.4):
        b += [R(px, 10.0, 5.0, 4.4, "#c83a2e", rx=0.6), R(px + 0.5, 10.6, 4.0, 1.0, "#e05a4a", rx=0.3),
              C(px + 2.5, 12.6, 1.0, "#8a2a22"), R(px - 0.4, 14.0, 5.8, 0.4, "#3a3a3a")]
    # valve wheels on the risers
    for vx in (13.9, 21.9):
        b += [C(vx, 6.0, 1.3, "#d8a020"), C(vx, 6.0, 0.8, "#c4c8cc"), R(vx - 1.3, 5.8, 2.6, 0.4, "#d8a020"),
              R(vx - 0.2, 4.7, 0.4, 2.6, "#d8a020")]
    # the alarm bell and a control box on the right
    b += [C(28.5, 4.0, 1.5, "#c83a2e"), C(28.5, 4.0, 0.6, "#8a2a22"),
          R(26.0, 7.0, 5.0, 6.0, "#e8e4d8", rx=0.4), R(26.6, 7.6, 3.8, 1.4, "#3a4a58"),
          C(27.4, 10.6, 0.35, "#3d9b4f"), C(28.6, 10.6, 0.35, "#3d9b4f"), C(29.8, 10.6, 0.35, "#f0b429")]
    # sprinkler heads along the pipe
    for hx in (25.0, 31.0):
        b += [R(hx - 0.2, 2.6, 0.4, 0.6, "#a0a0a0"), P([(hx - 0.7, 3.4), (hx + 0.7, 3.4), (hx, 3.0)], "#a0a0a0")]
    return w, b


# ─── The security office's further pictures ────────────────────────────────────

def security(variant):
    rnd = random.Random(97 + variant)
    w = 64
    b = shell(w, "#3a4858", "#202830", "#405060") + lights(w, 16, "#d0d0b0", "#888878")
    if variant == 1:
        # a control room: one great wall of screens, a long console, lockers
        b += [R(10, 1.5, 36, 8.0, "#1a2028", rx=0.4)]
        for r in range(2):
            for c in range(6):
                col = rnd.choice(["#2a4050", "#2a4a3a", "#3a3050", "#40402a"])
                b += [R(10.8 + c * 5.9, 2.2 + r * 3.7, 5.3, 3.2, col, rx=0.2),
                      R(11.4 + c * 5.9, 2.8 + r * 3.7, 2.0 + rnd.random() * 2, 0.4, "#6a8a9a")]
        b += [R(8, 11.0, 40, 3.4, "#5a4a3a", rx=0.4), R(7.6, 10.4, 40.8, 0.8, "#3a2e24", rx=0.3)]
        for kx in (14, 26, 38):
            b += [R(kx, 9.6, 4.0, 0.8, "#2a3038", rx=0.2), C(kx + 4.6, 10.1, 0.3, "#d84a3a"), C(kx + 5.3, 10.1, 0.3, "#3d9b4f")]
        for lx in (52, 55.4, 58.8):
            b += [R(lx, 3.5, 3.0, 11.0, "#5a6878", rx=0.3), R(lx + 2.2, 7.8, 0.3, 1.4, "#c8d0d8")]
        b += [P([(3, 2), (7, 2), (7, 6), (5, 7.6), (3, 6)], "#a8b0b8"), R(4.7, 3.0, 0.6, 3.0, "#4a5868"), R(3.7, 4.0, 2.6, 0.6, "#4a5868")]
        b += person(20, rnd, top="#2f4a7a", base=FLOOR_Y) + person(34, rnd, top="#2f4a7a", base=14.4, pose="sit")
    else:
        # the briefing room: a floor plan on the wall, radios charging, a table
        b += [R(4, 1.6, 24, 9.0, "#e8ecef", rx=0.3)]
        for (x, y, ww, hh) in [(5, 2.6, 7, 3.4), (12.4, 2.6, 6, 3.4), (18.8, 2.6, 8.2, 3.4), (5, 6.4, 10, 3.2), (15.4, 6.4, 11.6, 3.2)]:
            b += [R(x, y, ww, hh, "#c8d4dc"), R(x, y, ww, 0.2, "#5a6878"), R(x, y, 0.2, hh, "#5a6878")]
        b += [C(22, 4.4, 0.5, "#d84a3a"), C(9, 8.0, 0.5, "#f0b429")]
        b += [R(31, 3.0, 12, 0.4, "#5a6878")]
        for k in range(6):
            b += [R(31.6 + k * 1.9, 1.4, 0.9, 1.6, "#1a1a1a", rx=0.2), C(32.05 + k * 1.9, 1.2, 0.15, "#3d9b4f")]
        b += [R(30, 10.6, 22, 0.8, "#6a5a4a", rx=0.3), R(31, 11.4, 0.5, 3.0, "#3a2e24"), R(50.5, 11.4, 0.5, 3.0, "#3a2e24")]
        for cx in (33, 39, 45, 51):
            b += [R(cx - 1.0, 11.0, 2.0, 0.6, "#2a3038"), R(cx - 1.0, 8.6, 0.5, 2.6, "#2a3038")]
        b += [R(55, 5.0, 6, 9.4, "#2a3038", rx=0.3), R(55.6, 5.6, 4.8, 3.0, "#3a4a58"), C(58, 11.0, 1.2, "#5a6878")]
        b += person(16, rnd, top="#2f4a7a") + person(39, rnd, top="#2f4a7a", base=14.4, pose="sit") + \
            person(45, rnd, top="#2f4a7a", base=14.4, pose="sit")
    return w, b


def main():
    for n in os.listdir(HERE):
        p = os.path.join(HERE, n)
        if n.endswith(".svg") and MARK in open(p).read(600):
            os.remove(p)
    one = [("laundry", laundry, 12), ("guest-services", guest_services, 10), ("premier-lounge", premier_lounge, 20),
           ("pool", pool, 30), ("gym", gym, 16)]
    count = 0
    for name, fn, cells in one:
        for busy in (False, True):
            w, b = fn(busy)
            write(name + (".busy" if busy else ""), w, 16, b, (cells, 1),
                  "%s, %s" % (name.replace("-", " "), "in its hours, people in it" if busy else "open, nobody in it"))
            count += 2
    for busy in (False, True):
        w, b = convention(busy)
        write("convention" + (".busy" if busy else ""), w, 32, b, (30, 2),
              "convention centre, two floors, %s" % ("a conference on" if busy else "empty"))
        count += 2
    for busy in (False, True):
        w, b = kindergarten(busy)
        write("kindergarten" + (".busy" if busy else ""), w, 16, b, (12, 1),
              "kindergarten, %s" % ("children in it" if busy else "empty"))
        w, h, b = school(busy)
        write("school" + (".busy" if busy else ""), w, h, b, (30, 2),
              "primary school, two floors, %s" % ("lessons on" if busy else "empty"))
        count += 4
    w, b = sprinklers()
    write("sprinklers", w, 16, b, (8, 1), "sprinkler room: a tank, two pumps, the alarm bell")
    count += 1
    for v in (1, 2):
        w, b = security(v)
        write("security.%d" % v, w, 16, b, (16, 1), "security office, picture %d of 3" % (v + 1))
        count += 2
    print("%d files" % count)


if __name__ == "__main__":
    main()
