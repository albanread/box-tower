#!/usr/bin/env python3
"""Draw the people.  Rewrites the SVGs in this directory; NOTES.md has the
recolour key and the sizes.

Every figure is 6 x 12 units, feet on y = 12, body centred on x = 3.  Each
picture `x.svg` has a recolour layer `x.tint.svg` (same viewBox):

    red   = upper clothes (shirt, jacket, sleeves)  -> stress / role colour
    green = skin (face, neck, hands)                -> skin tone
    blue  = lower clothes (trousers, skirt)         -> trouser colour

A channel's value is how much of the new colour replaces the picture
there; back limbs carry 0.8 so a little of the picture's shading stays.
"""

import math
import os

HERE = os.path.dirname(os.path.abspath(__file__))
W, H = 6, 12

# Neutral picture colours (what shows with no tinting).
TOP, TOP_BACK = "#6f7782", "#59606a"
SKIN, SKIN_BACK = "#d9a87a", "#c4936a"
LEG, LEG_BACK = "#3b4250", "#2e3440"
SHOE, HAIR = "#1f2226", "#3a2a20"

HIP = (3.0, 8.0)
SHOULDER = (3.05, 5.1)
LEG_LEN, LEG_W = 3.7, 0.78
ARM_LEN, ARM_W = 2.9, 0.6


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


def limb(origin, angle, length, width):
    """A capsule from origin, angle in degrees from straight down (+ = forward)."""
    a = math.radians(angle)
    dx, dy = math.sin(a), math.cos(a)
    ox, oy = origin
    ex, ey = ox + dx * length, oy + dy * length
    nx, ny = dy * width / 2, -dx * width / 2
    pts = [(ox + nx, oy + ny), (ex + nx, ey + ny), (ex - nx, ey - ny), (ox - nx, oy - ny)]
    return pts, (ex, ey)


def cap(pts, end, r):
    return pts, end, r


class Fig:
    """Collects the picture and its tint layer side by side."""

    def __init__(self):
        self.pic, self.tint = [], []

    def add(self, shape_fn, colour, tint=None):
        self.pic.append(shape_fn(colour))
        self.tint.append(shape_fn(tint if tint else "#000000"))

    def poly(self, pts, colour, tint=None):
        self.add(lambda c: P(pts, c), colour, tint)

    def rect(self, x, y, w, h, colour, tint=None, rx=0):
        self.add(lambda c: R(x, y, w, h, c, rx), colour, tint)

    def circ(self, cx, cy, r, colour, tint=None):
        self.add(lambda c: C(cx, cy, r, c), colour, tint)


def tint(r=0, g=0, b=0):
    return "#%02x%02x%02x" % (round(r * 255), round(g * 255), round(b * 255))


T_TOP, T_TOPB = tint(r=1), tint(r=0.8)
T_SKIN, T_SKINB = tint(g=1), tint(g=0.8)
T_LEG, T_LEGB = tint(b=1), tint(b=0.8)


def leg(fig, angle, back):
    pts, (ex, ey) = limb(HIP, angle, LEG_LEN, LEG_W)
    fig.poly(pts, LEG_BACK if back else LEG, T_LEGB if back else T_LEG)
    fig.circ(HIP[0], HIP[1], LEG_W / 2, LEG_BACK if back else LEG, T_LEGB if back else T_LEG)
    # shoe: a flat wedge pointing forward
    fig.rect(ex - 0.45, ey - 0.15, 1.15, 0.5, SHOE, rx=0.2)
    return ex, ey


def arm(fig, angle, back):
    pts, (ex, ey) = limb(SHOULDER, angle, ARM_LEN, ARM_W)
    fig.poly(pts, TOP_BACK if back else TOP, T_TOPB if back else T_TOP)
    fig.circ(SHOULDER[0], SHOULDER[1], ARM_W / 2, TOP_BACK if back else TOP, T_TOPB if back else T_TOP)
    fig.circ(ex, ey, 0.36, SKIN_BACK if back else SKIN, T_SKINB if back else T_SKIN)
    return ex, ey


