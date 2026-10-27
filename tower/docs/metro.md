# Coming by train

*A BOX addition, 27 October 2026.*  Upstream's metro station is a picture
and one of the fifth star's conditions; nobody uses it.  Under BOX RULES
some of the office workers come by train.  A classic game is unchanged.

## Who rides

One in three office workers who do not drive (by a hash of their place;
the drivers are sims/office.rs's choice, a quarter of them).  They ride
only when the tower has a station and a lift, stairs or escalator from the
station's floor (its top floor, the ticket hall) to their office's floor:
`box_ext/metro.rs` asks the route scorer, without changing anything.

In the morning a rider sets off from the station's floor instead of the
lobby; in the evening they go home to it.  So a station takes part of the
rush off the lobby and its lifts, and puts it on the lifts that go down to
the station.  A lift that reaches the station is worth building.

Hooks in sims/office.rs (each `// BOX: metro`): the morning gate's
`arrival_floor`, the evening departure's `exit_floor`, and its arrival test
`is_exit`.

## On the screen

The engine (movers.rs) draws a rider who has just joined a lift's queue at
the station walking up from the platform to the ticket hall and along to
the lift; one getting out of a lift there walks to the stairs and down to
the platform.  The trains run as before.  The Tower pane counts the office
workers who come by train ("By train").

## Tests

sim/tests/metro.rs: with a station some set off from it and the lobby is
less busy; with BOX RULES off nobody does.  The play-test tower
(caches/tower/Metro.json) comes from the ignored writer in
sim/tests/amenities.rs: three floors of offices and a lift from the
station's floor.
