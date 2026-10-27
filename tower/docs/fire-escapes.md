# Fire escapes

*A BOX addition, 26 October 2026.*  The 1994 game draws its towers without
any way out but the lobby.  A real building of the size has fire escapes,
and ours has them too: a steel stair outside each end of the floors, which
people use when there is a fire.  They are drawn and walked by the engine
(ports/rosapps/tower-engine); the simulation knows nothing of them, so a
classic game is unchanged.

## Where they stand

`publish.rs` works them out each time it publishes the building, from the
cells: each row's runs of built cells, from the ground floor up (the rows
under the street have none).

- Beside each end of each run, outside it, a stair two cells wide, with a
  landing level with the floor.  A cell that is built is never used: two
  towers closer than two cells have no stair between them.
- Where the floor above steps back from this one's end, a landing on this
  floor's roof, so that people coming down onto the roof can reach it.
- A stair whose foot hangs over nothing (beside floors that overhang those
  below) goes on down until it stands on a roof or reaches the street.

Each unbroken column of these is one escape: `Escape { side, x, top,
bottom }`.  So where the ends of the floors are level, as in most towers,
an escape rises straight from the street to the top floor; at a step-back a
new escape stands on the roof below.

## The pictures

`art/src/building/pieces.py` draws a storey of stair two cells wide (the
building to the right of it, mirrored for the right end): the landing at
the floor, and two flights climbing from it to the landing above, outer end
to inner and back.  The top landing has its rail and nothing above it.
A back piece repeats every its own width, so each is cut into its two
cells: `building/escape-{flight,top}-{left,right}-{out,in}`.  They go in a
cell's back layer over the sky, or its front layer over a roof or the
street.

## Who uses them

Nobody but in a fire.  Everyone else comes and goes by the lobby: people
leaving a car on any floor walk along that floor, and only the lobby opens
on the street.

While a fire burns (`fire_fronts`), people come out of the floors above
the lobby, a few a second, as many in all as the tower's population: each
from a place on a floor, to the nearer escape beside that floor's run,
down it flight by flight (a storey counts as three cells walked), across a
roof to the next escape where the floors step back, and away along the
street.  They are drawn people, standing for the tower's traffic, as all
the engine's walkers are.

## The emergency services' fire stairs

The escapes are worked out by the simulation (`sim/src/box_ext/escapes.rs`;
the engine draws the same ones).  Under BOX RULES, where no lift, stairs or
escalator the player built reaches a patient's floor, the medics, the guard
and the stretcher go by a fire escape: the planner's fire stairs' leg
(emergency.rs) walks the team to the landing of the nearest escape beside
their floor whose stair reaches the other floor, the climb takes
`FIRE_STAIRS_TICKS_PER_FLOOR` a floor, and they walk in from the landing at
the other end.  The views give each climber's escape and how far along it
they are (`PersonView::climb`), and the engine draws them going up or down
its flights, the stretcher between the bearers.  Where no escape spans both
floors, the team goes as it went before: from where it stands.