def head(fig, role, female=False):
    if female:
        if role in ("security", "medical"):
            fig.add(lambda c: '<ellipse cx="1.75" cy="3.7" rx="0.55" ry="1.0" fill="%s"/>' % c, HAIR)  # ponytail
        elif role == "housekeeping":
            fig.circ(1.75, 2.5, 0.6, HAIR)                                                       # a bun
        else:
            fig.poly([(1.75, 2.9), (2.3, 1.85), (3.5, 1.75), (3.95, 2.5), (3.35, 5.6), (1.95, 5.7)], HAIR)  # long hair
    fig.rect(2.75, 4.0, 0.6, 0.7, SKIN, T_SKIN)  # neck
    fig.circ(2.85, 2.95, 1.15, HAIR)             # hair, at the back
    fig.circ(3.25, 3.25, 0.98, SKIN, T_SKIN)     # face, to the front
    fig.rect(4.05, 3.2, 0.3, 0.35, SKIN, T_SKIN, rx=0.1)  # nose
    if role == "security":
        fig.poly([(1.8, 2.5), (4.0, 2.2), (4.0, 1.6), (2.2, 1.5), (1.8, 2.0)], "#1f2a44")
        fig.poly([(3.6, 2.5), (4.9, 2.6), (4.9, 2.8), (3.6, 2.9)], "#141b2e")  # visor
        fig.circ(3.3, 1.95, 0.22, "#e3b64a")
    elif role == "housekeeping":
        fig.poly([(1.7, 2.9), (2.1, 1.9), (3.1, 1.6), (4.0, 2.1), (4.1, 2.5), (1.9, 3.3)], "#f1efe8")
    elif role == "medical":
        fig.poly([(1.8, 2.6), (2.0, 1.8), (4.1, 1.8), (4.2, 2.5)], "#f4f6f7")
        fig.rect(2.85, 1.85, 0.3, 0.7, "#c8302b")
        fig.rect(2.65, 2.05, 0.7, 0.3, "#c8302b")


SKIRTED = ("civilian", "worker", "guest", "housekeeping")


def torso(fig, role, female=False):
    if female:
        fig.rect(2.2, 4.6, 1.6, 3.6, TOP, T_TOP, rx=0.6)
    else:
        fig.rect(2.1, 4.5, 1.8, 3.9, TOP, T_TOP, rx=0.55)
    if role == "worker":
        fig.poly([(3.55, 4.6), (3.95, 4.6), (3.95, 5.6)], "#f2f2ee")      # collar
        fig.rect(3.6, 5.1, 0.3, 2.0, "#7a2f36")                            # tie edge
    elif role == "housekeeping":
        fig.rect(3.1, 6.2, 0.95, 3.0, "#f1efe8", rx=0.2)                   # apron
    elif role == "security":
        fig.circ(3.55, 5.6, 0.25, "#e3b64a")                               # badge
        fig.rect(2.1, 7.6, 1.8, 0.45, "#141b2e")                           # belt
    elif role == "medical":
        fig.rect(3.6, 4.6, 0.35, 3.8, "#d9dee2")                           # coat edge
        fig.poly([(2.9, 4.7), (3.3, 6.2), (3.6, 6.2), (3.2, 4.7)], "#3a3f45")  # stethoscope


def pose_side(role, legs, arms, guest_pull=False, female=False):
    fig = Fig()
    lf, lb = legs
    af, ab = arms
    if guest_pull:
        ab = -38
    # suitcase behind (guest)
    if role == "guest":
        fig.rect(0.15, 8.4, 1.7, 3.0, "#b5543a", rx=0.3)
        fig.rect(0.15, 9.6, 1.7, 0.3, "#8e3f2b")
        fig.circ(0.5, 11.6, 0.35, SHOE)
        fig.circ(1.5, 11.6, 0.35, SHOE)
    hb = arm(fig, ab, back=True)
    if role == "guest":
        # the pull handle from the case to the back hand
        x0, y0 = 1.4, 8.4
        x1, y1 = hb
        dx, dy = x1 - x0, y1 - y0
        n = math.hypot(dx, dy) or 1
        nx, ny = -dy / n * 0.12, dx / n * 0.12
        fig.poly([(x0 + nx, y0 + ny), (x1 + nx, y1 + ny), (x1 - nx, y1 - ny), (x0 - nx, y0 - ny)], "#2a2b2e")
    leg(fig, lb, back=True)
    leg(fig, lf, back=False)
    if female and role in SKIRTED:
        fig.poly([(2.15, 7.6), (3.95, 7.6), (4.55, 10.1), (1.55, 10.1)], LEG, T_LEG)   # the skirt
    torso(fig, role, female)
    head(fig, role, female)
    hf = arm(fig, af, back=False)
    if role == "worker":
        x, y = hf
        fig.rect(x - 0.15, y - 0.05, 0.3, 0.35, "#2b2018")
        fig.rect(x - 0.8, y + 0.25, 1.6, 1.2, "#5a3a26", rx=0.2)       # briefcase
    return fig


