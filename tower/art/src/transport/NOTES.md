# Transport pictures

All drawn by `gen.py` in this directory (run `python3 gen.py`; it rewrites
every SVG here).  Edit the generator, not the SVGs.  Sizes are in units: a
cell is 4, a floor 16.  On every floor the floor surface is y = 15 of that
floor; the bottom unit is the slab.

## Passenger lifts

Three kinds, after `carrierMode`: 0 express (`elevator-express`, 6 cells),
1 standard (`elevator`, 4 cells), 2 service (`elevator-service`, 4 cells).

| Kind | Look |
| --- | --- |
| standard | warm concrete shaft, steel frames, clear glass landing doors, yellow car |
| express | dark blue braced shaft with rail lights, brass frames, blue glass, blue car |
| service | block shaft with hazard bands, mesh gates, grey steel car, padded walls |

| Piece | Size (std/service, express) | Where |
| --- | --- | --- |
| `K.svg` | 16x16, 24x16 | the shaft, once for every floor of the run |
| `K-top.svg` | 16x8, 24x8 | machine room, its bottom edge on the top edge of the topmost floor |
| `K-pit.svg` | 16x4, 24x4 | pit, its top edge on the bottom edge of the lowest floor |
| `K-landing.N.svg` | 16x16, 24x16 | on each floor the lift serves; N = 0 closed, 1, 2 opening, 3 open |
| `K-landing-skip.svg` | 16x16, 24x16 | on each floor of the run the lift does not serve |
| `K-car-load.N.svg` | 12x13, 20x13 | car, back layer: interior and riders; N = 0 empty, 1 a few, 2 about half, 3 full |
| `K-car-doors.N.svg` | 12x13, 20x13 | car, middle layer: door frames as for the landing |
| `K-car.svg` | 12x13, 20x13 | car, front layer: roof, posts, platform |

`.emit.svg` layers: express shaft (rail lights), express top (beacon),
every landing frame (hall lantern, call button), skip landing (no-stop
sign), every car shell (cab light, which shows through the doorway).

The machine room and pit stand outside the run's floors.  Draw them
behind rooms, so they show only where nothing is built (on the roof, below
ground); a room above the top floor simply covers the machine room.

### Draw order, back to front

1. shaft pieces (and top, pit);
2. each car: `car-load.N`, then `car-doors.N`, then `car`;
3. landings (`landing.N` or `landing-skip`).

The landings are in front of the car on purpose: the doors are see-through
(glass, or a mesh gate), so a car moving in the shaft is seen through
them, and the header hides the car's roof as it passes a floor slab.

### Placing a car

The car is 2 units in from the shaft's left edge (`(shaftW - carW) / 2`).
At floor f its top is 2 units below the top of that floor: it fills
y = 2 .. 15, the platform on the floor surface.  Between floors move it
linearly; it is the same height wherever it is.

### Doors

When a car stops, play `car-doors.0..3` and the landing's `landing.0..3`
together (the two sets are drawn to the same four steps), hold at 3 for
the dwell, then play them back 3..0.  Upstream's dwell is in ticks; a frame
a tick or two looks right.

### Riders

The load is baked into the car, not drawn as people sprites.  A standard
car holds 21 and an express 42 (`assignment_capacity`); there is no room
to draw that many at 4 pixels a unit, and upstream does not show riders'
stress, only a count.  So: pick N from the car's occupancy over its
capacity (0 empty, 1 up to a third, 2 up to two thirds, 3 more), and
draw the count as a number if wanted.  One picture a car, no per-rider
sprites.  (The people family has a `-front` pose if the program ever wants
to draw riders itself over `car-load.0`.)

## Car lift (a BOX addition)

8 cells wide (32 units): a 24-unit vehicle on a 28-unit platform, with
heavy side walls.  Shaft, top and pit stack as for the passenger lifts.

| Piece | Size | Where |
| --- | --- | --- |
| `car-lift.svg` | 32x16 | the shaft, every floor of the run, the street floor included |
| `car-lift-top.svg` | 32x8 | machine room, on top of the street floor |
| `car-lift-pit.svg` | 32x4 | pit, under the lowest basement |
| `car-lift-entrance.N.svg` | 32x16 | street floor: canopy and roller shutter, N = 0 shut .. 3 open (+ emit) |
| `car-lift-landing.N.svg` | 32x16 | served basement: bi-parting freight door, N = 0 shut .. 3 open (+ emit) |
| `car-lift-landing-skip.svg` | 32x16 | basement floor not served (+ emit) |
| `car-lift-platform.svg` | 28x16 | the platform (+ emit: floodlight and beacons) |

Draw order: shaft, platform, vehicle, then entrance or landing.  The
platform sits 2 units in from the shaft's left edge; at a floor it covers
that floor exactly (deck top at y = 15), so a vehicle on it stands with its
bottom edge on y = 15, centred on the platform (2 units in from the
platform's edge).  The vehicle drives in and out sideways through the open
door, passing behind the frame's jambs.

## Stairs and escalator

Both 8 cells by one floor (32x16), rising left to right from this floor's
surface (y = 15) to the next floor's (y = -1, just above the top edge).
Mirror them for a flight rising right to left.  `escalator.0..3` are the
steps moving up, a quarter-step a frame; play 3..0 for a down escalator.
Both are redrawn from tower-together's `stairs.svg` and `escalator.svg`
(MIT, Patrick Hulin) to our grid; the files say so.
