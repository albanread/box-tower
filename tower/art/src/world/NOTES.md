# world/

The ground, the street and the sky's furniture.  Drawn by
`art/src/building/pieces.py` (see building/NOTES.md).

## On the grid (1 cell x 1 floor, 4 x 16 units)

| Piece | Where |
| --- | --- |
| `street` | floor 10 (ground floor) in every cell outside the building: the pavement, top at y = 13.6, ground line at y = 16 |
| `street-lamp` (+emit) | instead of `street` every 8 cells or so |
| `street-tree` | 2x1, instead of two `street` cells |
| `street-bench`, `street-hydrant`, `street-planter` | instead of `street`, by a hash of x; keep them clear of the building's ends |
| `road` | floor 10 where a road is wanted (the parking ramp's mouth); same ground line |
| `soil-top-a/b` | floor 9 (just under the street) where nothing is built: road base over soil |
| `soil-a/b/c/d` | floors 5-8 unbuilt |
| `rock-a/b/c/d` | floors 0-4 unbuilt.  The two strata lines are at the same height in every variant, so they run on across cells |

Pick soil and rock variants by a hash of (x, floor), never x mod 4, so no
pattern shows.  Underground cells inside a basement run use
`building/basement*` instead.

## Free sprites (sizes in units)

| Piece | Size | Use |
| --- | --- | --- |
| `cloud-a` .. `cloud-g` | 20-64 x 8-14 | drift left to right at 1-4 units a second, more slowly for small ones (further away); 6-12 in view.  Fade them at night with the shader |
| `skyline-far` (+emit) | 400 x 96 | parallax backdrop; repeats every 400 units; its bottom on the ground line; scroll at 0.25 of the camera |
| `skyline-near` (+emit) | 400 x 96 | the same at 0.5 of the camera, in front of the far one |
| `sun` (+emit) | 24 x 24 | crosses the sky by day |
| `moon` (+emit) | 16 x 16 | by night |
| `star.0-1` (+emit) | 4 x 4 | night sky; frame 0 is the twinkle, show it a few ticks at random |

The sky gradient itself is the shader's.  The skylines' emission layers
are their lit windows, to be added only by night, each at its own depth
(behind the tower, never over it).
