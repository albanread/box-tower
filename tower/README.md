# TOWER — a tower-building simulation, for the BOX

*Started 25 October 2026.* A port of **tower-together** (Patrick Hulin,
MIT), a clean-room reimplementation of the 1994 tower simulation by Yoot
Saito, to run on the BOX as a ROSAPPS game with an interface of our own.
The original game's name belongs to its publishers; it is not ours to use
and this game does not carry it.

## Where it comes from

| What | From |
| --- | --- |
| the simulation we translate | github.com/phulin/tower-together, `apps/worker/src/sim` (TypeScript, about 21,000 lines), at commit `073c0e4` |
| the trace fixtures it is checked against | the same commit, `apps/worker/src/sim/fixtures` (287 MB: not copied here; see Checking) |
| `UPSTREAM-LICENSE.md` | the same commit's `LICENSE.md`, unchanged |

tower-together was made by having the original Windows 3.1 program
reverse-engineered into a specification and the simulation rebuilt from it;
its trace tests replay games recorded from the original program and compare
the whole state on every tick. It ships none of the original's code or art.
Yoot Saito's own source (github.com/YootTowerManagement/YootTower) is not
published and is not used.

## The plan

1. **sim/** -- the simulation translated to Rust, file for file
   (TRANSLATION.md has the rules), until all of upstream's trace tests pass
   in Rust as they do in TypeScript.
2. An engine program (ports/rosapps/tower-engine) over it, as Micropolis's
   is: the tower drawn by the GPU, the tools in the palette, the details in
   the properties pane.
3. The front end, !Tower (a name to be chosen), with our own art.

Where it stands (25 October 2026): ports/rosapps/tower-engine publishes
the tower surface (docs/graphics-design.md, units 4 and 5 done), the Mac's
QEMU draws its field in Metal (unit 7: classic, equal to the twin), and
ports/rosapps/shell/!Tower is a playable front end: a palette of seventeen
tools covering every room the simulation has (each tool's kinds, such as
the three hotel rooms or the three shafts, chosen on the properties pane,
which says what a tool costs, needs, gives and why it is refused); the
pane's pages Tower (the time, funds, stars and what the next star needs,
the news, each item a button to where it happened), Room (the room
inspected: state, rent, evaluation, people, stress), Finances (the
ledger's income and expenses for the period), Evaluation (the rating
view, rooms by rating, the worst ones) and Options; the game's questions
(fire, bomb threat, a car taken from a busy shaft) in a dialogue box,
answered back to the simulation; and saving and loading.  ports/rosapps/shell/build.sh builds it, packs the art into it
(art/out/4, then towerpack) and copies four fixture towers into it from
TOWER_SNAPSHOTS.  The parity check is tower-surface's twincheck example.

## Towers on disc

A tower is saved as the simulation's snapshot (`SimSnapshot`, JSON), file
type **&A9D, Tower**.  The type is not in ROOL's allocation list (nor in
the BOX's typemap or MimeMap, where &F79 is CSS); it was chosen next to
the BOX's other games' and programs' types (&A9A FloorPln, &A9B Publish,
&A9C Micropol).  !Tower's !Boot sets `File$Type_A9D` and the run type, so
a tower double-clicked opens in !Tower; the fixture towers in
!Tower.Towers carry it too.

For the play tests, a file `event.request` beside the engine holding
`fire` or `bomb` starts that event at once (`TowerSim::box_test_event`,
without the game's guards of day, hour, stars and height).

## Checking

The upstream checkout lives beside the BOX's sources (not in this tree):

    git clone https://github.com/phulin/tower-together /Volumes/xc/BOX/src/tower-together
    git -C /Volumes/xc/BOX/src/tower-together checkout 073c0e4

Upstream's own test, as the baseline (19 traces, about 17 s):

    cd /Volumes/xc/BOX/src/tower-together && npm install --ignore-scripts
    cd apps/worker && npx vitest run src/sim/trace.test.ts --testTimeout=600000

The data model's check: upstream's own simulation builds each fixture's
tower as its trace test does and writes `saveState()` at a dozen points
(about 280 snapshots, outside the tree in TOWER_SNAPSHOTS, default
/Volumes/xc/BOX/caches/tower/snapshots); `sim/tests/snapshot_roundtrip.rs`
reads each into `SimSnapshot` and writes it back, and the JSON must come
back the same:

    node harness/dump-snapshots.mjs [FIXTURE ...]
    cd sim && cargo test --release --test snapshot_roundtrip -- --nocapture

The Rust translation's (TOWER_UPSTREAM names the checkout):

    cd sim && cargo test --release

(`sim/tests/trace.rs` is upstream's trace.test.ts; its tests, and
commands.rs's translation of commands.test.ts, are `#[ignore]`d while
other files are still `todo!`: `cargo test --release -- --include-ignored`
runs them.)

and the lockstep check, which runs the TypeScript and the Rust side by side
and names the first tick and field where they differ:

    node harness/lockstep.mjs FIXTURE [--every N] [--max-ticks M]

(`--ts-only` runs the TypeScript half against itself, to check the harness.)
