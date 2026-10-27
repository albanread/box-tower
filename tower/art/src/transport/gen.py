#!/usr/bin/env python3
"""Draw the transport pictures: elevator shafts, caps, landings and cars,
stairs and the escalator.  Run it from anywhere; it rewrites the SVGs in
this directory.  NOTES.md says how the pieces fit together.

Units: a cell is 4 units, a floor 16.  At a floor, the floor surface is
y = 15 (the bottom unit is the slab); a car standing at a floor fills
y = 2 .. 15.
"""

import os

HERE = os.path.dirname(os.path.abspath(__file__))
FLOOR = 16
FRAMES = 4  # landing and car doors: 0 closed, 1, 2 opening, 3 open


def f(v):
    s = ("%.3f" % v).rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def rect(x, y, w, h, fill, rx=0, extra=""):
    r = ' rx="%s"' % f(rx) if rx else ""
    return '<rect x="%s" y="%s" width="%s" height="%s"%s fill="%s"%s/>' % (
        f(x), f(y), f(w), f(h), r, fill, extra)


def circle(cx, cy, r, fill, extra=""):
    return '<circle cx="%s" cy="%s" r="%s" fill="%s"%s/>' % (f(cx), f(cy), f(r), fill, extra)


def poly(pts, fill, extra=""):
    return '<polygon points="%s" fill="%s"%s/>' % (
        " ".join("%s,%s" % (f(x), f(y)) for x, y in pts), fill, extra)


def svg(w, h, body, comment):
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %s %s" width="%s" height="%s">\n'
            '  <!-- units: %sx%s -->\n  <!-- %s -->\n  %s\n</svg>\n'
            % (f(w), f(h), f(w), f(h), f(w), f(h), comment, "\n  ".join(body)))


def write(name, w, h, body, comment):
    with open(os.path.join(HERE, name), "w", encoding="utf-8") as fh:
        fh.write(svg(w, h, body, comment))


# --------------------------------------------------------------------------
# The three kinds.  Standard: warm concrete shaft, steel frame, clear glass
# landing doors, a yellow car.  Express: wider, a dark blue shaft with
# bracing and rail lights, brass frames, blue-tinted glass, a blue car.
# Service: raw block shaft with tie beams and hazard bands, a mesh gate,
# a grey steel car with padded walls.

KINDS = {
    "elevator": dict(
        w=16, car_w=12,
        wall="#8f8a7f", wall2="#857f74", rail="#5c574e", cable="#3d3a35",
        frame="#aeb3b8", frame2="#8c9298", lamp="#f4f1e6", lamp_emit="#fff6dc",
        car="#f6d463", car_dark="#b8962e", car_door="#f9e3a0", car_seam="#c9a843",
        inside="#efe3b8", inside2="#e2d29c", inside_floor="#8a7440",
        crowd=["#7c6c4a", "#6a7d86", "#8a5f55", "#5f6f58"], head="#c7a27c",
        roof_light="#fffbe8"),
    "elevator-express": dict(
        w=24, car_w=20,
        wall="#3f4b5d", wall2="#475569", rail="#2a3340", cable="#1f2630",
        frame="#c9a45a", frame2="#a8853f", lamp="#ffcf6b", lamp_emit="#ffc95a",
        car="#4f7fc4", car_dark="#2f5590", car_door="#9dbde6", car_seam="#6f97cf",
        inside="#dce6f2", inside2="#c8d6ea", inside_floor="#40506a",
        crowd=["#4a5f7c", "#6e5a78", "#3f6e6a", "#7a6b52"], head="#c7a27c",
        roof_light="#fff1c8"),
    "elevator-service": dict(
        w=16, car_w=12,
        wall="#7a7264", wall2="#6f6759", rail="#4a453c", cable="#2f2c28",
        frame="#6d7277", frame2="#585d62", lamp="#ff9a2e", lamp_emit="#ff8a1e",
        car="#9ba2a7", car_dark="#6a7176", car_door="#bcc2c6", car_seam="#8a9196",
        inside="#c8cbc4", inside2="#7d8b97", inside_floor="#4a4d50",
        crowd=["#d0743a", "#4f6f96", "#c9a43a", "#5f7a4f"], head="#c3a07e",
        roof_light="#f3f5ee"),
}
HAZARD_Y, HAZARD_K = "#e8a33a", "#2e2d2b"


