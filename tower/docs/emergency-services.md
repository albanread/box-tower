# BOX RULES and the emergency services

*A BOX addition, 25 October 2026.* The 1994 game sends a fire brigade
nobody sees and a bomb squad that is only a message.  We add medical
incidents, which happen only under BOX RULES, and we draw the fire engines,
the police and the ambulances in the street.

The code is `sim/src/box_ext/` (`mod.rs` for the switch and its random
generator, `emergency.rs` for the incidents, `street.rs` for the vehicles
in the street); the tests are `sim/tests/emergency.rs`.  The parking and
car-elevator rules that BOX RULES bring are in
[car-elevator.md](car-elevator.md).

## BOX RULES

BOX RULES is a world option, `world.boxExt.boxRules`.  Classic games are
upstream's game; BOX RULES are where our fork makes the logic make sense.

| Tower | BOX RULES |
| --- | --- |
| made for the BOX (`TowerSim::create_box`) | on |
| `TowerSim::create`, every trace fixture, the lockstep harness, a classic game | off |
| an upstream snapshot loaded | off (the field is absent) |

`TowerSim::set_box_rules(on)` turns them on or off; `box_rules()` says
which.  The snapshot holds `boxRules: true` only when they are on.

BOX RULES have their own random generator, `world.boxExt.rng`: a 32-bit
linear congruential generator (multiplier 1664525, increment 1013904223;
a draw is the top 24 bits), seeded from the tower's id when the rules are
first turned on, and saved with the tower.  The world's own generator is
never drawn for anything here.

What BOX RULES govern: medical incidents, and fire engines and police that
do their jobs (here); parking that makes sense, and the car elevator, which
can only be built under them ([car-elevator.md](car-elevator.md)).  With
BOX RULES off only the street's fire engines and police cars remain, as a
picture: runtime state, never saved, read by no rule.  Every trace test
passes and the lockstep harness finds no difference.

## Medical incidents

### Who collapses, and when

Every tick under BOX RULES, with chance *population* / 1,300,000, somebody
collapses: about *population* / 500 a day (two a day for a tower of
1,000).  The person is drawn from everybody the game has in the tower, all
alike, so incidents fall where people are:

- anybody riding a lift, queued for one, or on stairs or an escalator;
- anybody else not gone: not yet come (state 0x20), with no guest (0x24),
  or gone home (0x25, 0x26, 0x27);
- a condo resident only when the game has them on their own floor.

That takes in office workers, hotel guests, condo residents, shoppers and
diners (whose sims belong to their shop), cinema and party-hall guests,
and staff (housekeeping, security, recycling).

They collapse where they are:

| Where | The patient lies |
| --- | --- |
| standing still | at their unit (an office, a room, the shop they visit, their post); on a trip to a shop or restaurant (state 0x22), at that venue |
| queued for a lift | at that lift, on that floor; they leave its queue |
| aboard a car | at their stop: when the car delivers them, they collapse instead of going on (their arrival handler is not run) |
| on stairs or an escalator | on the floor they set out from |

At most three incidents are under way at once (riders due to collapse
included).  The tower is told: "Someone has collapsed: medics are on their
way".  The patient does nothing more (their refresh is held) until the
incident lets them go.