def pose_front(role, female=False):
    """Facing the viewer: riding in a car, standing in a room."""
    fig = Fig()
    if female and role not in ("security", "medical", "housekeeping"):
        fig.rect(1.85, 2.6, 2.3, 3.0, HAIR, rx=0.8)                                    # long hair, behind
    if role == "guest":
        fig.rect(3.9, 8.5, 1.8, 3.0, "#b5543a", rx=0.3)
        fig.rect(3.9, 9.7, 1.8, 0.3, "#8e3f2b")
    for x in (2.25, 3.0):
        fig.rect(x, 8.0, 0.75, 3.6, LEG, T_LEG, rx=0.25)
        fig.rect(x - 0.1, 11.5, 0.95, 0.5, SHOE, rx=0.2)
    if female and role in SKIRTED:
        fig.poly([(2.0, 7.6), (4.0, 7.6), (4.5, 10.1), (1.5, 10.1)], LEG, T_LEG)
    fig.rect(1.95, 4.5, 2.1, 3.9, TOP, T_TOP, rx=0.6)
    for x in (1.35, 4.05):
        fig.rect(x, 4.7, 0.6, 3.0, TOP_BACK, T_TOPB, rx=0.3)
        fig.circ(x + 0.3, 7.85, 0.36, SKIN, T_SKIN)
    fig.rect(2.7, 4.0, 0.6, 0.7, SKIN, T_SKIN)
    fig.circ(3.0, 2.85, 1.12, HAIR)
    fig.circ(3.0, 3.25, 0.98, SKIN, T_SKIN)
    if role == "worker":
        fig.poly([(2.6, 4.55), (3.4, 4.55), (3.0, 5.2)], "#f2f2ee")
        fig.rect(2.85, 5.0, 0.3, 2.0, "#7a2f36")
        fig.rect(3.8, 7.9, 1.6, 1.2, "#5a3a26", rx=0.2)
    elif role == "housekeeping":
        fig.rect(2.3, 6.2, 1.4, 3.0, "#f1efe8", rx=0.2)
        fig.poly([(1.8, 2.7), (2.1, 1.8), (3.9, 1.8), (4.2, 2.7)], "#f1efe8")
    elif role == "security":
        fig.poly([(1.8, 2.5), (2.0, 1.6), (4.0, 1.6), (4.2, 2.5)], "#1f2a44")
        fig.rect(1.7, 2.4, 2.6, 0.35, "#141b2e")
        fig.circ(3.0, 1.95, 0.22, "#e3b64a")
        fig.circ(2.5, 5.6, 0.25, "#e3b64a")
        fig.rect(1.95, 7.6, 2.1, 0.45, "#141b2e")
    elif role == "medical":
        fig.poly([(1.8, 2.6), (2.0, 1.8), (4.0, 1.8), (4.2, 2.6)], "#f4f6f7")
        fig.rect(2.85, 1.85, 0.3, 0.7, "#c8302b")
        fig.rect(2.65, 2.05, 0.7, 0.3, "#c8302b")
        fig.rect(2.85, 4.6, 0.3, 3.8, "#d9dee2")
    return fig


ROLES = ["civilian", "worker", "guest", "housekeeping", "security", "medical"]
WALK = [((24, -24), (-20, 20)), ((6, -4), (-5, 4)), ((-24, 24), (20, -20)), ((-4, 6), (4, -5))]
STAND = ((3, -3), (6, -4))


def svg(body, comment, w=W, h=H):
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %d %d" width="%d" height="%d">\n'
            '  <!-- units: %dx%d -->\n  <!-- %s -->\n  %s\n</svg>\n'
            % (w, h, w, h, w, h, comment, "\n  ".join(body)))


def mirror(body, w=W):
    return ['<g transform="translate(%d,0) scale(-1,1)">' % w] + body + ["</g>"]


def out(name, fig, comment, flip=False, w=W, h=H, tinted=True):
    pic, tin = (mirror(fig.pic, w), mirror(fig.tint, w)) if flip else (fig.pic, fig.tint)
    with open(os.path.join(HERE, name + ".svg"), "w") as fh:
        fh.write(svg(pic, comment, w, h))
    if tinted:
        with open(os.path.join(HERE, name + ".tint.svg"), "w") as fh:
            fh.write(svg(tin, "recolour layer for %s: red upper clothes, green skin, blue lower clothes" % name, w, h))


