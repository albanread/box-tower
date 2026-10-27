#!/usr/bin/env python3
"""Draw the vehicles: ordinary cars in side view, facing right and left,
two wheel frames each, with a night layer for the lamps.  Rewrites the
SVGs in this directory.  NOTES.md has the sizes.

Every vehicle is 24 x 10 units (6 cells long, well under a floor high),
the tyres touching y = 10, so the picture's bottom edge goes on the floor
surface.  Right-facing pictures have the front at the right.
"""

import math
import os

HERE = os.path.dirname(os.path.abspath(__file__))
W, H = 24, 10
WR = 1.65          # wheel radius
WY = H - WR        # wheel centre
GLASS, GLASS2 = "#9cc0d6", "#c9dfec"
TYRE, HUB, HUB2 = "#26282b", "#9aa1a8", "#5d636a"
HEAD, TAIL = "#f3ecc8", "#c8302b"
HEAD_E, TAIL_E = "#fff4cf", "#ff3a2a"


def f(v):
    s = ("%.3f" % v).rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def P(pts, fill, extra=""):
    return '<polygon points="%s" fill="%s"%s/>' % (
        " ".join("%s,%s" % (f(x), f(y)) for x, y in pts), fill, extra)


def R(x, y, w, h, fill, rx=0):
    r = ' rx="%s"' % f(rx) if rx else ""
    return '<rect x="%s" y="%s" width="%s" height="%s"%s fill="%s"/>' % (f(x), f(y), f(w), f(h), r, fill)


def C(cx, cy, r, fill):
    return '<circle cx="%s" cy="%s" r="%s" fill="%s"/>' % (f(cx), f(cy), f(r), fill)


def wheel(cx, frame, WY=WY, WR=WR):
    out = [C(cx, WY, WR, TYRE), C(cx, WY, WR * 0.58, HUB)]
    a0 = math.pi / 4 * frame  # frame 1 turns the spokes 45 degrees
    for i in range(2):
        a = a0 + i * math.pi / 2
        dx, dy = math.cos(a) * WR * 0.55, math.sin(a) * WR * 0.55
        nx, ny = -math.sin(a) * 0.14, math.cos(a) * 0.14
        out.append(P([(cx - dx + nx, WY - dy + ny), (cx + dx + nx, WY + dy + ny),
                      (cx + dx - nx, WY + dy - ny), (cx - dx - nx, WY - dy - ny)], HUB2))
    out.append(C(cx, WY, 0.28, HUB2))
    return out


def shade(hexc, k):
    h = hexc.lstrip("#")
    r, g, b = (int(h[i:i + 2], 16) for i in (0, 2, 4))
    return "#%02x%02x%02x" % tuple(max(0, min(255, int(c * k))) for c in (r, g, b))