def hazard_band(x, y, w, h, step=1.6):
    """A yellow band with dark diagonal stripes, clipped to its box."""
    out = [rect(x, y, w, h, HAZARD_Y)]
    k = x - h
    while k < x + w:
        a, b = max(k, x - h), k + step / 2
        out.append(poly([(a, y + h), (a + h, y), (b + h, y), (b, y + h)], HAZARD_K))
        k += step
    return out


# ---- shafts ---------------------------------------------------------------

def shaft(name, k):
    w = k["w"]
    b = [rect(0, 0, w, FLOOR, k["wall"])]
    if name == "elevator":
        # poured concrete: a faint lift joint at mid-floor, form-tie dots
        b.append(rect(0, 7.8, w, 0.4, k["wall2"]))
        for x in (4, 8, 12):
            for y in (3.5, 11.5):
                b.append(circle(x, y, 0.25, k["wall2"]))
    elif name == "elevator-express":
        # bracing behind the rails: an X each floor
        for (x0, y0, x1, y1) in ((2, 0, w - 2, FLOOR), (w - 2, 0, 2, FLOOR)):
            dx, dy = x1 - x0, y1 - y0
            n = (dx * dx + dy * dy) ** 0.5
            ox, oy = -dy / n * 0.35, dx / n * 0.35
            b.append(poly([(x0 + ox, y0 + oy), (x1 + ox, y1 + oy),
                           (x1 - ox, y1 - oy), (x0 - ox, y0 - oy)], k["wall2"]))
        b.append(rect(0, 0, w, 0.8, k["wall2"]))
    else:
        # concrete block coursing and a tie beam at slab level
        for i, y in enumerate((2, 6, 10)):
            b.append(rect(0, y, w, 0.25, k["wall2"]))
            off = 0 if i % 2 else 4
            for x in range(off, w + 1, 8):
                b.append(rect(x, y, 0.25, 4, k["wall2"]))
        b += hazard_band(0, 14.2, w, 0.8)
    # guide rails, with a bracket each floor
    for rx in (1.2, w - 1.8):
        b.append(rect(rx, 0, 0.6, FLOOR, k["rail"]))
        b.append(rect(rx - 0.4, 5, 1.4, 0.6, k["rail"]))
    if name == "elevator-express":
        for rx in (2.2, w - 2.8):
            b.append(rect(rx, 0, 0.6, FLOOR, k["rail"]))
    # hoist ropes down the middle
    cx = w / 2
    for dx in (-0.6, 0, 0.6):
        b.append(rect(cx + dx - 0.08, 0, 0.16, FLOOR, k["cable"]))
    write(name + ".svg", w, FLOOR, b, "%s shaft, one floor; repeat for every floor of the run" % name)
    if name == "elevator-express":
        e = [circle(rx + 0.3, 5.3, 0.45, "#7fb8ff") for rx in (1.2, w - 1.8)]
        write(name + ".emit.svg", w, FLOOR, e, "rail marker lights")


def caps(name, k):
    w = k["w"]
    # machine room: a small house on top of the run, 8 units high
    t = [rect(0.5, 1.5, w - 1, 6.5, k["frame2"]),
         rect(0, 0.6, w, 1.2, k["rail"]),
         rect(1.5, 3, w - 3, 3.6, k["wall"])]
    for y in (3.6, 4.6, 5.6):  # louvre
        t.append(rect(2, y, w - 4, 0.45, k["wall2"]))
    if name == "elevator-express":
        t.append(rect(w / 2 - 1, 0, 2, 0.6, k["frame"]))
    if name == "elevator-service":
        t += hazard_band(0, 7.2, w, 0.8)
    write(name + "-top.svg", w, 8, t, "machine room; sits directly above the top floor of the run")
    if name == "elevator-express":
        write(name + "-top.emit.svg", w, 8, [circle(w / 2, 0.3, 0.45, "#ff5a4a")], "beacon")
    # pit: 4 units under the lowest floor, with two spring buffers
    p = [rect(0, 0, w, 4, k["wall2"]), rect(0, 3.2, w, 0.8, k["rail"])]
    for bx in (w * 0.3, w * 0.7):
        p.append(rect(bx - 1, 2.6, 2, 0.6, k["rail"]))
        for i in range(3):
            p.append(rect(bx - 0.7, 0.6 + i * 0.7, 1.4, 0.45, HAZARD_Y))
        p.append(rect(bx - 0.9, 0.2, 1.8, 0.4, HAZARD_K))
    write(name + "-pit.svg", w, 4, p, "pit; sits directly below the lowest floor of the run")


