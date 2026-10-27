# events/

What sim/src/events.rs and cathedral.rs make happen, drawn by
`art/src/building/pieces.py` (see building/NOTES.md).  There is no Santa
or other seasonal event in the simulation, so none is drawn.  The random
news event is sound only.  The "VIP special visitor" in events.rs toggles
the metro (rooms' art); the VIP pieces here are for the VIP suite's guest.

Grid pieces are 1 cell x 1 floor (4 x 16 units) unless stated; figures are
drawn in a 4 x 16 cell with feet on y = 15 and face right (mirror to walk
left).

| Piece | Size | Use, timing |
| --- | --- | --- |
| `fire.0-5` (+emit) | 1x1 | every burning cell between the fronts (span = left front .. right front + 12, as upstream).  10 frames a second, frame = (t + x) mod 6.  Drawn OVER the room; replaces upstream's red overlay |
| `fire-left.0-5`, `fire-right.0-5` (+emit) | 1x1 | the end cells of the span: lower flames, tapering outwards |
| `smoke.0-3` | 1x2 | bottom floor on the burning floor, top in the floor above; on every third burning cell; 6 frames a second |
| `burnt` (+emit: embers) | 1x1 | cells the fire has deleted, until something is built there |
| `helicopter.0-3` (+emit) | 8x1 | rescue helicopter, faces left (it works right to left: helicopter_extinguish_pos).  Rotor 20 frames a second.  Hover with its tail over the extinguish position, one floor above the highest burning floor |
| `water.0-2` | 1x1 | under the helicopter, over the burning cells being put out; 10 frames a second |
| `firefighter.0-1` (+emit: reflective band) | 1x1 | the rescue helpers (fire_rescue_helpers): walk left; 4 frames a second |
| `bomb.0-1` (+emit) | 1x1 | where the bomb is, while the search runs; blinks 2 a second |
| `explosion.0-3` (+emit) | 4x2 | centred on the bomb's cell, bottom floor the bomb's; once, 8 frames a second; then `building/rubble` over the blast rectangle |
| `vip-car` (+emit) | 5x1 | the VIP's car at the kerb on floor 10, facing left |
| `vip.0-1` | 1x1 | the VIP walking; 4 frames a second |
| `guest-a.0-1`, `guest-b.0-1` | 1x1 | cathedral guests (CATHEDRAL_FAMILIES sims) walking; 4 frames a second |
| `rating-star` (+emit) | 16 x 16 units | free sprite: shown over the tower when StarAdvanced arrives; scale up and fade over 2 seconds |
| `fireworks.0-4` (+emit) | 32 x 32 units | free sprite over the roof at the same moment; 5 frames a second, two or three at once offset in time and place |