# Each type: body polygon (front at right), glass polygons, pillars, the
# wheel centres, lamp boxes (front, rear) and extras.
TYPES = {
    "saloon": dict(colour="#b8413a",
        body=[(1, 8.6), (1, 5.9), (2.2, 5.2), (6.4, 5.0), (9.2, 2.5), (15.6, 2.5), (18.8, 5.0),
              (22.4, 5.4), (23.2, 6.3), (23.2, 8.6)],
        glass=[[(7.4, 4.95), (9.7, 3.1), (12.1, 3.1), (12.1, 4.95)],
               [(12.8, 4.95), (12.8, 3.1), (15.2, 3.1), (17.6, 4.95)]],
        wheels=(5.4, 18.8), front=(22.3, 5.8, 0.9, 0.7), rear=(1, 5.9, 0.8, 0.8)),
    "hatchback": dict(colour="#3f78b5",
        body=[(2.6, 8.6), (2.6, 4.4), (3.9, 2.4), (13.8, 2.4), (17.4, 4.9), (20.8, 5.3),
              (21.6, 6.2), (21.6, 8.6)],
        glass=[[(4.3, 4.6), (5.0, 3.0), (8.4, 3.0), (8.4, 4.6)],
               [(9.1, 4.6), (9.1, 3.0), (13.5, 3.0), (16.2, 4.6)]],
        wheels=(6.0, 18.0), front=(20.7, 5.7, 0.9, 0.7), rear=(2.6, 4.6, 0.7, 1.2)),
    "estate": dict(colour="#4f7a52",
        body=[(1, 8.6), (1, 4.6), (1.8, 2.4), (15.6, 2.4), (18.9, 5.0), (22.4, 5.4),
              (23.2, 6.3), (23.2, 8.6)],
        glass=[[(2.4, 4.8), (2.9, 3.0), (6.6, 3.0), (6.6, 4.8)],
               [(7.3, 4.8), (7.3, 3.0), (11.6, 3.0), (11.6, 4.8)],
               [(12.3, 4.8), (12.3, 3.0), (15.3, 3.0), (17.7, 4.8)]],
        wheels=(5.4, 18.8), front=(22.3, 5.8, 0.9, 0.7), rear=(1, 4.8, 0.7, 1.4),
        extra=[("rail", (3, 2.0, 11.5, 0.4))]),
    "van": dict(colour="#e4e1d8",
        body=[(1, 8.6), (1, 1.4), (1.6, 0.8), (16.6, 0.8), (20.6, 4.4), (22.8, 5.0),
              (23.2, 6.0), (23.2, 8.6)],
        glass=[[(16.9, 4.6), (16.9, 1.6), (17.4, 1.6), (20.0, 4.6)]],
        wheels=(5.0, 19.0), front=(22.3, 5.4, 0.9, 0.8), rear=(1, 2.0, 0.6, 1.6),
        extra=[("seam", (14.2, 1.2, 0.25, 6.6)), ("seam", (9.0, 1.2, 0.25, 6.6)),
               ("handle", (13.2, 5.2, 0.8, 0.3)), ("stripe", (1, 5.6, 15.6, 0.7))]),
    "taxi": dict(colour="#e2b13c",
        body=[(1, 8.6), (1, 5.9), (2.2, 5.2), (6.4, 5.0), (9.2, 2.5), (15.6, 2.5), (18.8, 5.0),
              (22.4, 5.4), (23.2, 6.3), (23.2, 8.6)],
        glass=[[(7.4, 4.95), (9.7, 3.1), (12.1, 3.1), (12.1, 4.95)],
               [(12.8, 4.95), (12.8, 3.1), (15.2, 3.1), (17.6, 4.95)]],
        wheels=(5.4, 18.8), front=(22.3, 5.8, 0.9, 0.7), rear=(1, 5.9, 0.8, 0.8),
        extra=[("sign", (10.6, 1.3, 3.4, 1.2)), ("band", (2, 6.3, 20.4, 0.5))]),
    "pickup": dict(colour="#6b6f78",
        body=[(1, 8.6), (1, 4.6), (10.6, 4.6), (10.6, 2.3), (15.4, 2.3), (18.6, 4.9),
              (22.4, 5.3), (23.2, 6.2), (23.2, 8.6)],
        glass=[[(11.3, 4.7), (11.3, 3.0), (14.9, 3.0), (17.3, 4.7)]],
        wheels=(5.2, 18.8), front=(22.3, 5.7, 0.9, 0.7), rear=(1, 4.8, 0.6, 1.2),
        extra=[("seam", (10.0, 4.6, 0.3, 3.9)), ("tail", (1, 4.6, 9.0, 0.4))]),
}


