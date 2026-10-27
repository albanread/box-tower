# Amenities

*A BOX addition, 26 October 2026.*  A tower of the 1994 game houses people
and keeps them waiting for lifts, but gives them nothing to do in it beyond
eating and shopping.  Six new rooms give the tenants, residents and guests
the services a real building of the size would have, and the security
office, which the game already has, gets more pictures.

They are BOX RULES rooms (docs/emergency-services.md).  With BOX RULES off,
`place_tile` refuses them as upstream refuses any tile it does not know
("Invalid tile type"); a tower that has them and has its rules turned off
keeps them standing, inert, until the rules are turned on again.

The code is `sim/src/box_ext/amenities.rs`; its tests are
`sim/tests/amenities.rs`.

## The rooms

| Room | Tile type | Size | Where | Stars | Cost | Upkeep, 3 days |
| --- | --- | --- | --- | --- | --- | --- |
| Laundry | `laundry` | 12 cells | above ground | 2 | $80,000 | $10,000 |
| Guest services | `guestServices` | 10 cells | in a lobby (the ground floor's or a sky lobby) | 3 | $100,000 | $15,000 |
| Premier lounge | `premierLounge` | 20 cells | above ground | 4 | $300,000 | $30,000 |
| Swimming pool | `pool` | 30 cells | a basement floor | 3 | $400,000 | $20,000 |
| Gym | `gym` | 16 cells | a basement floor | 3 | $150,000 | $10,000 |
| Convention centre | `convention` | 30 cells, 2 floors | above ground, two clear floors | 4 | $1,000,000 | $50,000 |
| Sprinkler room | `sprinklers` | 8 cells | above ground | 3 | $200,000 | $10,000 |

The sprinkler room serves no units: it slows fires on the floors within
five of its own (docs/fire.md).

A room is placed as any other: on bare floor of its width (both floors for
the convention centre, clicked on the lower), never over a shaft or another
room.  Guest services are the exception: a concierge's desk in the lobby,
built over lobby cells, which are lobby again when it is removed.  As for parking, the stars are checked by the front end, through
`get_tile_star_requirement`.  Upkeep is charged with the other running
costs, every third day.

## What they do

Each room serves some kinds of unit: it makes their occupants' stay
better, and so the unit's evaluation.  Upstream works a unit's evaluation
out from its occupants' average trip stress, adds 60 when a noisy neighbour
is near, and compares the score with the star's two thresholds (blue,
yellow, red).  Under BOX RULES an amenity serving the unit takes its relief
off that score, after the noise:

| Room | Serves | Relief | Units each |
| --- | --- | --- | --- |
| Laundry | hotel rooms | 20 | 60 |
| Guest services | hotel rooms, condominiums | 20, 10 | 80 |
| Premier lounge | offices, suites | 20, 30 | 40 |
| Swimming pool | condominiums, hotel rooms | 30, 20 | 60 |
| Gym | offices, condominiums | 20, 20 | 60 |
| Convention centre | hotel rooms | 10 | 120 |

A room serves at most its number of units, the nearest first (by floors,
then by cells), and only once it is open: a day after it was built, and
only while the rules are on.  Two rooms of one kind serve twice as many
units, not one unit twice; reliefs of different kinds add up, to at most
60 in all.  So a tower can win back what its noise or its lifts lose, but
amenities cannot make a slow tower good.

## What they earn

| Room | Income |
| --- | --- |
| Swimming pool | $40 a visit; a visit is one occupant of a unit it serves, once a day, on a weekend day or after 5 p.m. |
| Gym | $25 a visit, as the pool's, on any day before 9 a.m. or after 5 p.m. |
| Premier lounge | $60 a visit, a served office's worker at midday on a weekday |
| Convention centre | on a weekday, $20,000, and $500 more for each occupied hotel room in the tower |

The visits are counted, not walked: the simulation has no one walking, and
an amenity's visitors do not take the lifts.  The presenter shows that a
room is busy (below).  Laundry and guest services earn nothing; the hotel
earns more by them.

## What is saved

`box_ext.amenities`: the rooms (kind, left cell, row, the tick built),
today's visits by room, and the tallies.  An empty list is not written, so
a tower without amenities saves as before.

## What the presenter reads

The cells carry the tile type, the left column the anchor, as any wide
room.  `box_views().amenities` gives each room's kind, place, whether it is
open, how many units it serves and whether it is busy now (in its hours,
with someone served).  The engine draws the room's picture, and in its
hours the busy one (the room with people in it), lit by the room's lamp
profile.

## The pictures

art/src/rooms: `laundry`, `guest-services`, `premier-lounge`, `pool`, `gym`,
`convention` (two floors), each with `.busy` and an emission layer, and
`security.1`, `security.2` (the security office's variants, chosen by
place as other rooms').  The tool palette has a new group, Amenities
(`tw_amenity`), with the six rooms.

## Hooks in the translated files

- sims/scoring.rs `recompute_object_operational_status`: the relief, after
  the noise (`// BOX: amenities`).
- commands.rs `place_tile` and `remove_tile`: through
  `amenities::intercept_place` and `intercept_remove`, as the car elevator.
- tick: visits and income in `amenities::tick`, beside the emergency tick.
- the three-day expenses: `amenities::upkeep`.

## Hotels by appeal (BOX RULES)

Upstream lets every hotel room that is not rated badly every night.  Under
BOX RULES a room is booked on a night by chance (box_ext/hotels.rs): 70 in
a hundred, 10 more if it is rated good, and 5 more for each kind of amenity
guests value that the tower has open -- laundry, guest services, a pool, a
convention centre, a premier lounge -- to a hundred.  A room with no
booking stays empty that night, as upstream's suites do below three stars.
So the amenities pay through the hotel's takings as well as its ratings.
The Room pane gives a hotel room's chance ("Booked").  Housekeeping still
needs a service lift to reach the rooms, as upstream's does.

sim/tests/hotels.rs: four nights of 44 twin rooms, every room every night
in a classic game, about seven in ten under BOX RULES, nearly all with four
kinds of amenity.
