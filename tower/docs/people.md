# People

*A BOX addition, 26 October 2026.*  The 1994 game counts its people (six
to an office, three to a condominium, a hotel room's guests) but does not
say who they are.  Under BOX RULES the tower keeps a register of them,
worked out from the tower as it stands whenever it is asked for
(`sim/src/box_ext/people.rs`); nothing of it is saved, and it changes
nothing in the simulation.

## Households

Each occupied condominium is a household of three, as upstream counts it:
two adults and a child, one adult and two children, or three adults,
chosen by a hash of its place.  Hotel guests are counted apart: they live
elsewhere.

## Jobs

Every room open gives jobs:

| Trade | Rooms, and jobs each |
| --- | --- |
| Offices | an office let, 6 |
| Catering | restaurant 6, fast food 4, premier lounge 4 |
| Shops | shop 3 |
| Hotel | housekeeping 4, laundry 3, guest services 3, a front desk and room service for every six rooms |
| Entertainment | cinema 5, party hall 4, convention centre 6 |
| Services | security 6, recycling 4, metro 4, parking ramp 1, car elevator 1 |
| Health | medical centre 6 |
| Education | kindergarten 4, primary school 12 |
| Leisure | pool 3, gym 2 |

Four in ten of the tower's adults work in the tower while there are jobs
left, three in ten work outside, and three are at home (retired, caring,
between jobs); the rest of the tower's jobs are held by people who
commute in.

## Children and schools

Four in ten children are under five and go to a kindergarten, the rest to
a primary school: the tower's own while there are places (30 a
kindergarten, 150 a school), outside the tower otherwise.  The
kindergarten (12 cells, two stars, $120,000, $12,000 upkeep) charges $25 a
child a day; the primary school (30 cells, two floors, three stars,
$600,000, $40,000 upkeep) is free.  Both are amenities
(docs/amenities.md): they serve the condominiums, whose rating they lift
(20 and 30), and are busy on weekday daytimes.

## On the screen

The Tower pane has a People section: who lives here and in how many homes,
the children, hotel guests, the jobs here, how many are held by residents
and how many commute in, the residents working outside, the children at
school in the tower, and the jobs by trade.  The people drawn are men and
women of several heights, children walking with their parents, and dogs
(art/src/people/gen.py; the engine chooses a figure by a hash of the
person).

## Shifts and the school run

The engine draws the register's comings and goings (movers.rs,
`shifts_and_school`), as traffic, not one for one.  Each kind of
workplace has its shifts (shops from 9 till half past 6, restaurants from
half past 10, security at 7 and 7 again, the school from a quarter to 8,
the pool from a quarter past 6, the lobby's desk at half past 6, half past
2 and half past 10 ...); as the clock passes a shift's start two or three
of the room's staff come in, one in five from a home in the tower and the
rest from the street through the lobby, to a lift that serves both
floors, unseen while they ride, and on to the room; as it passes the end
they go home the same way.  On a weekday at about twenty to 8 a parent
walks each household's children to the tower's kindergarten or school, or
out to the street for a school outside, and at about twenty to 4 brings
them home.

## Working at home (BOX RULES)

The register feeds the simulation (box_ext/locals.rs).  At each day's end
the office jobs among those the tower's residents hold are given to that
many office workers (by a hash of their place, never one who drives), each
with a home in one of the tower's condominiums.  In the morning such a
worker sets off from their home's floor instead of the lobby, and in the
evening goes back to it, when a lift, stairs or escalator joins the two
floors.  So a tower that houses its own workers has less traffic through
its lobby.  The engine draws them out of their front doors to the lift and
back.  Those who come by train (docs/metro.md) are the next choice; the
rest come in by the lobby.

Children at school in the tower already make their households happier:
the kindergarten's and the school's relief on the condominiums' ratings
(docs/amenities.md).

sim/tests/locals.rs checks it.

## Routes: short hops and the walk

Under BOX RULES two things change in how people choose and pay for a
route; a classic game is unchanged.

- A lift's queue counts against it.  The route scan
  (route_scoring/select_candidate.rs) adds `QUEUE_COST_PER_PERSON` (13)
  for each person already waiting at the floor in the direction of travel,
  so when a lift is crowded a short hop goes by the stairs or an escalator
  instead.
- The walk is part of the trip.  Upstream charges stress for a walk only
  past 80 cells (30) or 125 (60).  We charge a tick for each cell walked
  between the room and its lift or stairs beyond `FREE_WALK_CELLS` (6), in
  sims/scoring.rs, so an office at the far end of a floor from its lift
  rates worse than one beside it.

## The rating, step by step

Under BOX RULES the Room pane shows how a rated room's evaluation is made
up (box_ext/rating.rs, which takes the steps sims/scoring.rs takes without
changing anything):

- its people's average trip stress;
- the walk to the nearest lift or stairs serving its floor, in cells, and
  what that walk adds to each trip (included in the trip stress);
- the rent: +30 for a low rent, -30 for a high one;
- +60 for a noisy neighbour;
- what the tower's amenities take off;
- the score, and the star rating's thresholds: under the first is Good,
  under the second Fair, otherwise Bad.

sim/tests/box_routes.rs checks all three.