def draw(t, k, frame):
    col = k["colour"]
    dark, light = shade(col, 0.72), shade(col, 1.12)
    b = [P(k["body"], col)]
    # sill shadow along the bottom, and a waist line
    b.append(R(1 if t not in ("hatchback",) else 2.6, 7.6, 22.2 if t != "hatchback" else 19, 1.0, dark))
    for g in k["glass"]:
        b.append(P(g, GLASS))
        x0, y0 = g[0][0], g[-1][1] if False else g[0][1]
        gx = min(p[0] for p in g) + 0.6
        gy = min(p[1] for p in g)
        b.append(P([(gx, gy + 1.4), (gx + 0.35, gy + 1.4), (gx + 1.0, gy + 0.3), (gx + 0.65, gy + 0.3)],
                   GLASS2))
    for kind, (x, y, w, h) in k.get("extra", []):
        if kind == "rail":
            b.append(R(x, y, w, h, dark))
        elif kind == "seam":
            b.append(R(x, y, w, h, dark))
        elif kind == "handle":
            b.append(R(x, y, w, h, dark))
        elif kind == "stripe":
            b.append(R(x, y, w, h, "#3f78b5"))
        elif kind == "sign":
            b.append(R(x + 0.6, y + h - 0.3, w - 1.2, 0.5, dark))
            b.append(R(x, y, w, h, "#f6f0dc", rx=0.4))
        elif kind == "band":
            for i in range(int(w // 1.0)):
                b.append(R(x + i * 1.0, y, 0.5, h, "#2e2d2b" if i % 2 == 0 else col))
        elif kind == "tail":
            b.append(R(x, y, w, h, light))
    # door shut lines and a handle for the cars
    if t in ("saloon", "taxi"):
        b += [R(12.2, 5.0, 0.2, 2.6, dark), R(17.6, 5.0, 0.2, 2.6, dark), R(13.2, 5.4, 0.8, 0.25, dark)]
    if t == "hatchback":
        b += [R(8.6, 4.6, 0.2, 3.0, dark), R(9.6, 5.0, 0.8, 0.25, dark)]
    if t == "estate":
        b += [R(11.8, 4.8, 0.2, 2.8, dark), R(6.8, 4.8, 0.2, 2.8, dark), R(12.6, 5.2, 0.8, 0.25, dark)]
    # wheel arches
    for cx in k["wheels"]:
        b.append(C(cx, WY, WR + 0.45, dark))
    # lamps and bumpers
    fx, fy, fw, fh = k["front"]
    rx, ry, rw, rh = k["rear"]
    b.append(R(fx, fy, fw, fh, HEAD, rx=0.2))
    b.append(R(rx, ry, rw, rh, TAIL, rx=0.2))
    xl = min(p[0] for p in k["body"])
    xr = max(p[0] for p in k["body"])
    b.append(R(xr - 1.2, 7.4, 1.4, 0.7, "#3a3c40", rx=0.2))
    b.append(R(xl - 0.2, 7.4, 1.4, 0.7, "#3a3c40", rx=0.2))
    for cx in k["wheels"]:
        b += wheel(cx, frame)
    e = [R(fx, fy, fw, fh, HEAD_E, rx=0.2),
         P([(fx + fw, fy - 0.2), (W, fy - 0.6), (W, fy + fh + 0.8), (fx + fw, fy + fh + 0.2)],
           HEAD_E, ' fill-opacity="0.35"'),
         R(rx, ry, rw, rh, TAIL_E, rx=0.2)]
    if t == "taxi":
        e.append(R(10.6, 1.3, 3.4, 1.2, "#ffe9a8", rx=0.4))
    return b, e


def svg(body, comment, w=W, h=H):
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %d %d" width="%d" height="%d">\n'
            '  <!-- units: %dx%d -->\n  <!-- %s -->\n  %s\n</svg>\n'
            % (w, h, w, h, w, h, comment, "\n  ".join(body)))


def mirror(body, w=W):
    return ['<g transform="translate(%d,0) scale(-1,1)">' % w] + body + ["</g>"]


# ---- emergency vehicles ---------------------------------------------------
# Each has: the body (two wheel frames), a beacon overlay with 4 frames
# (lamps lit in the day picture, a glow in its .emit layer), and the
# body's own night layer (head and tail lamps).  Beacon lamps are drawn
# unlit on the body; the overlay paints the lit ones over them.
# Beacon frames: 0 group A lit, 1 dark, 2 group B lit, 3 dark.

BEACON_FRAMES = [("A",), (), ("B",), ()]
BLUE_OFF, BLUE_ON, BLUE_E = "#6f86a8", "#4f9bff", "#7ab8ff"
RED_OFF, RED_ON, RED_E = "#9a5450", "#ff3a2e", "#ff6a5a"
FIRE, FIRE_D, FIRE_L = "#c42a22", "#8f1d18", "#d9493f"
CHROME, CHROME_D = "#c9cdd1", "#9aa0a6"


def star(cx, cy, r, w, fill):
    """A plain six-armed star: three crossed bars."""
    out = []
    for a in (0, 60, 120):
        t = math.radians(a)
        dx, dy = math.sin(t) * r, -math.cos(t) * r
        nx, ny = math.cos(t) * w / 2, math.sin(t) * w / 2
        out.append(P([(cx - dx + nx, cy - dy + ny), (cx + dx + nx, cy + dy + ny),
                      (cx + dx - nx, cy + dy - ny), (cx - dx - nx, cy - dy - ny)], fill))
    return out


def lamps_common(b, e, front, rear, w, wheels, wy, wr, dark):
    fx, fy, fw, fh = front
    rx, ry, rw, rh = rear
    b.append(R(fx, fy, fw, fh, HEAD, rx=0.2))
    b.append(R(rx, ry, rw, rh, TAIL, rx=0.2))
    e += [R(fx, fy, fw, fh, HEAD_E, rx=0.2),
          P([(fx + fw, fy - 0.2), (w, fy - 0.6), (w, fy + fh + 0.8), (fx + fw, fy + fh + 0.2)],
            HEAD_E, ' fill-opacity="0.35"'),
          R(rx, ry, rw, rh, TAIL_E, rx=0.2)]


def ambulance(frame):
    w, h, wr = 28, 12, 1.7
    wy = h - wr
    body, dark = "#eef0ee", "#b9bdbb"
    b = [P([(1, 10.4), (1, 1.2), (1.5, 0.8), (18.6, 0.8), (18.6, 3.0), (22.6, 3.0), (25.6, 6.0),
            (27.0, 6.4), (27.0, 10.4)], body)]
    b.append(R(1, 9.4, 26, 1.0, dark))
    b.append(R(1, 6.6, 26, 0.9, "#c8302b"))                         # side stripe
    b.append(R(1, 7.5, 26, 0.35, "#f2c84a"))
    b += star(9.5, 4.0, 1.7, 0.75, "#2f62b5")
    b.append(P([(18.9, 6.0), (18.9, 3.5), (22.3, 3.5), (24.8, 6.0)], GLASS))  # cab window
    b.append(P([(19.4, 5.4), (19.75, 5.4), (20.6, 3.9), (20.25, 3.9)], GLASS2))
    b.append(R(18.6, 3.0, 0.3, 6.4, dark))                          # cab / box join
    b.append(R(14.6, 1.6, 2.6, 2.4, GLASS, rx=0.2))                 # box side window
    b.append(R(23.0, 6.6, 0.8, 0.25, dark))
    for cx in (6.0, 22.6):
        b.append(C(cx, wy, wr + 0.45, dark))
    e = []
    lamps_common(b, e, (26.1, 6.9, 0.9, 0.8), (1, 2.0, 0.6, 1.6), w, None, wy, wr, dark)
    b.append(R(25.4, 9.4, 1.8, 0.8, "#3a3c40", rx=0.2))
    b.append(R(0.8, 9.4, 1.6, 0.8, "#3a3c40", rx=0.2))
    for cx in (6.0, 22.6):
        b += wheel(cx, frame, wy, wr)
    beacons = [("A", (1.4, 0.0, 1.6, 0.8), BLUE_OFF, BLUE_ON, BLUE_E),
               ("B", (16.6, 0.0, 1.6, 0.8), BLUE_OFF, BLUE_ON, BLUE_E),
               ("B", (19.4, 2.3, 1.4, 0.7), BLUE_OFF, BLUE_ON, BLUE_E),
               ("A", (21.0, 2.3, 1.4, 0.7), RED_OFF, RED_ON, RED_E)]
    return w, h, b, e, beacons


def police(frame):
    w, h = W, H
    k = TYPES["saloon"]
    body, dark = "#f2f2f0", "#b9bdbb"
    b = [P(k["body"], body), R(1, 7.6, 22.2, 1.0, "#1f3f7a")]
    b.append(R(1, 5.6, 22.2, 1.1, "#1f3f7a"))                       # door band
    for i in range(11):                                             # a block band under it
        b.append(R(1.6 + i * 2.0, 6.7, 1.0, 0.5, "#2f62b5"))
    for g in k["glass"]:
        b.append(P(g, GLASS))
        gx, gy = min(p[0] for p in g) + 0.6, min(p[1] for p in g)
        b.append(P([(gx, gy + 1.4), (gx + 0.35, gy + 1.4), (gx + 1.0, gy + 0.3), (gx + 0.65, gy + 0.3)], GLASS2))
    b += [R(12.2, 5.0, 0.2, 2.6, dark), R(17.6, 5.0, 0.2, 2.6, dark)]
    b.append(R(10.2, 2.2, 4.4, 0.35, "#3a3c40"))                    # light bar foot
    for cx in k["wheels"]:
        b.append(C(cx, WY, WR + 0.45, dark))
    e = []
    lamps_common(b, e, k["front"], k["rear"], w, None, WY, WR, dark)
    b.append(R(22.0, 7.4, 1.4, 0.7, "#3a3c40", rx=0.2))
    b.append(R(0.8, 7.4, 1.4, 0.7, "#3a3c40", rx=0.2))
    for cx in k["wheels"]:
        b += wheel(cx, frame)
    beacons = [("A", (10.2, 1.3, 2.2, 0.9), BLUE_OFF, BLUE_ON, BLUE_E),
               ("B", (12.4, 1.3, 2.2, 0.9), RED_OFF, RED_ON, RED_E)]
    return w, h, b, e, beacons


def lockers(b, x0, x1, y0, y1):
    n = max(1, round((x1 - x0) / 5.5))
    lw = (x1 - x0) / n
    for i in range(n):
        x = x0 + i * lw + 0.25
        b.append(R(x, y0, lw - 0.5, y1 - y0, FIRE_L, rx=0.2))
        y = y0 + 0.7
        while y < y1 - 0.3:
            b.append(R(x, y, lw - 0.5, 0.18, FIRE))
            y += 0.7


def fire_cab(b, x0, w, top):
    """A crew cab at the right end, from x0 to w - 0.5."""
    xr = w - 0.5
    b.append(P([(x0, 11.2), (x0, top), (xr - 3.2, top), (xr - 1.0, top + 3.0), (xr, top + 3.4),
                (xr, 11.2)], FIRE))
    b.append(P([(xr - 3.0, top + 3.1), (xr - 3.0, top + 0.6), (xr - 2.9, top + 0.6), (xr - 1.2, top + 3.1)], GLASS))
    b.append(R(x0 + 0.6, top + 0.6, xr - 3.6 - x0 - 0.6, 2.5, GLASS, rx=0.2))
    b.append(R(x0 + 0.6 + (xr - 3.6 - x0 - 0.6) / 2 - 0.15, top + 0.6, 0.3, 2.5, FIRE))
    b.append(P([(x0 + 1.1, top + 2.8), (x0 + 1.45, top + 2.8), (x0 + 2.2, top + 0.9), (x0 + 1.85, top + 0.9)], GLASS2))
    b.append(R(x0, top + 4.1, 0.3, 11.2 - top - 4.1, FIRE_D))
    return xr


def fire_common(b, w, wheels, wr, wy):
    b.append(R(0.8, 9.6, w - 1.3, 0.6, "#f2f2ee"))                  # white band
    b.append(R(0.8, 10.2, w - 1.3, 1.0, FIRE_D))
    for cx in wheels:
        b.append(C(cx, wy, wr + 0.45, FIRE_D))


def pumper(frame):
    w, h, wr = 36, 13, 1.8
    wy = h - wr
    b = [R(1, 2.8, 27, 8.4, FIRE)]
    b.append(R(1, 1.9, 27, 0.9, CHROME_D))                           # hose bed
    for x in range(2, 27, 2):
        b.append(R(x, 1.5, 1.0, 0.4, "#e2d9a0"))                     # folded hose
    b.append(R(3, 0.9, 21, 0.3, CHROME)); b.append(R(3, 1.4, 21, 0.3, CHROME))  # roof ladder
    for x in range(3, 25, 2):
        b.append(R(x, 0.9, 0.25, 0.8, CHROME))
    lockers(b, 1.4, 27.6, 3.4, 9.2)
    fire_common(b, w, (7.0, 30.6), wr, wy)
    xr = fire_cab(b, 28, w, 1.6)
    b.append(C(19.5, 6.3, 1.4, CHROME_D)); b.append(C(19.5, 6.3, 0.9, "#e2d9a0"))  # hose reel
    e = []
    lamps_common(b, e, (xr - 0.9, 6.4, 0.9, 0.8), (1, 3.4, 0.5, 1.4), w, None, wy, wr, FIRE_D)
    b.append(R(xr - 1.6, 10.0, 2.1, 0.9, CHROME, rx=0.2))
    for cx in (7.0, 30.6):
        b += wheel(cx, frame, wy, wr)
    beacons = [("A", (28.6, 0.8, 1.9, 0.8), RED_OFF, RED_ON, RED_E),
               ("B", (30.5, 0.8, 1.9, 0.8), RED_OFF, RED_ON, RED_E),
               ("B", (1.0, 1.0, 1.0, 0.9), RED_OFF, RED_ON, RED_E)]
    return w, h, b, e, beacons


TURNTABLE = (8.0, 3.0)   # the ladder's pivot on the ladder truck, facing right


def ladder_rails(b, x0, y0, x1, y1, width, rail, rung, step=1.2, base_dark=None):
    dx, dy = x1 - x0, y1 - y0
    n = math.hypot(dx, dy)
    ux, uy = dx / n, dy / n
    nx, ny = -uy * width / 2, ux * width / 2
    rw = 0.35
    for s in (1, -1):
        ox, oy = nx * s, ny * s
        ix, iy = -nx * s * rw / (width / 2), -ny * s * rw / (width / 2)
        b.append(P([(x0 + ox, y0 + oy), (x1 + ox, y1 + oy), (x1 + ox + ix, y1 + oy + iy),
                    (x0 + ox + ix, y0 + oy + iy)], rail))
    t = step
    while t < n - 0.2:
        cx, cy = x0 + ux * t, y0 + uy * t
        b.append(P([(cx + nx, cy + ny), (cx - nx, cy - ny), (cx - nx + ux * 0.22, cy - ny + uy * 0.22),
                    (cx + nx + ux * 0.22, cy + ny + uy * 0.22)], rung))
        t += step


def ladder_truck(frame, deployed=False):
    w, h, wr = 40, 13, 1.8
    wy = h - wr
    b = [R(1, 4.2, 31, 7.0, FIRE)]
    lockers(b, 1.4, 31.6, 4.8, 9.2)
    fire_common(b, w, (6.6, 10.6, 34.6), wr, wy)
    xr = fire_cab(b, 32, w, 2.6)
    # turntable
    tx, ty = TURNTABLE
    b.append(R(tx - 3.2, ty + 0.4, 6.4, 0.9, CHROME_D))
    b.append(R(tx - 1.6, ty - 0.4, 3.2, 1.0, FIRE_D, rx=0.3))
    b.append(C(tx, ty, 0.6, CHROME))
    if deployed:
        for ox in (3.0, 29.0):                                       # outriggers down
            b.append(R(ox - 0.5, 9.0, 1.0, 3.6, CHROME_D))
            b.append(R(ox - 1.2, 12.4, 2.4, 0.6, "#3a3c40"))
    else:
        ladder_rails(b, tx - 1.5, ty - 0.6, w - 1.8, 1.4, 1.3, CHROME, CHROME_D)
        b.append(R(w - 4.2, 1.9, 0.8, 0.7, CHROME_D))                # ladder rest over the cab
    e = []
    lamps_common(b, e, (xr - 0.9, 7.4, 0.9, 0.8), (1, 4.8, 0.5, 1.4), w, None, wy, wr, FIRE_D)
    b.append(R(xr - 1.6, 10.0, 2.1, 0.9, CHROME, rx=0.2))
    for cx in (6.6, 10.6, 34.6):
        b += wheel(cx, frame, wy, wr)
    beacons = [("A", (36.2, 1.8, 1.2, 0.8), RED_OFF, RED_ON, RED_E),
               ("B", (34.6, 1.8, 1.2, 0.8), RED_OFF, RED_ON, RED_E),
               ("A", (1.0, 3.4, 1.0, 0.8), RED_OFF, RED_ON, RED_E)]
    return w, h, b, e, beacons


LADDER_W, LADDER_H = 24, 44
LADDER_PIVOT = (2.0, 42.0)


def ladder_extended():
    """The raised ladder: pivot at LADDER_PIVOT, tip resting top right."""
    b = []
    x0, y0 = LADDER_PIVOT
    x1, y1 = 22.4, 1.2
    # three telescoped sections, narrowing towards the tip
    secs = [(0.0, 0.45, 1.9, CHROME_D), (0.4, 0.75, 1.6, CHROME), (0.7, 1.0, 1.35, "#dfe2e5")]
    for a, z, wid, col in secs:
        sx, sy = x0 + (x1 - x0) * a, y0 + (y1 - y0) * a
        ex, ey = x0 + (x1 - x0) * z, y0 + (y1 - y0) * z
        ladder_rails(b, sx, sy, ex, ey, wid, col, CHROME_D if col != CHROME_D else "#7d838a")
    b.append(R(x1 - 0.2, y1 - 1.2, 1.6, 0.6, "#3a3c40"))             # tip rest against the wall
    b.append(C(x0, y0, 1.2, FIRE_D)); b.append(C(x0, y0, 0.6, CHROME))
    e = [C(x1 - 0.4, y1 - 0.2, 0.5, "#fff4cf"),
         P([(x1 - 0.4, y1 - 0.4), (x1 - 4.0, y1 + 3.5), (x1 - 1.0, y1 + 4.5)], "#fff4cf", ' fill-opacity="0.3"')]
    b.append(C(x1 - 0.4, y1 - 0.2, 0.5, "#f3ecc8"))
    return b, e


EMERGENCY = {"ambulance": ambulance, "police": police, "fire-pumper": pumper,
             "fire-ladder": ladder_truck,
             "fire-ladder-deployed": lambda fr: ladder_truck(fr, deployed=True)}


def write(name, body, comment, w, h):
    with open(os.path.join(HERE, name), "w") as fh:
        fh.write(svg(body, comment, w, h))


def emergency():
    for t, fn in EMERGENCY.items():
        for fr in (0, 1):
            w, h, b, e, beacons = fn(fr)
            unlit = [R(x, y, bw, bh, off, rx=0.25) for g, (x, y, bw, bh), off, on, em in beacons]
            b = b + unlit
            for side in ("right", "left"):
                fl = (lambda x: mirror(x, w)) if side == "left" else (lambda x: x)
                write("%s-%s.%d.svg" % (t, side, fr), fl(b), "%s facing %s, wheel frame %d of 2" % (t, side, fr), w, h)
                write("%s-%s.%d.emit.svg" % (t, side, fr), fl(e), "%s head and tail lamps by night" % t, w, h)
        for bf, lit in enumerate(BEACON_FRAMES):
            day, glow = [], []
            for g, (x, y, bw, bh), off, on, em in beacons:
                if g in lit:
                    day.append(R(x, y, bw, bh, on, rx=0.25))
                    glow.append(R(x - 0.3, y - 0.3, bw + 0.6, bh + 0.6, em, rx=0.5))
                    glow.append(R(x - 1.2, y - 1.0, bw + 2.4, bh + 2.0, em, rx=1.0)
                                .replace("/>", ' fill-opacity="0.35"/>'))
            if not day:
                day = glow = ["<g/>"]
            for side in ("right", "left"):
                fl = (lambda x: mirror(x, w)) if side == "left" else (lambda x: x)
                write("%s-%s-beacon.%d.svg" % (t, side, bf), fl(day),
                      "%s beacons facing %s, frame %d of 4; drawn over the body" % (t, side, bf), w, h)
                write("%s-%s-beacon.%d.emit.svg" % (t, side, bf), fl(glow),
                      "%s beacon glow, frame %d of 4" % (t, bf), w, h)
    # BOX: the ambulance parked at the lobby with its back open: the rear
    # door swung out behind it, the inside lit, the stretcher's bed ready
    w, h, b, e, beacons = ambulance(0)
    b = b + [R(x, y, bw, bh, off, rx=0.25) for g, (x, y, bw, bh), off, on, em in beacons]
    b += [R(1.0, 1.3, 3.6, 7.9, "#5d646b"), R(1.3, 1.6, 3.3, 7.3, "#dfe6e8"),       # the opening, lit inside
          R(1.3, 6.6, 3.3, 0.6, "#9aa3ab"), R(1.6, 7.2, 0.3, 1.7, "#9aa3ab"),      # the stretcher's bed
          R(0.0, 1.0, 1.0, 8.4, "#eef0ee"), R(0.0, 6.6, 1.0, 0.9, "#c8302b"),      # the door, swung out
          R(0.0, 7.5, 1.0, 0.35, "#f2c84a"), R(0.0, 1.0, 0.2, 8.4, "#b9bdbb")]
    e = e + [R(1.3, 1.6, 3.3, 7.3, "#fff6dd", rx=0.2).replace("/>", ' fill-opacity="0.6"/>')]
    for side in ("right", "left"):
        fl = (lambda x: mirror(x, w)) if side == "left" else (lambda x: x)
        write("ambulance-open-%s.svg" % side, fl(b), "ambulance parked, facing %s, its back open" % side, w, h)
        write("ambulance-open-%s.emit.svg" % side, fl(e), "ambulance's open back lit by night", w, h)
    b, e = ladder_extended()
    for side in ("right", "left"):
        fl = (lambda x: mirror(x, LADDER_W)) if side == "left" else (lambda x: x)
        write("fire-ladder-extended-%s.svg" % side, fl(b),
              "the ladder raised against a wall, facing %s; pivot at (%g, %g) for right" % (
                  side, LADDER_PIVOT[0], LADDER_PIVOT[1]), LADDER_W, LADDER_H)
        write("fire-ladder-extended-%s.emit.svg" % side, fl(e), "tip floodlight", LADDER_W, LADDER_H)


# ---- parked, nose out (BOX: a car in its bay, as if reversed in) ----------
# 10 x 10 units, the tyres on y = 10: a car seen from the front, its body
# the side view's colour, its roof the side view's height.

FRONT_W, FRONT_H = 10, 10


def front(t, k):
    col = k["colour"]
    dark, light = shade(col, 0.72), shade(col, 1.12)
    roof = min(p[1] for p in k["body"])            # the side view's roof line
    van = t == "van"
    b = [R(0.9, 8.4, 1.6, 1.6, TYRE, rx=0.3), R(7.5, 8.4, 1.6, 1.6, TYRE, rx=0.3)]
    # the greenhouse: roof to waist, narrower at the top
    top = roof + 0.2
    b.append(P([(2.0 if not van else 1.0, top), (8.0 if not van else 9.0, top), (8.9, 5.2), (1.1, 5.2)], col))
    b.append(P([(2.6 if not van else 1.6, top + 0.6), (7.4 if not van else 8.4, top + 0.6), (8.2, 5.0), (1.8, 5.0)], GLASS))
    b.append(P([(3.2, top + 0.9), (4.0, top + 0.9), (3.0, 4.7), (2.3, 4.7)], GLASS2))
    # the body below the waist, the grille, lamps, a blank plate, bumper
    b += [R(0.5, 5.0, 9.0, 3.8, col, rx=0.8), R(0.5, 5.0, 9.0, 0.5, light, rx=0.3),
          R(2.8, 6.0, 4.4, 1.1, "#2a2c30", rx=0.3), R(3.0, 6.25, 4.0, 0.2, "#4a4c52"),
          R(3.0, 6.65, 4.0, 0.2, "#4a4c52"),
          R(0.9, 5.9, 1.6, 0.9, HEAD, rx=0.3), R(7.5, 5.9, 1.6, 0.9, HEAD, rx=0.3),
          R(3.9, 7.4, 2.2, 0.7, "#f2f2ee", rx=0.15),
          R(0.4, 8.0, 9.2, 0.7, "#3a3c40", rx=0.3),
          R(0.0, 4.6, 0.8, 0.6, dark, rx=0.2), R(9.2, 4.6, 0.8, 0.6, dark, rx=0.2)]   # mirrors
    if t == "taxi":
        b.append(R(3.6, roof - 1.0, 2.8, 1.1, "#f6f0dc", rx=0.4))
    if t == "estate":
        b.append(R(2.2, roof - 0.3, 5.6, 0.35, dark))
    if van:
        b.append(R(0.5, 7.0, 9.0, 0.4, "#3f78b5"))
    e = [R(0.9, 5.9, 1.6, 0.9, HEAD_E, rx=0.3), R(7.5, 5.9, 1.6, 0.9, HEAD_E, rx=0.3)]
    if t == "taxi":
        e.append(R(3.6, roof - 1.0, 2.8, 1.1, "#ffe9a8", rx=0.4))
    return b, e


# ---- the metro's train (BOX: it runs; the station is drawn without one) ----
# A car is 56 x 21 units, its wheels on y = 21 (the rail): the platform
# hall's train of art/src/rooms/metro.1.svg, a car at a time.  The front car
# has its headlamp at the right, the rear car its tail lamp at the left (a
# train going left is drawn flipped); each with its doors shut and open.

METRO_W, METRO_H = 56, 21


def metro_car(front, doors_open):
    y = -26  # metro.1's coordinates, moved up to the car's top
    b = [R(0, 26 + y, 56, 18.4, "#d8dce0", rx=2.2), R(0, 26 + y, 56, 1.6, "#a8b0b8", rx=1.6),
         R(0, 37.6 + y, 56, 1.6, "#c04040"), R(0, 39.2 + y, 56, 0.6, "#2a5090")]
    e = []
    for wx in (3, 16.2, 29.4, 42.6):
        b += [R(wx, 29.2 + y, 9, 5.4, "#304050", rx=0.8), R(wx + 0.6, 29.8 + y, 7.8, 2, "#5a7088", rx=0.5)]
        e.append(R(wx, 29.2 + y, 9, 5.4, "#fff4d0", rx=0.8).replace("/>", ' fill-opacity="0.55"/>'))
    for dx in (11.6, 37.6):
        if doors_open:
            b += [R(dx, 28.6 + y, 4.4, 15.4, "#e8e0c8", rx=0.4),            # the lit inside
                  R(dx - 1.6, 28.6 + y, 1.8, 15.4, "#b8c0c8", rx=0.3),     # the leaves, slid apart
                  R(dx + 4.2, 28.6 + y, 1.8, 15.4, "#b8c0c8", rx=0.3)]
            e.append(R(dx, 28.6 + y, 4.4, 15.4, "#fff4d0", rx=0.4))
        else:
            b += [R(dx, 28.6 + y, 4.4, 15.4, "#b8c0c8", rx=0.4), R(dx + 2.1, 28.6 + y, 0.2, 15.4, "#8a9098"),
                  R(dx + 0.5, 29.4 + y, 1.4, 5.2, "#304050", rx=0.3), R(dx + 2.5, 29.4 + y, 1.4, 5.2, "#304050", rx=0.3)]
    b.append(R(2, 44.2 + y, 52, 1.2, "#303038"))
    for cx in (6, 10, 46, 50):
        b += [C(cx, 45.6 + y, 1.2, "#202024"), C(cx, 45.6 + y, 0.45, "#606068")]
    if front:
        b.append(R(54.8, 31 + y, 1.2, 2, "#f0e8a0", rx=0.4))
        e.append(R(54.6, 30.6 + y, 1.6, 2.8, "#fff4cf", rx=0.5))
    else:
        b.append(R(0, 31 + y, 1.2, 2, "#e04040", rx=0.4))
        e.append(R(0, 30.8 + y, 1.4, 2.4, "#ff3a2a", rx=0.5))
    return b, e


def main():
    for front_car in (True, False):
        for doors in (False, True):
            name = "metro-car-%s%s" % ("front" if front_car else "rear", "-open" if doors else "")
            b, e = metro_car(front_car, doors)
            with open(os.path.join(HERE, name + ".svg"), "w") as fh:
                fh.write(svg(b, "the metro's %s car, doors %s" % ("front" if front_car else "rear", "open" if doors else "shut"),
                             METRO_W, METRO_H))
            with open(os.path.join(HERE, name + ".emit.svg"), "w") as fh:
                fh.write(svg(e, "its lit windows and lamps by night", METRO_W, METRO_H))
    for t, k in TYPES.items():
        b, e = front(t, k)
        with open(os.path.join(HERE, "%s-front.svg" % t), "w") as fh:
            fh.write(svg(b, "%s parked nose out, seen from the front" % t, FRONT_W, FRONT_H))
        with open(os.path.join(HERE, "%s-front.emit.svg" % t), "w") as fh:
            fh.write(svg(e, "%s headlamps by night" % t, FRONT_W, FRONT_H))
    for t, k in TYPES.items():
        for fr in (0, 1):
            b, e = draw(t, k, fr)
            for side, bb, ee in (("right", b, e), ("left", mirror(b), mirror(e))):
                n = "%s-%s.%d" % (t, side, fr)
                with open(os.path.join(HERE, n + ".svg"), "w") as fh:
                    fh.write(svg(bb, "%s facing %s, wheel frame %d of 2" % (t, side, fr)))
                with open(os.path.join(HERE, n + ".emit.svg"), "w") as fh:
                    fh.write(svg(ee, "%s lamps by night" % t))
    emergency()


if __name__ == "__main__":
    main()
