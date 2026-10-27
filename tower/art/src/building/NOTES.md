# building/

The structure of the tower itself.  Every file is drawn by `pieces.py` in
this directory (it also draws `world/` and `events/`); change a piece there
and run `python3 art/src/building/pieces.py`.  It replaces only the SVGs
that carry its own marker comment.

All pieces are grid slots: 1 cell x 1 floor (4 x 16 units) unless stated.
Transparent where nothing is drawn, so most of them are overlays.  A
floor's walking line (top of the slab) is y = 15 in every piece.

| Piece | Size | Where it goes |
| --- | --- | --- |
| `slab` | 1x1 | over every built cell, rooms included, last but walls; bottom 1 unit |
| `floor`, `floor-column` (+emit) | 1x1 | a floor laid with no room.  Use `floor-column` where cell x mod 3 = 0, `floor` elsewhere; the column's work lamp glows at night |
| `wall-left`, `wall-right` | 1x1 | over the leftmost / rightmost cell of each floor's run (also on the ground floor) |
| `roof` | 1x1 | in the floor ABOVE the top built floor of each column (so it sits on the empty sky cell) |
| `roof-light` (+emit) | 1x1 | instead of `roof` where cell x mod 4 = 0 |
| `roof-left`, `roof-right` | 1x1 | instead of `roof` above the end cells of a run (and at step-backs) |
| `roof-tank`, `roof-plant` | 2x1 | instead of two `roof` cells, now and then (hash of x; not at the ends) |
| `roof-beacon.0-1` (+emit) | 1x1 | one on the highest roof, middle cell; blinks 1 frame a second (0 = lit) |
| `base` | 1x1 | over every building cell of the ground floor (floor 10): granite plinth, over the slab |
| `basement`, `basement-light` (+emit) | 1x1 | an underground floor laid with no room; `basement-light` where x mod 3 = 0 |
| `basement-left`, `basement-right` | 1x1 | retaining wall, ends of an underground run (instead of wall-left/right) |
| `construct.0-2` (+emit on 1) | 1x1 | a cell being built; replaces the room.  Loop at 3 frames a second, frame = (t + x) mod 3 so neighbours are out of step |
| `rubble`, `rubble-b` | 1x1 | a cell demolished or blown up and not yet cleared; alternate by x |
| `dust.0-2` | 1x1 | over a cell as it is demolished: play once, 6 frames a second, then show rubble |

Draw order within a cell: room or empty floor -> slab -> base (ground floor)
-> wall ends -> roof pieces in the cell above.

None of these pieces have room art in them, so the presenter can tint the
slab and walls with the night shader like everything else.
