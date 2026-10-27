# A fire

*A BOX addition, 27 October 2026.*  In the 1994 game a fire burns rooms
out along a floor and climbs, and the people in the tower carry on as if
nothing were happening.  Under BOX RULES the tower lives through it.  A
classic game is unchanged.

It is meant for children as well as grown-ups, so nobody is hurt: the
alarm sounds, everyone walks out, and everyone comes back.

## The alarm

When a fire starts, everyone at rest on the fire floor, on the floor below
it and on every floor above goes out (box_ext/fire.rs).  People already on
their way somewhere carry on, and are sent out when they stop on one of
those floors.  Each person out is held: they do nothing until they come
back.  The engine draws them leaving from their own rooms, a few at a time,
along their floor to the nearer fire escape, down it and away along the
street (docs/fire-escapes.md has the escapes).

Going out is a trip of its own: `EVACUATION_STRESS` (40) is added to each
person's trip stress, so the rooms' ratings dip for a while after a fire.

## The all-clear

`ALL_CLEAR_TICKS` (100, about an hour) after the fire is out, everyone goes
back in, except the people of the floors the fire reached.  Those floors
are closed for the rest of the day: their rooms are drawn grey with their
lamps off, and their people come back at the day's end, when the floors
open again.  Their shops, restaurants and fast food take no more visitors
that day (closed as at night; they open with the others the next morning),
so they earn less at closing.

The Tower pane shows the fire alarm, how many are out, and the floors
closed.

## Where a fire may go

Under BOX RULES a fire climbs only from a floor that is burning (box_ext/
street.rs), and never into a floor with nothing built on it.  Upstream's
fire climbs into the empty sky above the tower and goes on "burning" there
until the cleanup at four in the afternoon.

## The sprinkler room

An amenity (docs/amenities.md): 8 cells, above the lobby, from 3 stars,
$200,000, $10,000 every three days.  It covers the floors within
`SPRINKLER_REACH` (5) of its own, up and down.  On those floors a fire
spreads at half speed, and a fire does not climb into them.  It takes
effect from the day's end after it is built, as the other amenities do.

## Tests

sim/tests/fire.rs: everyone on and above the fire goes out and comes back
(the closed floors' people the next day); going out adds to the stress;
sprinklers save rooms and stop the climb; a classic fire sends nobody out.