# ---- BOX: the emergency services' poses ------------------------------------

def pose_kneel(role, female=False):
    """Kneeling by a patient on the floor to the right: one knee down, the
    other foot flat, both hands reaching forward and down."""
    fig = Fig()
    # back leg: thigh down to the knee on the floor, shin back along it
    fig.poly([(2.4, 8.2), (3.3, 8.2), (2.9, 11.4), (2.0, 11.4)], LEG_BACK, T_LEGB)
    fig.rect(0.3, 11.0, 2.6, 0.8, LEG_BACK, T_LEGB, rx=0.35)
    fig.rect(0.0, 11.3, 0.8, 0.6, SHOE, rx=0.2)
    # front leg: thigh forward level, shin straight down
    fig.rect(2.6, 7.8, 2.6, 0.85, LEG, T_LEG, rx=0.4)
    fig.rect(4.4, 8.2, 0.8, 3.5, LEG, T_LEG, rx=0.35)
    fig.rect(4.3, 11.5, 1.4, 0.5, SHOE, rx=0.2)
    # torso and head lowered by three units, leaning forward
    g = Fig()
    torso(g, role, female)
    head(g, role, female)
    lean = '<g transform="translate(0.6 3.0) rotate(18 3 8)">'
    fig.pic += [lean] + g.pic + ["</g>"]
    fig.tint += [lean] + g.tint + ["</g>"]
    for (x0, y0, x1, y1, back) in ((3.6, 8.4, 5.6, 10.6, True), (3.8, 8.6, 5.9, 11.0, False)):
        pts = [(x0 - 0.3, y0), (x0 + 0.3, y0), (x1 + 0.3, y1), (x1 - 0.3, y1)]
        fig.poly(pts, TOP_BACK if back else TOP, T_TOPB if back else T_TOP)
        fig.circ(x1, y1, 0.36, SKIN_BACK if back else SKIN, T_SKINB if back else T_SKIN)
    return fig


def patient_lying():
    """Someone collapsed on the floor, head to the right: 12 x 12, the body
    along the bottom."""
    fig = Fig()
    fig.rect(1.0, 10.6, 3.8, 0.9, LEG_BACK, T_LEGB, rx=0.4)       # far leg
    fig.rect(0.6, 11.1, 4.4, 0.9, LEG, T_LEG, rx=0.4)             # near leg
    fig.rect(0.2, 10.8, 0.7, 1.2, SHOE, rx=0.2)
    fig.rect(4.6, 10.3, 4.2, 1.7, TOP, T_TOP, rx=0.7)             # body
    fig.rect(5.4, 11.3, 3.0, 0.7, TOP_BACK, T_TOPB, rx=0.3)       # arm along it
    fig.circ(5.3, 11.6, 0.36, SKIN, T_SKIN)
    fig.circ(10.0, 11.0, 1.15, HAIR)
    fig.circ(9.8, 10.9, 0.98, SKIN, T_SKIN)                       # face up
    return fig


def stretcher():
    """The patient on a stretcher carried at the medics' hand height (y 7.6
    of a figure): 14 x 12, the poles running out to the bearers' hands."""
    fig = Fig()
    frame, canvas, blanket, pillow = "#9aa3ab", "#e4e8ea", "#c8473f", "#f4f6f7"
    fig.rect(0.0, 7.7, 14.0, 0.45, frame, rx=0.2)                 # the poles
    for x in (3.2, 10.8):
        fig.rect(x, 8.1, 0.35, 1.3, frame)                        # the legs, folded
    fig.rect(2.4, 7.1, 9.2, 0.7, canvas, rx=0.3)                  # the canvas
    fig.rect(9.4, 5.8, 2.0, 1.4, pillow, rx=0.5)
    fig.circ(10.3, 5.6, 0.9, HAIR)
    fig.circ(10.2, 5.7, 0.8, SKIN)                                # the patient's face
    fig.poly([(2.6, 7.2), (2.8, 5.9), (6.0, 5.4), (9.6, 5.9), (9.6, 7.2)], blanket)
    fig.rect(3.0, 6.3, 6.4, 0.25, "#a63a33")
    return fig


def emergency():
    for role in ("medical", "security"):
        for suffix, female, k in VARIANTS:
            out("%s%s-kneel-right" % (role, suffix), scaled(pose_kneel(role, female), k),
                "%s kneeling by a patient, facing right" % (role + suffix))
    out("patient-lying", patient_lying(), "a patient collapsed on the floor, head to the right", w=12, h=12, tinted=False)
    out("stretcher-right", stretcher(),
        "a patient on a stretcher, head to the right, carried between two medics", w=14, h=12, tinted=False)


