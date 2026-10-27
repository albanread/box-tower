# Vehicles

All drawn by `gen.py` in this directory (`python3 gen.py` rewrites every
SVG here).  Edit the generator, not the SVGs.  No badges, lettering or
shapes from any real make.

## Size and anchor

Every ordinary car is 24x10 units (the emergency vehicles are below): 6 cells long, under two-thirds of a floor
high, tyres touching the bottom edge.  Put the bottom edge on the floor
surface (y = 15 of the floor).  Shorter bodies (the hatchback) are centred
in the same box, so every vehicle shares one atlas slot and one anchor.
On the car lift platform (28 wide) a vehicle sits 2 units in from the
platform's left edge.

## The set

`saloon` (red), `hatchback` (blue), `estate` (green, roof rails), `van`
(white with a stripe, tall), `taxi` (amber, roof lamp, chequer band),
`pickup` (grey, open bed).

| File | What |
| --- | --- |
| `TYPE-right.N.svg` | facing right (front at the right), wheel frame N = 0, 1 |
| `TYPE-left.N.svg` | the same, mirrored |
| `*.emit.svg` | headlamp with a short beam, tail lamp; the taxi's roof lamp |

The two wheel frames turn the crossed spokes by 45 degrees, half their
repeat; alternate them about every 2 units driven and the wheels read as
turning (two frames cannot show which way, which at this size nobody sees).  A presenter may flip
the right-facing pictures instead of loading the left ones.

## Emergency vehicles

No lettering, crests or real services' markings: colour, stripes, and on
the ambulance a plain six-armed star made of three bars.

| Type | Size (units) | Cells | Wheels |
| --- | --- | --- | --- |
| `ambulance` | 28x12 | 7 | 2 |
| `police` | 24x10 | 6 | 2 (the saloon's shape) |
| `fire-pumper` | 36x13 | 9 | 2 |
| `fire-ladder` | 40x13 | 10 | 3 (tandem rear) |
| `fire-ladder-deployed` | 40x13 | 10 | the same truck parked: outriggers down, no ladder on its back |
| `fire-ladder-extended-right/left` | 24x44 | 6 x 2.75 floors | the raised ladder, a separate piece |

All stand with their bottom edge on the floor surface, as the cars do.

Files for each type T (and both sides S = `right`, `left`):

| File | What |
| --- | --- |
| `T-S.N.svg` + `.emit.svg` | body, wheel frame N = 0, 1; beacons drawn unlit; emit = head and tail lamps |
| `T-S-beacon.B.svg` + `.emit.svg` | beacon overlay, B = 0..3: the lit lamps (day picture) and their glow (emit) |

Draw the body, then the beacon overlay of the current frame over it, in
the same place.  The lit lamps are in the day picture, so they show by
day; the emit layer adds the glow at night.  Frames: 0 group A lit, 1
dark, 2 group B lit, 3 dark; groups alternate front/back and blue/red.
Step a frame every 125 ms (each lamp flashes twice a second); start each
vehicle at a random frame so a row of them does not flash in step.  The
wheel frames and the beacon frames are independent.

### The raised ladder

`fire-ladder-extended-right.svg` (24x44) goes with `fire-ladder-deployed-
right`: its pivot is at (2, 42) in the piece and the turntable at (8, 3)
in the truck, so the piece's top-left is at truck + (6, -39).  For the
left-facing pair the pivot is (22, 42) and the turntable (32, 3): piece at
truck + (10, -39).  The tip is 41 units above the turntable, which
is 10 above the street: about 51 units, a little over three floors.  It
rests against a wall 21 units (about 5 cells) ahead of the turntable, so
park the truck with the turntable that far from the building's face.  Its `.emit.svg` is a floodlight at the tip.  To raise or
lower it, swap `fire-ladder` for `fire-ladder-deployed` plus this piece.