# ---- landings -------------------------------------------------------------
# Drawn IN FRONT of the car.  Jambs and header frame the opening; the
# doors are see-through (glass, or a mesh gate on the service lift), so a
# car passing a floor is seen through them.

JAMB = 1.2
HEAD_Y, HEAD_H = 0.3, 1.5
OPEN_TOP, OPEN_BOT = HEAD_Y + HEAD_H, 15.0


def landing_frame(k, w, skip=False):
    fr, fr2 = (k["frame2"], k["rail"]) if skip else (k["frame"], k["frame2"])
    b = [rect(0, HEAD_Y, w, HEAD_H, fr),
         rect(0, HEAD_Y + HEAD_H - 0.3, w, 0.3, fr2),
         rect(0, HEAD_Y, JAMB, OPEN_BOT - HEAD_Y, fr),
         rect(w - JAMB, HEAD_Y, JAMB, OPEN_BOT - HEAD_Y, fr),
         rect(JAMB - 0.3, OPEN_TOP, 0.3, OPEN_BOT - OPEN_TOP, fr2),
         rect(w - JAMB, OPEN_TOP, 0.3, OPEN_BOT - OPEN_TOP, fr2),
         rect(0, OPEN_BOT - 0.35, w, 0.35, fr2)]  # threshold
    if not skip:
        # call button on the right jamb
        b.append(rect(w - JAMB + 0.3, 8, 0.6, 1.4, fr2, rx=0.2))
        b.append(circle(w - JAMB + 0.6, 8.7, 0.22, k["lamp"]))
    return b


def landing_doors(name, k, w, t):
    """Door panels, t = 0 closed .. 1 open.  Panels slide behind the jambs."""
    inner = w - 2 * JAMB
    half = inner / 2
    vis = half * (1 - t)  # visible width of each panel
    if vis <= 0.01:
        return []
    h = OPEN_BOT - OPEN_TOP
    b = []
    for side in (0, 1):
        x0 = JAMB if side == 0 else w - JAMB - vis
        edge = x0 + vis - 0.35 if side == 0 else x0
        if name == "elevator-service":
            # a folding mesh gate: bars and two rails
            b.append(rect(x0, OPEN_TOP + 0.4, vis, 0.4, k["frame"]))
            b.append(rect(x0, OPEN_BOT - 1.2, vis, 0.4, k["frame"]))
            n = max(1, int(vis / 1.1))
            for i in range(n + 1):
                bx = x0 + (vis - 0.25) * i / n if n else x0
                b.append(rect(bx, OPEN_TOP, 0.25, h, k["frame"]))
            b.append(rect(edge, OPEN_TOP, 0.35, h, k["frame2"]))
        else:
            tint = "#cfe3f0" if name == "elevator" else "#7fa6d9"
            op = "0.32" if name == "elevator" else "0.42"
            b.append(rect(x0, OPEN_TOP, vis, h, tint, extra=' fill-opacity="%s"' % op))
            # a glint, and the metal leading edge
            if vis > 1.5:
                gx = x0 + vis * 0.25
                b.append(poly([(gx, OPEN_TOP + 3), (gx + 0.5, OPEN_TOP + 3),
                               (gx + 1.4, OPEN_TOP + 1.2), (gx + 0.9, OPEN_TOP + 1.2)],
                              "#ffffff", ' fill-opacity="0.45"'))
            b.append(rect(edge, OPEN_TOP, 0.35, h, k["frame2"]))
            b.append(rect(x0, OPEN_BOT - 1.0, vis, 0.6, k["frame2"]))  # kick plate
    return b