def scaled(fig, k):
    """The figure k times its size, its feet where they were (x 3, y 12)."""
    if k == 1:
        return fig
    g = Fig()
    t = '<g transform="translate(3 12) scale(%s) translate(-3 -12)">' % f(k)
    g.pic = [t] + fig.pic + ["</g>"]
    g.tint = [t] + fig.tint + ["</g>"]
    return g


# BOX: everyone is not one tall man.  Each role as a man and as a woman,
# each at full height and a tenth shorter ("" -f -s -fs); children ("child",
# "child-f") at about three fifths; dogs.  The engine chooses by a hash of
# the person.
VARIANTS = [("", False, 1.0), ("-f", True, 1.0), ("-s", False, 0.9), ("-fs", True, 0.9)]


def dog(frame):
    """A dog, walking (frames 0-3) or standing (None), facing right: 6 x 12,
    feet on y = 12, about knee high to a grown-up."""
    fig = Fig()
    coat, dark = "#8a5a32", "#5e3c20"
    sw = [(14, -14), (4, -4), (-14, 14), (-4, 4)][frame] if frame is not None else (3, -3)
    for (x, a) in ((1.8, sw[1]), (2.4, sw[0]), (3.9, sw[0]), (4.5, sw[1])):
        pts, _ = limb((x, 10.0), a, 2.0, 0.42)
        fig.poly(pts, dark if x in (1.8, 3.9) else coat)
    fig.add(lambda c: '<ellipse cx="3.1" cy="9.6" rx="1.9" ry="0.85" fill="%s"/>' % c, coat)    # body
    fig.poly([(4.5, 9.3), (5.0, 8.0), (5.6, 8.3), (5.2, 9.6)], coat)                            # neck
    fig.add(lambda c: '<ellipse cx="5.35" cy="8.1" rx="0.6" ry="0.48" fill="%s"/>' % c, coat)   # head
    fig.poly([(5.6, 8.05), (6.0, 8.25), (5.95, 8.45), (5.5, 8.4)], dark)                        # muzzle
    fig.poly([(5.0, 7.75), (5.25, 7.4), (5.35, 8.1)], dark)                                     # ear
    fig.poly([(1.3, 9.3), (0.5, 8.4), (0.7, 8.25), (1.5, 9.0)], dark)                           # tail
    fig.rect(4.7, 8.6, 0.45, 0.22, "#c8302b")                                                    # collar
    return fig


def main():
    for role in ROLES:
        for suffix, female, k in VARIANTS:
            name = role + suffix
            who = "%s (%s%s)" % (role, "woman" if female else "man", ", shorter" if k < 1 else "")
            for side, flip in (("right", False), ("left", True)):
                out("%s-stand-%s" % (name, side), scaled(pose_side(role, *STAND, guest_pull=True, female=female), k),
                    "%s standing or waiting, facing %s" % (who, side), flip)
                for i, (legs, arms) in enumerate(WALK):
                    out("%s-walk-%s.%d" % (name, side, i),
                        scaled(pose_side(role, legs, arms, guest_pull=True, female=female), k),
                        "%s walking, facing %s, frame %d of 4" % (who, side, i), flip)
            out("%s-front" % name, scaled(pose_front(role, female), k), "%s facing the viewer (riding, or in a room)" % who)
    for name, female in (("child", False), ("child-f", True)):
        who = "a child (%s)" % ("girl" if female else "boy")
        for side, flip in (("right", False), ("left", True)):
            out("%s-stand-%s" % (name, side), scaled(pose_side("civilian", *STAND, female=female), 0.62),
                "%s standing, facing %s" % (who, side), flip)
            for i, (legs, arms) in enumerate(WALK):
                out("%s-walk-%s.%d" % (name, side, i), scaled(pose_side("civilian", legs, arms, female=female), 0.62),
                    "%s walking, facing %s, frame %d of 4" % (who, side, i), flip)
        out("%s-front" % name, scaled(pose_front("civilian", female), 0.62), "%s facing the viewer" % who)
    for i in range(4):
        out("dog-walk-right.%d" % i, scaled(dog(i), 0.75), "a dog walking, facing right, frame %d of 4" % i, tinted=False)
    out("dog-stand-right", scaled(dog(None), 0.75), "a dog standing, facing right", tinted=False)
    emergency()


if __name__ == "__main__":
    main()
