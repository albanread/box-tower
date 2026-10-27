# BOX Tower

A port of **tower-together**, the open reimplementation of the 1994
tower-building simulation, to the BOX: a RISC OS application environment
that runs on Linux.  In the BOX the game is a RISC OS desktop program.  Its
window, menus, tool palette and properties pane are an ordinary RISC OS
task, the simulation runs beside it as a Linux program, and the tower is
drawn on the GPU.

The port targets the BOX and no other system.  This repository is published
so that the port's source is available under the licence of the work it
comes from.  It is not complete and is not meant to be built on its own:
the rest of the BOX (the RISC OS desktop, the libraries the front end and
the engine are built with, and the presenters that draw the tower) is not
part of it.

## Where it comes from

The simulation is translated from **[phulin/tower-together](https://github.com/phulin/tower-together)
at commit `073c0e4`**, by Patrick Hulin, under the MIT licence.
tower-together rebuilt the original game's simulation from a specification
and checks it, tick by tick, against games recorded from the original
program; it contains none of the original's code or art.  Its own files are
not copied here; they are at the commit above.

- `tower/sim` is upstream's `apps/worker/src/sim` (TypeScript) translated to
  Rust, file for file (`tower/TRANSLATION.md` has the rules).  With the BOX's
  rules off it plays exactly as upstream does: upstream's nineteen trace
  tests pass against it, tick for tick.
- `tower/art` holds the pictures.  Those derived from upstream's
  `apps/client/public/rooms` say so in a comment at their top and are listed
  in `tower/art/PROVENANCE.md`; the rest were drawn for this port.
- `tower/UPSTREAM-LICENSE.md` is upstream's licence, unchanged.

This game is not made, endorsed or supported by the original game's makers
or publishers, and it does not use the original game's name.

## What the BOX added

**BOX RULES** (`tower/sim/src/box_ext`), a set of rules a tower made in the
BOX plays by, so that the game makes sense where the original's rules do
not; each is described in `tower/docs`:

- a car elevator and a car park in the basement: spaces, ramps, residents'
  and commuters' cars, parking fees and running costs;
- medical incidents, ambulances and medics, police at a bomb threat, fire
  engines and fire escapes;
- a gentle fire: everyone walks out by the escapes and comes back; a
  sprinkler room;
- amenities: laundry, guest services, premier lounge, pool, gym, convention
  centre, kindergarten, primary school;
- who lives and works in the tower, and people who work where they live;
- metro commuters; hotels that fill by their appeal;
- routes that weigh a crowded lift against the stairs, and the walk to the
  lift counted in a trip's stress; each room's rating set out step by step.

Each hook into the translated files is marked `BOX:` in the source.

**The program** the BOX runs:

| Path | What it is |
|---|---|
| `rosapps/tower-engine` | the engine: Rust over the simulation.  It talks to its RISC OS window over a socket (the tools, the panes, the news, the game's questions) and publishes the tower, the people and the vehicles for the GPU through shared memory |
| `tower/surface` | the layout of what the engine publishes, and a CPU twin of the GPU drawing used to check it |
| `rosapps/shell/!Tower` | the RISC OS front end (Mojo) |
| `tower/art` | the generators and sources of the pictures |
| `tower/harness` | the harness that runs upstream's TypeScript and the Rust side by side |

## Licence

Everything here is under the MIT licence, as upstream is: see
[LICENSE](LICENSE), which carries upstream's copyright notice and
ours, and [NOTICE](NOTICE).