def landings(name, k):
    w = k["w"]
    for i in range(FRAMES):
        t = i / (FRAMES - 1)
        body = landing_doors(name, k, w, t) + landing_frame(k, w)
        body.append(rect(w / 2 - 1, HEAD_Y + 0.35, 2, 0.7, k["lamp"], rx=0.3))
        state = ["closed", "opening", "opening", "open"][i]
        write("%s-landing.%d.svg" % (name, i), w, FLOOR, body,
              "landing door, frame %d (%s); drawn in front of the car" % (i, state))
        write("%s-landing.%d.emit.svg" % (name, i), w, FLOOR,
              [rect(w / 2 - 1, HEAD_Y + 0.35, 2, 0.7, k["lamp_emit"], rx=0.3),
               circle(w - JAMB + 0.6, 8.7, 0.22, k["lamp_emit"])],
              "hall lantern and call button")
    # skipped floor: no doors, a guard bar across, a no-stop disc on the header
    body = landing_frame(k, w, skip=True)
    body.append(rect(JAMB, 8.6, w - 2 * JAMB, 0.7, k["frame2"]))
    body.append(rect(JAMB, 12.2, w - 2 * JAMB, 0.7, k["frame2"]))
    body.append(circle(w / 2, HEAD_Y + HEAD_H / 2, 0.75, "#c8302b"))
    body.append(rect(w / 2 - 0.5, HEAD_Y + HEAD_H / 2 - 0.16, 1, 0.32, "#ffffff"))
    write("%s-landing-skip.svg" % name, w, FLOOR, body,
          "landing on a floor this lift passes without stopping; in front of the car")
    write("%s-landing-skip.emit.svg" % name, w, FLOOR,
          [circle(w / 2, HEAD_Y + HEAD_H / 2, 0.75, "#ff4a3a")], "no-stop sign")


# ---- cars -----------------------------------------------------------------
# A car is three layers, back to front: load (interior + riders), doors,
# shell (roof, posts, platform).  13 units tall; at a floor its top is at
# y = 2 and its platform rests on the floor surface y = 15.

CAR_H = 13
POST = 1.0
ROOF = 1.4
PLAT = 1.0


def rider(x, foot, c, head, s=1.0):
    """A front-facing rider, 2.2 wide, about 8.6 tall, feet at `foot`."""
    b = [rect(x + 0.35 * s, foot - 3.6 * s, 0.6 * s, 3.6 * s, "#3a3a3e"),
         rect(x + 1.25 * s, foot - 3.6 * s, 0.6 * s, 3.6 * s, "#3a3a3e"),
         rect(x, foot - 7.2 * s, 2.2 * s, 3.9 * s, c, rx=0.6 * s),
         circle(x + 1.1 * s, foot - 7.95 * s, 0.95 * s, head)]
    return b