`TowerSim::box_trigger_incident(admission)` makes one happen now, and
`box_collapse_sim(index)` makes a given person collapse (both for tests and
for a scenario's script).

### Who responds

**A security guard first**, if the tower has a security office: from the
office nearest the patient by the route planner's estimate.  Guards know
the building: where nothing the player built reaches the patient, the
guard takes the fire stairs.  The guard goes to the patient and stays with
them until the medics or the staff arrive, then goes back to the office.

**A medical centre's staff**, if the tower has a centre with one of the
game's ten medical service slots free.  The centre is chosen as the game
chooses one for an office worker's medical trip (sims/medical.rs): one in
the patient's zone of fifteen floors if there is one, else any, at random
(BOX RULES' generator); among the centres the staff can reach by the
player's own transport, if any can.  The patient takes a slot, and the
centre's `pendingVisitorsCount`, as such a trip does.  Two staff go from
the centre to the patient, treat them, carry them back to the centre and
admit them; the staff go back in; the patient stays.

**Otherwise an ambulance**.  It drives along the street from the nearer
end, beacons on, to the middle of the ground-floor lobby (or of whatever is
built on the ground floor) and stops.  Two medics get out, go up to the
patient, treat them, carry them down and out, and the ambulance drives off
the way it came, beacons on.

**An outside admission** (only with a medical centre): every tick has
chance (medical centres) / 2,600 of one, about one a day for each centre.
The ambulance comes, two medics carry the patient in and up to the centre,
admit them, and go back down and away.

### How they get there

Medics, staff, guards and the carried patient ride as passengers, but they
are crews who know the building.  The route planner finds the quickest way
by the tower's own lifts, stairs and escalators:

- a lift leg is counted as 120 ticks plus 4 a floor; stairs 30 ticks a
  floor; an escalator 15; walking half a cell a tick;
- they use lifts of every kind, service lifts included, and change from
  one lift to another on any floor where both stop (ordinary passengers
  keep the game's rules: they change only at lobbies);
- stairs and escalators may be taken anywhere;
- each of them is a request in the carrier's queue for that floor and
  direction (route id `box:er:PERSON:N`), so they wait behind whoever is
  already queued and take a place in a car (its `assignmentCapacity`) like
  anyone else: at rush hour they wait for the crowd.  The team goes on
  when all of them have been delivered;
- a request that is lost (the end-of-day flush empties the queues; a
  timed-out office worker pops those queued ahead of them) is queued
  again; a lift that is removed or stops serving a floor makes them plan
  again from where they are.

**If the patient truly cannot be reached** (no lift, stairs or escalator
leads there from where the responders start), the incident still happens.
The responders take the planned route to the reachable floor nearest the
patient, then the building's fire stairs the rest of the way at **120
ticks a floor**, four times slower than stairs the player built.  The slow
response counts (below).  The way back is planned the same way, and a
guard comes the same way.

### Timings

| | |
| --- | --- |
| walking | half a cell a tick |
| ambulance | 2 cells a tick on the street; 7 cells long |
| treatment where the patient lies | 40 ticks |
| admitted | 800 ticks, then discharged |

### What it costs and earns

**Response time**: from the collapse to the medics' or staff's arrival.
It counts against the tower as a slow lift does: every other person of the
patient's unit (the office's other workers, the room's other guests, the
condo's household, the shop's other customers) takes it as stress through
`add_delay_to_current_sim`, the game's path for a waiting delay, capped as
that path caps it at 300.  That stress feeds the unit's evaluation like any
other.  If a guard reached the patient first, the wait counts in full up to
the guard's arrival and at half from then on.  Each incident keeps its
`response` and `stress`; `boxExt.emergency.responseTicks` is the running
total and `collapses`, `guards` the counts.

**The patient** goes away as the game sends people away, once carried out
by ambulance or discharged from the centre:

| Who | Then |
| --- | --- |
| an office worker | home for the day (state 0x26, night park; back the next morning) |
| a hotel guest | checks out (state 0x04, the checkout queue, which pays for the stay as usual) |
| a shopper or diner | home (state 0x27) |
| anybody else (a condo resident, staff, a cinema or party guest) | back to where they belong, in the state they were in when they collapsed |

**Admissions**: each admission to a medical centre pays the tower
**$3,000**, collapses in the tower and outside patients alike.
`boxExt.emergency.admissions` is the count.  If the centre is demolished
while a patient is on the way, the patient is admitted nowhere, is not
paid for and is discharged at once.

## Service vehicles in the street

`TowerSim::box_views().serviceVehicles` lists the vehicles to draw in the
street, BOX RULES on or off:

| When | Vehicles |
| --- | --- |
| a fire (event flag bit 3) | a `fire-pumper` parked 11 cells left of the fire's column and a ladder truck at it.  While parked, if the fire's floor is 1 to 3 floors above the street, the truck is `fire-ladder-deployed` with `ladderTo` the fire's floor (draw the raised ladder, art/src/vehicles/NOTES.md); otherwise `fire-ladder`, stowed |
| a bomb search (event flag bit 0) | two `police` cars either side of the bomb's column |
| BOX RULES' ambulances | `ambulance` |

Each drives in along the street from the nearer end at 2 cells a tick,
parks, and when the event is over drives away the way it came; beacons
are on throughout.  Each has `style` (an art/src/vehicles type), `x` (left
end, cells), `floor` (10, the street), `row`, `length`, `facing`, `beacon`,
`moving` and `ladderTo`.

With BOX RULES off the fire engines and police cars are a picture only:
their positions are runtime state, never saved, and no rule reads them (a
classic tower loaded during a fire sees its engines drive in again).
Under BOX RULES they are saved (`boxExt.street`), so a loaded tower goes on
exactly, and they work:

### Fire engines

Once both engines are parked:

- on the floors their ladder reaches (1 to 3 floors above the street) the
  fire's fronts spread at **half speed** (every other spread step is held);
- every **40 ticks** of their work they put out the lowest burning floor
  within reach (both its fronts);
- the game's helicopter rule stays for higher floors.

Under BOX RULES, also: a fire climbs to the next floor only from a floor
that is still burning (upstream's fire climbs on its timetable even past a
floor that has been put out), and a fire with nothing left burning is out,
however it was put out (the engines, the helicopter, the security
firefighters, or burnt out).  The day simply carries on: upstream jumps the
clock to tick 1500 when a fire ends early, and a classic game still does;
under BOX RULES no fire's end moves the clock.

A test lights a fire on the first floor up: with BOX RULES off it burns
until the game's fixed cleanup at tick 2000, about 1,770 ticks; with them
on, the engines arrive, raise the ladder and have it out in about 110.

### Police

Once a police car is parked, the police search for the bomb: a search step
a tick of their own (the game's own search, floor by floor, tile by tile),
beside any security guards' step.  If nobody was searching (the tower has
no security office), they start at the bomb's floor.  So a tower without
security can still have its bomb found, and one with security finds it
twice as fast.

## What else the presenter reads

`TowerSim::box_views()` also gives `people`: each medic, member of staff,
guard and patient, with `family` (the figure to draw: "security" for a
guard, "medical" for the others), `role` (`medic`, `staff`, `guard`,
`patient`), `x` (cells), `floor` (the car's floor while aboard a lift),
`row`, `facing`, `phase` (`walking`, `standing`, `lift`, `stairs`,
`treating`, `lying`, `carried`, `admitted`) and `inLift` (aboard a car,
rather than queued for one).  Medics still in the ambulance are not
listed.  `vehicles` and `platforms` are the car elevator's.

## Hooks in the translated files

Each is marked `// BOX:`.

| File | Hook | Off |
| --- | --- | --- |
| `tick/service_idle_tasks.rs` | `emergency::tick`, then the car elevator's, then `street::tick` | the first two return at once; the street keeps only its unsaved picture |
| `events.rs` `tick_fire_event` | `street::fire_out`: engines' knock-down; a fire with nothing burning is out | false at once |
| `events.rs` `resolve_fire_event` | no jump to tick 1500 under BOX RULES | upstream's jump |
| `events.rs` `advance_fire_spread` | `fire_ignition_blocked` (no climbing from a floor not burning); `fire_spread_held` (half speed within the engines' reach) | false at once |
| `events.rs` `tick_bomb_event` | `police_search`: the police start or step the search | nothing |
| `queue/dispatch_arrivals.rs` | a rider due to collapse does so at their stop (`collapse_on_arrival`); a delivered rider who is no sim goes to `on_carrier_arrival` | false at once with nobody due; ids not starting `box:er:` return at once |
| `sims/mod.rs` `advance_sim_refresh_stride` | a held patient is skipped (`is_held`) | false at once with nobody held |
| `sims/parking.rs` `try_assign_parking_service` | BOX RULES' parking (car-elevator.md) | upstream's body |
| `index.rs` | `create_box`, `set_box_rules`, `box_rules`, `box_trigger_incident`, `box_collapse_sim`, `box_views` | |

## Not done yet

- The fire engines do not drive up to fires above their ladder's reach to
  any effect; the helicopter remains the answer there.
