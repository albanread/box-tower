# The car elevator

*A BOX addition, 25 October 2026.* The 1994 game reaches its underground
car parks only by ramps chained down from the first basement.  The car
elevator is a second way in: a lift for cars from the street straight down
to a basement floor, as some city office blocks have.  Cars come along the
street, wait their turn at the shaft, ride the platform down, drive along
the floor to a parking space and park; when the driver goes home the car
goes back the same way.

The car elevator is a BOX RULES feature (docs/emergency-services.md):
classic games are upstream's game, which has none.  With BOX RULES off,
`place_tile` refuses `carElevator` as upstream refuses any tile it does not
know ("Invalid tile type").  If BOX RULES are turned off in a tower that
has car elevators, they stand inert (no coverage, no traffic) until the
rules are turned on again; they can still be removed.

The code is `sim/src/box_ext/car_elevator.rs`; its tests are
`sim/tests/car_elevator.rs`.

## Building it

| | |
| --- | --- |
| tile type | `carElevator` |
| width | 8 cells (the art's car lift; `CAR_ELEVATOR_WIDTH`) |
| height | from the ground floor's row (the street) down to the basement floor chosen, every floor between included |
| cost | $100,000 for a car elevator to B1, and $25,000 for each floor below B1 |
| stars | 3, as for parking spaces and ramps |
| limit | 8 in a tower |

The command is the ordinary `place_tile`, with `tileType: "carElevator"`,
`x` the shaft's left column and `y` the row of the deepest basement floor
it is to reach (B1 is row 110, B2 row 111, ... B10 row 119).  The whole
shaft is built at once, from the street down.  A `y` above ground is
refused.

To take it deeper, place it again at the same `x` with a deeper `y`: the
new floors are added and $25,000 a floor is charged.  It cannot be made
shallower; remove it and build it again.

Every cell of the shaft must be empty or bare floor (floor is replaced).
It cannot go through the lobby or any room, or over another car elevator,
and no lift, stairs or escalator may be laid over it.  It needs no support:
it stands on its own.

The cells carry tile type `carElevator`, with the left column of each row
the anchor, as any other wide tile; the command's patch lists them, and the
parking spaces whose coverage changed.

As with parking, the stars are not checked by the simulation but by the
front end, through `get_tile_star_requirement("carElevator")`, which
answers 3.  It has no running cost.

## Joining it to the basement

A car elevator stands outside the building, under the street beside it:
it may not pass through the lobby.  Under BOX RULES the basement is dug
out to it.  An underground tile needs a built cell above it (upstream's
rule), or, under BOX RULES, a built cell beside it on its own row
(`side_support`, the hook in place_tile's support check).  So basement
floor is laid outwards from the building cell by cell, along B1, B2 and
so on, up to the shaft and beyond it, and parking spaces are placed on it
there as anywhere else.  Classic games keep upstream's rule.

Under BOX RULES bare basement floor is also a driveway: the coverage walk
(below) and the car elevator's own bays cross it as they cross parking
spaces, so a space is reached however much floor lies between it and the
landing.  A room on the row (a security office, a gym, the metro) still
stops the cars, as does a gap of more than three empty cells.

## The car park (BOX RULES)

- **A space stands for five cars** (`BAY_CARS`): it takes cars until five
  hold it.  It is drawn bare until a car parks in it, then with one car,
  nose out, the style and colour of the last to come.
- **In and out.**  A car drives along the floor side on to its space; there
  it is replaced by the same car seen from the front, so it seems to have
  reversed in.  When its driver is going it waits `GETTING_IN_TICKS` (its
  people come back to it), and drives out the way it came.
- **Ramps.**  A ramp stack (ramps from B1 down, each hanging from the one
  above) takes cars down a floor a ramp, the way alternating (B1's from its
  left end, at the street, to its right end; B2's back again), driven across
  as the car drops.  A car the car elevator brings to a floor goes on down
  any stack it reaches there, to spaces the platform does not serve; a
  stack with no car elevator is driven from the street, down and back up.
  `bay_routes`, `street_routes` and `path_in` plan the way; cars follow it.
- **Capacity.**  A car is sent only to a space with room; when every space
  a car elevator reaches is full, cars are turned away at the street
  ("Cars are being turned away at the car elevator"), as when the line is
  `LINE_MAX` long.  Found a space: no stress; none: 300 ticks of it and the
  complaint, as before.
- **Fees.**  Each car that comes to park pays `PARKING_FEE` ($20) for the
  day, taken at the day's end (`daily`, tallied in `parkingIncome`).
- **Running costs.**  Upstream charges a parking space every three days as
  it charges a lobby cell (the same family), by the star rating: $40,000 a
  space at four stars, which no fee could meet.  Under BOX RULES a space
  costs `SPACE_UPKEEP` ($100) a period, less than its five cars' fees, so a
  car park in use pays its way; lobby cells keep upstream's rate.  A car
  elevator costs `CAR_ELEVATOR_UPKEEP` ($10,000) a period to run, as a
  lift's car does, shown with the lifts (hooks in ledger.rs's
  do_expense_sweep: `space_upkeep`, `upkeep`).
- **Who drives.**  Upstream's drivers are a quarter of the office workers
  (by their office's place), but they drive only while their office's
  stay phase is 2, some days and not others.  Under BOX RULES they drive
  every working day from three stars, so the morning rush is the same each
  weekday.  The line in the street may grow to `LINE_MAX` (24) cars: a car
  lift serves one car at a time, and its drivers' waits are their stress,
  so a busy tower needs more than one (or ramps).
- **Commuters.**  An office worker whose car is given a space waits at the
  morning gate (`hold_commuter`; held from the sims' refresh) while the car
  drives in, and sets off when it has parked: from its floor, by whatever
  lift, stairs or escalator goes up from there, or from the lobby if none
  does (having walked up), or from the lobby after `COMMUTE_WAIT_MAX`
  ticks if the car never parks (`commute`, after the car tick).
- **People.**  One to four people get out of each car that parks and walk
  to the nearest lift, stairs or escalator on that floor; they walk back
  when it is going.  They are drawn traffic (movers.rs): the simulation's
  commuters still come in by the lobby.
- **Residents.**  Half the occupied condominiums (by a hash of their place)
  have a car.  It comes home between 17:00 and 20:00 (`RESIDENT_HOME_FROM`,
  a time of its own) and parks overnight if there is a space; if there is
  none, the household is given the 300 ticks of stress an office worker
  turned away is.  It goes out between 7:15 and 9:15 (`RESIDENT_OUT_FROM`),
  as the office workers' cars come in, so the same spaces serve both; at
  the weekend, when the offices are shut, it stays home.  It
  pays `PARKING_FEE` for the night like any car (`residents`, after the
  commuters; `Vehicle::resident`, `home`).  The Tower pane counts them
  ("Cars home").

## Removing it

`remove_tile` on any cell of it removes the whole car elevator, every car
using it and the platform.  A row of the shaft becomes floor where
something still needs it (tiles either side on the row, anything standing
on the street row, anything hanging below the bottom); otherwise it
becomes empty.  Nothing is refunded, as for other tiles.

## Parking coverage

On each basement floor it serves, the car elevator's landing is a parking
anchor exactly as a parking ramp is: the coverage rebuild walks left from
the cell to its left and right from the cell to its right, across parking
spaces (under BOX RULES bare basement floor too) and gaps of at most three
empty cells, stopping at anything else.
The spaces reached are covered (`coverageFlag` 1).  A car elevator to B2
therefore covers the spaces beside it on B1 and on B2 with no ramp at all.
The street row is not an anchor, and a ramp's walk stops at the shaft as at
any other object.

The spaces a car elevator reaches this way are its bays.

## When a car comes

Upstream has no moment when a person arrives by car.  We take it to be
the call of `try_assign_parking_service`: an office worker at the morning
gate who asks for parking (three stars, an occupied office, `(floor +
column) % 4 == 1`), or a hotel-suite guest in the evening.  What happens
then is BOX RULES' parking, below.

- the bay is chosen among the free bays by a hash of the driver's identity
  and the day number;
- the style (saloon, hatchback, estate, van, taxi, pickup) is chosen by a
  hash of the driver's identity, so a person's car looks the same every
  day;
- a person has at most one car at a time.

A parked car leaves when its driver is going or has gone home (the
driver's state is 0x05 or 0x45, departing; 0x24, hotel room vacated; 0x25,
0x26 or 0x27, night park or parked), when the driver no longer exists, or
at the latest one game day (2600 ticks) after it came.

## Under BOX RULES: parking that makes sense

BOX RULES change parking.  With them off, parking is upstream's,
unchanged (ramps only).

Upstream's parking is inverted: a covered space leaves the demand log and
`try_assign_parking_service` draws only from uncovered spaces, so the more
spaces a ramp or car elevator reaches, the more office workers complain
"Office workers demand Parking".  Under BOX RULES the same call means what
it says:

- a covered (reachable) space that no car holds serves the driver;
- a bay a ramp reaches is taken at once: the car is simply parked there
  (no traffic is drawn for ramps) and leaves when its driver does;
- otherwise the car elevator with the shortest street line that reaches a
  free bay takes the car (bays reached by several go to the first built);
- a car elevator whose line is already **8** cars long turns the car away;
- met: the driver's elapsed time is rebased from the clock as upstream's
  success does, and nobody complains;
- not met (no free reachable bay, or turned away): the driver takes **300**
  ticks of stress (`add_delay_to_current_sim`, the game's no-route delay),
  and the tower is told: "Office workers demand Parking" (the office
  caller's own message), "Hotel guests demand Parking", or "Cars are being
  turned away at the car elevator".

No random number is drawn (upstream's draw in the call is not made).

### Busyness

The platform carries one car and takes time (boarding, doors, a quarter of
a floor a tick), so at rush hour a line forms in the street.  Under BOX
RULES:

- each tick a car stands in the street line, or waits parked for the
  platform to take it out, adds a tick to its wait; a car beyond the
  **third** in the line (backed up past the street entrance) adds two;
- when the car boards, its wait becomes its driver's stress through
  `add_delay_to_current_sim` (capped at 300), as a passenger's wait for a
  lift does; the stress feeds the evaluation of the driver's office or room;
- a line longer than three cars is reported once a day: "Car elevator
  queue is backing up into the street" (`platforms[].backedUp` and `line`
  in the views);
- the platform keeps a tally, `waitTotal` and `rides`;
  `carElevatorInfo.averageWait` and `rides` show it.

A test (`rush_hour_queues_at_one_car_elevator_and_a_second_relieves_it`)
runs a tower of about 150 driving office workers for two days.  With one
car elevator to B3 the line reaches 8, eight cars are turned away and the
average wait is about 240 ticks.  With a second one the wait is about 130
ticks, nobody is turned away, and the office workers' stress is lower.

## The traffic

Cars come from the end of the street nearer the shaft (the left end if the
shaft's middle is left of the map's middle) and leave by the same end.  A
car is 6 cells long.  Positions are kept in sixteenths of a cell and of a
floor, so that everything is integer arithmetic and the same everywhere.

| Phase | What the car does | Speed |
| --- | --- | --- |
| `approaching` | drives along the street to its place in the line | 2 cells a tick |
| `queued` | waits in line; the first car 1 cell from the shaft, each next one a car and a cell further back | |
| `boarding` | drives onto the platform (from the line, or from its bay) and stops in its middle | 1 cell a tick |
| `riding` | rides the platform | a quarter of a floor a tick |
| `toBay` | drives off the platform along the basement floor to its bay | 1 cell a tick |
| `parked` | stands centred on its bay (never into the shaft) | |
| `awaitingLift` | parked, its driver gone, waiting for the platform | |
| `leaving` | drives off the platform at the street and away | 2 cells a tick |

The platform carries one car.  It serves the cars in the order they asked
for it (an arriving car asks when it appears, a leaving one when its driver
goes), and goes through these phases:

| Phase | |
| --- | --- |
| `idle` | doors shut, nothing to do |
| `moving` | to the floor where its next car is, or with a car aboard to its destination; doors shut |
| `opening` | 4 ticks |
| `open` | a car is driving on or off; it closes once the car aboard has driven clear of the shaft |
| `closing` | 4 ticks, then moves off with its car or goes idle |

So a car to B2 takes 1 tick a cell along the platform to board, 4 ticks
for the doors, 8 ticks down, 4 ticks for the doors and then its drive to
the bay: about 50 ticks from the head of the line to parked.

Order within a tick, after the carrier tick: cars whose bay has been
demolished are taken away (a demolished bay takes its car with it), parked
cars whose drivers have gone ask for the platform; then each platform
moves; then each car moves, in the order they came.

## What is saved

`world.boxExt`: `carElevators` (column, bottom floor, platform, the queue
of car ids, `waitTotal`, `rides` and `backedUpDay`), `vehicles` (with
`waited`, and `elevator` -1 for a car a ramp parked) and `nextVehicleId`,
beside `boxRules` and the rest of BOX RULES' state.  A classic tower saves
none of it.

## What the presenter reads

`TowerSim::box_ext()` gives the state; `box_ext::car_elevator::vehicle_views`
and `platform_views` turn it into what is drawn:

- each vehicle: `x` (left end, in cells, fractional), `floor` (floor index,
  ground 10, B1 9, fractional while riding), `row` (the same as a grid
  row), `length` (6), `facing` (-1 left, 1 right), `phase`, `lane` (1 for a
  car leaving along the street, 0 otherwise: the street has two lanes, and
  arriving cars queue in the near one), `style` (an art/src/vehicles name);
- each platform: `column`, `width`, `bottomFloor`, `floor` and `row`
  (fractional while moving), `phase`, `doorOpen` (0 shut .. 1 open, the
  landing doors at its floor) and `loaded`.

The shaft itself is the cells of tile type `carElevator` (the art draws
the street floor with the entrance and each basement with a landing).
A parked car may be drawn by its vehicle picture or left to the bay's own
picture (`parking.0..3`), as the presenter prefers.

`queryCell` on a car elevator's cell adds `carElevatorInfo`: column, width,
floors served, bottom floor, the platform, the queue's length, how many
cars use it, how many bays it reaches and how many are taken, and what one
more floor would cost.

## Hooks in the translated files

Each is marked `// BOX:` and does nothing with BOX RULES off (removal
excepted).

| File | Hook |
| --- | --- |
| `lib.rs` | `pub mod box_ext` |
| `world.rs` | `WorldState::box_ext`, skipped in JSON while empty |
| `snapshot.rs` | the field in `create_initial_snapshot` and `serialize_sim_state` |
| `commands.rs` | `handle_place_tile` calls `intercept_place`; `handle_remove_tile` calls `intercept_remove`; `append_parking_coverage_patches` made `pub(crate)` |
| `sims/parking.rs` | `rebuild_parking_coverage` adds `coverage_anchors`; `try_assign_parking_service` runs `assign_parking_box_rules` under BOX RULES |
| `tick/service_idle_tasks.rs` | `tick` after `carrier_tick` |
| `index.rs` | `QueryCellResult::car_elevator_info`; `TowerSim::box_ext()` |
| `resources.rs` | `get_tile_star_requirement("carElevator")` |

With BOX RULES off, every trace test passes and the lockstep harness finds
no difference.

## Not done yet

- Cars always come from the nearer street end.