def car(name, k):
    cw = k["car_w"]
    inner = cw - 2 * POST
    # loads: 0 empty, 1 a few, 2 about half, 3 full
    counts = {12: [0, 1, 3, 5], 20: [0, 2, 5, 9]}[cw]
    for lvl, n in enumerate(counts):
        b = [rect(0, 0, cw, CAR_H, k["inside"])]
        if name == "elevator-service":
            # quilted protection pads on the walls
            for x in range(1, int(cw) - 1, 2):
                b.append(rect(x + 0.1, ROOF + 0.8, 1.8, 7.5, k["inside2"], rx=0.3))
        else:
            b.append(rect(0, ROOF + 1, cw, 6, k["inside2"]))
        b.append(rect(0, 7.4, cw, 0.35, k["car_dark"]))  # handrail
        b.append(rect(POST + 0.6, ROOF, inner - 1.2, 0.4, k["roof_light"]))
        b.append(rect(0, CAR_H - PLAT - 0.6, cw, 0.6, k["inside_floor"]))
        foot = CAR_H - PLAT - 0.15
        if n:
            # a back row (smaller, paler) when crowded, then the front row
            front = min(n, int(inner // 2.4))
            back = n - front
            if back:
                step = inner / (back + 1)
                for i in range(back):
                    x = POST + step * (i + 1) - 1.0
                    b += rider(x, foot - 0.6, k["crowd"][(i + 2) % 4], k["head"], 0.9)
            step = inner / front
            for i in range(front):
                x = POST + step * i + (step - 2.2) / 2
                b += rider(x, foot, k["crowd"][i % 4], k["head"])
        label = ["empty", "a few riders", "about half full", "full"][lvl]
        write("%s-car-load.%d.svg" % (name, lvl), cw, CAR_H, b,
              "car interior, %s; the back layer of the car" % label)
    # doors: centre-opening panels that shrink into the posts
    half = inner / 2
    for i in range(FRAMES):
        t = i / (FRAMES - 1)
        vis = half * (1 - t)
        b = []
        if vis > 0.01:
            h = CAR_H - ROOF - PLAT
            b.append(rect(POST, ROOF, vis, h, k["car_door"]))
            b.append(rect(cw - POST - vis, ROOF, vis, h, k["car_door"]))
            b.append(rect(POST + vis - 0.3, ROOF, 0.3, h, k["car_seam"]))
            b.append(rect(cw - POST - vis, ROOF, 0.3, h, k["car_seam"]))
            if name == "elevator-service":
                for y in (4.5, 7.5, 10):
                    b.append(rect(POST, y, vis, 0.35, k["car_seam"]))
                    b.append(rect(cw - POST - vis, y, vis, 0.35, k["car_seam"]))
            elif vis > 1.4:
                # a vision panel in each leaf
                b.append(rect(POST + vis - 1.4, 3.6, 0.7, 3.2, k["inside2"], rx=0.2))
                b.append(rect(cw - POST - vis + 0.7, 3.6, 0.7, 3.2, k["inside2"], rx=0.2))
        if not b:
            b.append("<g/>")
        state = ["closed", "opening", "opening", "open"][i]
        write("%s-car-doors.%d.svg" % (name, i), cw, CAR_H, b,
              "car doors, frame %d (%s); the middle layer of the car" % (i, state))
    # shell
    b = [rect(0, 0, cw, ROOF, k["car_dark"]),
         rect(0, 0, cw, ROOF - 0.4, k["car"]),
         rect(0, 0, POST, CAR_H, k["car"]),
         rect(cw - POST, 0, POST, CAR_H, k["car"]),
         rect(POST - 0.25, ROOF, 0.25, CAR_H - ROOF - PLAT, k["car_dark"]),
         rect(cw - POST, ROOF, 0.25, CAR_H - ROOF - PLAT, k["car_dark"]),
         rect(0, CAR_H - PLAT, cw, PLAT, k["car_dark"]),
         rect(cw / 2 - 0.9, 0.2, 1.8, 0.5, k["roof_light"], rx=0.25)]  # position lamp
    if name == "elevator-service":
        b += hazard_band(0, CAR_H - PLAT, cw, PLAT)
    if name == "elevator-express":
        b.append(rect(0, ROOF - 0.4, cw, 0.25, k["frame_band"] if "frame_band" in k else "#d9b25e"))
    write("%s-car.svg" % name, cw, CAR_H, b, "car shell; the front layer of the car")
    write("%s-car.emit.svg" % name, cw, CAR_H,
          [rect(POST, ROOF, inner, CAR_H - ROOF - PLAT, k["roof_light"], extra=' fill-opacity="0.22"'),
           rect(POST + 0.6, ROOF, inner - 1.2, 0.4, k["roof_light"]),
           rect(cw / 2 - 0.9, 0.2, 1.8, 0.5, k["roof_light"], rx=0.25)],
          "cab light; seen through the doorway when the doors are open")


# ---- stairs and escalator -------------------------------------------------
# 8 cells by one floor.  The flight rises left to right from this floor's
# surface (y 15) to the next floor's (y -1, just above the top edge).

SW = 32


def stairs():
    stone, stone2, nose = "#cdc5b3", "#aca390", "#e6dfcf"
    rail, post = "#5d554a", "#7d7466"
    pts = [(0, 15), (4, 15)]
    for i in range(8):
        x, y = 4 + 3 * i, 13 - 2 * i
        pts += [(x, y), (x + 3, y)]
    pts += [(SW, -1), (SW, 2.5), (8.5, 16), (0, 16)]
    b = [poly(pts, stone)]
    # the stringer: a darker band under the flight
    b.append(poly([(4, 16), (SW, -0.6 + 3.6), (SW, 2.5), (8.5, 16)], stone2))
    for i in range(8):
        x, y = 4 + 3 * i, 13 - 2 * i
        b.append(rect(x, max(y, 0), 3, 0.45, nose))
    b.append(rect(0, 15, 4, 0.45, nose))
    # handrail parallel to the nosings, 4.5 up, with posts
    def ry(x):
        return 13 - 2 * (x - 4) / 3 - 4.5
    x0, x1 = 1.0, SW
    b.append(poly([(x0, ry(4) ), (4, ry(4)), (x1, ry(x1)), (x1, ry(x1) + 0.55),
                   (4, ry(4) + 0.55), (x0, ry(4) + 0.55)], rail))
    b.append(rect(x0, ry(4), 0.6, 15 - ry(4), post))
    for i in (1, 3, 5, 7):
        x = 4 + 3 * i + 1.5
        top = ry(x)
        b.append(rect(x - 0.2, top, 0.4, (13 - 2 * i) - top, post))
    write("stairs.svg", SW, FLOOR, b, "stairs, one floor's rise, left to right; redrawn from tower-together's stairs.svg (MIT, Patrick Hulin) to our grid")


def escalator():
    x0, x1 = 5.0, 27.0
    slope = 16 / (x1 - x0)

    def line(x):
        if x <= x0:
            return 15.0
        if x >= x1:
            return -1.0
        return 15 - (x - x0) * slope

    truss, truss2 = "#b9c4b0", "#9aa690"
    step, comb, tread = "#3f4542", "#8f9a8e", "#646d68"
    rail = "#24282a"
    frames = 4
    pitch = 2.0
    for fr in range(frames):
        b = []
        # truss body under the step line
        b.append(poly([(0, 15), (x0, 15), (x1, -1), (SW, -1), (SW, 3.4), (x1 + 1, 3.4),
                       (x0 + 3.5, 16), (0, 16)], truss))
        b.append(poly([(x0 + 1.6, 16), (x1 + 0.4, 1.8), (x1 + 1, 3.4), (x0 + 3.5, 16)], truss2))
        # glass balustrade
        hb = 4.2
        b.append(poly([(1.2, 15 - hb), (x0, 15 - hb), (x1, -1 - hb), (SW, -1 - hb),
                       (SW, -1), (x1, -1), (x0, 15), (1.2, 15)],
                      "#dff0f2", ' fill-opacity="0.35"'))
        # the step band: a sawtooth of steps that moves up by pitch/frames each frame
        band = [(0, 15), (x0, 15)]
        ph = pitch * fr / frames
        x = x0 - pitch + ph
        teeth = []
        while x < x1 + pitch:
            a, c = max(x, x0), min(x + pitch, x1)
            if c > a:
                yt = line(c)
                teeth.append((a, c, yt))
            x += pitch
        for a, c, yt in teeth:
            band += [(a, yt), (c, yt)]
        band += [(x1, -1), (SW, -1), (SW, 0.2), (x1, 0.2), (x0, 16.2), (0, 16.2)]
        b.append(poly(band, step))
        for a, c, yt in teeth:
            if c - a > 0.3:
                b.append(rect(a, max(yt, 0), c - a, 0.3, tread))
        # comb plates at each landing
        b.append(rect(0, 15, x0, 0.4, comb))
        b.append(rect(x1, 0, SW - x1, 0.3, comb))
        # moving handrail along the top of the glass, with newels
        r = 0.6
        b.append(poly([(1.2, 15 - hb), (x0, 15 - hb), (x1, -1 - hb), (SW, -1 - hb),
                       (SW, -1 - hb + r), (x1, -1 - hb + r), (x0, 15 - hb + r), (1.2, 15 - hb + r)], rail))
        b.append(circle(1.2, 15 - hb + r / 2 + 0.5, 0.8, rail))
        b.append(rect(0.4, 15 - hb + 0.9, 0.8, hb - 0.9, "#c0c9bc"))
        write("escalator.%d.svg" % fr, SW, FLOOR, b,
              "escalator, frame %d of %d (steps moving up); redrawn from tower-together's escalator.svg (MIT, Patrick Hulin) to our grid" % (fr, frames))


# ---- car lift (a BOX addition) --------------------------------------------
# Carries one vehicle between the street floor and the basement parking
# floors.  8 cells (32 units) wide.  The platform is 28 wide and its deck
# is flush with the floor surface (deck top y = 15 at a floor).

CL_W = 32
CL = dict(wall="#6b6760", wall2="#5d5952", side="#4e4b46", beam="#3b3a38",
          frame="#7c8288", frame2="#5f656b", deck="#55595d", deck2="#8a9096",
          lamp="#ffb43c", lamp_emit="#ffa82a", light="#fff3d6")
CL_SIDE = 2.0


def car_lift():
    k = CL
    w = CL_W
    # shaft: thick side walls, I-beam rails, a heavy tie beam each floor
    b = [rect(0, 0, w, FLOOR, k["wall"]),
         rect(0, 0, CL_SIDE, FLOOR, k["side"]), rect(w - CL_SIDE, 0, CL_SIDE, FLOOR, k["side"])]
    for y in (4, 8, 12):
        b.append(rect(CL_SIDE, y, w - 2 * CL_SIDE, 0.25, k["wall2"]))
    b.append(rect(CL_SIDE, 0, w - 2 * CL_SIDE, 1.4, k["beam"]))
    for rx in (CL_SIDE, w - CL_SIDE - 1.6):
        b.append(rect(rx, 0, 1.6, FLOOR, k["beam"]))
        b.append(rect(rx + 0.5, 0, 0.6, FLOOR, k["wall2"]))
    for x in (8, 16, 24):  # four heavy ropes / chains, drawn as pairs
        b.append(rect(x - 0.35, 0, 0.25, FLOOR, "#2a2928"))
        b.append(rect(x + 0.1, 0, 0.25, FLOOR, "#2a2928"))
    b += hazard_band(CL_SIDE, 1.4, w - 2 * CL_SIDE, 0.6, step=2)
    write("car-lift.svg", w, FLOOR, b, "car lift shaft, one floor; repeat for every floor of the run, the street floor included")
    # machine room: 8 high
    t = [rect(0, 1.2, w, 6.8, k["side"]), rect(0, 0.4, w, 1.2, k["beam"]),
         rect(2, 2.6, w - 4, 4.6, k["wall"])]
    for cx in (9, 23):  # two winding drums
        t.append(circle(cx, 4.9, 2.0, k["beam"]))
        t.append(circle(cx, 4.9, 1.2, k["frame"]))
        t.append(circle(cx, 4.9, 0.4, k["beam"]))
    t.append(rect(13, 4.4, 6, 1.0, k["frame2"]))  # shaft between drums
    t += hazard_band(0, 7.2, w, 0.8, step=2)
    write("car-lift-top.svg", w, 8, t, "car lift machine room; directly above the top (street) floor of the run")
    p = [rect(0, 0, w, 4, k["side"]), rect(0, 3.2, w, 0.8, k["beam"])]
    for bx in (7, 16, 25):
        p.append(rect(bx - 1.4, 2.4, 2.8, 0.8, k["beam"]))
        for i in range(3):
            p.append(rect(bx - 1, 0.5 + i * 0.65, 2, 0.45, HAZARD_Y))
        p.append(rect(bx - 1.2, 0.1, 2.4, 0.4, HAZARD_K))
    write("car-lift-pit.svg", w, 4, p, "car lift pit; directly below the lowest floor of the run")

    # the platform: deck, two uprights, an overhead crosshead with a lamp
    pw = 28
    pl = [rect(0, 0.6, pw, 1.2, k["frame2"]),
          rect(0, 0.6, 1.2, 15.4, k["frame2"]), rect(pw - 1.2, 0.6, 1.2, 15.4, k["frame2"]),
          rect(0.3, 1.8, 0.6, 13.2, k["frame"]), rect(pw - 0.9, 1.8, 0.6, 13.2, k["frame"]),
          rect(0, 15, pw, 1, k["deck"]), rect(0, 15, pw, 0.3, k["deck2"])]
    pl += hazard_band(1.2, 1.8, pw - 2.4, 0.5, step=2)
    for x in (1.2, pw - 2.2):  # wheel stops at each end of the deck
        pl.append(rect(x, 14.3, 1, 0.7, HAZARD_Y))
    pl.append(rect(pw / 2 - 1.5, 0, 3, 0.6, k["frame"]))
    pl.append(rect(pw / 2 - 1.2, 1.8, 2.4, 0.6, k["light"], rx=0.3))
    pl.append(circle(1.6, 0.6, 0.6, k["lamp"]))
    pl.append(circle(pw - 1.6, 0.6, 0.6, k["lamp"]))
    write("car-lift-platform.svg", pw, FLOOR, pl,
          "car lift platform; at a floor its deck top is on the floor surface; the vehicle is drawn in front of it")
    write("car-lift-platform.emit.svg", pw, FLOOR,
          [rect(pw / 2 - 1.2, 1.8, 2.4, 0.6, k["light"], rx=0.3),
           poly([(pw / 2 - 1.2, 2.4), (pw / 2 + 1.2, 2.4), (pw / 2 + 7, 15), (pw / 2 - 7, 15)],
                k["light"], ' fill-opacity="0.18"'),
           circle(1.6, 0.6, 0.6, k["lamp_emit"]), circle(pw - 1.6, 0.6, 0.6, k["lamp_emit"])],
          "platform floodlight and amber beacons")

    # frame shared by the entrance and the landings (in front of the platform)
    jamb, top = 1.6, 2.6

    def frame(dark=False):
        fr, fr2 = (k["frame2"], k["beam"]) if dark else (k["frame"], k["frame2"])
        return [rect(0, 0, w, top, fr), rect(0, top - 0.4, w, 0.4, fr2),
                rect(0, 0, jamb, 15, fr), rect(w - jamb, 0, jamb, 15, fr),
                rect(jamb - 0.3, top, 0.3, 15 - top, fr2), rect(w - jamb, top, 0.3, 15 - top, fr2),
                rect(0, 14.6, w, 0.4, fr2)]
    oh = 15 - top
    ow = w - 2 * jamb

    # street entrance: a canopy and a roller shutter that rolls up
    for i in range(FRAMES):
        t = i / (FRAMES - 1)
        sh = oh * (1 - t)
        b = []
        if sh > 0.01:
            b.append(rect(jamb, top, ow, sh, "#a7adb2"))
            y = top + 0.9
            while y < top + sh - 0.2:
                b.append(rect(jamb, y, ow, 0.3, "#8c9297"))
                y += 1.2
            b.append(rect(jamb, top + sh - 0.6, ow, 0.6, k["frame2"]))
            b += hazard_band(jamb, top + sh - 1.1, ow, 0.5, step=2)
        b.append(rect(jamb, top, ow, 0.9, k["beam"]))  # the roll housing
        b += frame()
        b.append(rect(-0.5 + 0.5, 0, w, 1.0, k["beam"]))
        for x in (6, 16, 26):
            b.append(rect(x - 1.2, 1.0, 2.4, 0.6, k["light"], rx=0.3))
        b.append(circle(jamb + 0.8, 4, 0.6, k["lamp"]))
        b.append(circle(w - jamb - 0.8, 4, 0.6, k["lamp"]))
        state = ["closed", "opening", "opening", "open"][i]
        write("car-lift-entrance.%d.svg" % i, w, FLOOR, b,
              "car lift street entrance, shutter frame %d (%s); on the street floor, in front of the platform" % (i, state))
        e = [rect(x - 1.2, 1.0, 2.4, 0.6, k["light"], rx=0.3) for x in (6, 16, 26)]
        e += [circle(jamb + 0.8, 4, 0.6, k["lamp_emit"]), circle(w - jamb - 0.8, 4, 0.6, k["lamp_emit"])]
        write("car-lift-entrance.%d.emit.svg" % i, w, FLOOR, e, "canopy lights and beacons")

    # basement landing: a bi-parting freight door (upper leaf up, lower leaf down)
    for i in range(FRAMES):
        t = i / (FRAMES - 1)
        hh = oh / 2 * (1 - t)
        b = []
        if hh > 0.01:
            b.append(rect(jamb, top, ow, hh, "#8d9399"))
            b.append(rect(jamb, 15 - hh, ow, hh, "#8d9399"))
            for x in range(4, int(w) - 2, 4):
                b.append(rect(x, top, 0.35, hh, "#767c82"))
                b.append(rect(x, 15 - hh, 0.35, hh, "#767c82"))
            b += hazard_band(jamb, top + hh - 0.7, ow, 0.7, step=2)
            b += hazard_band(jamb, 15 - hh, ow, 0.7, step=2)
        b += frame()
        b.append(circle(w - jamb + 0.8, 8.5, 0.35, k["lamp"]))
        b.append(rect(w / 2 - 1.5, 0.8, 3, 0.9, k["lamp"], rx=0.4))
        state = ["closed", "opening", "opening", "open"][i]
        write("car-lift-landing.%d.svg" % i, w, FLOOR, b,
              "car lift landing on a served basement floor, frame %d (%s); in front of the platform" % (i, state))
        write("car-lift-landing.%d.emit.svg" % i, w, FLOOR,
              [rect(w / 2 - 1.5, 0.8, 3, 0.9, k["lamp_emit"], rx=0.4),
               circle(w - jamb + 0.8, 8.5, 0.35, k["lamp_emit"])], "landing lantern")
    b = frame(dark=True)
    for y in (7.5, 11.5):
        b += hazard_band(jamb, y, ow, 0.8, step=2)
    b.append(circle(w / 2, 1.2, 0.9, "#c8302b"))
    b.append(rect(w / 2 - 0.6, 1.0, 1.2, 0.4, "#ffffff"))
    write("car-lift-landing-skip.svg", w, FLOOR, b,
          "car lift landing on a floor it does not serve; in front of the platform")
    write("car-lift-landing-skip.emit.svg", w, FLOOR, [circle(w / 2, 1.2, 0.9, "#ff4a3a")], "no-stop sign")


def main():
    car_lift()
    for name, k in KINDS.items():
        shaft(name, k)
        caps(name, k)
        landings(name, k)
        car(name, k)
    stairs()
    escalator()


if __name__ == "__main__":
    main()
