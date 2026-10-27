# Translating the simulation from TypeScript to Rust

The aim is a Rust simulation that does exactly what the TypeScript does:
the same state after every tick, for every input.  Not a better design, not
a tidier one.  Upstream's trace tests decide; if the Rust differs from the
TypeScript anywhere, the Rust is wrong.

The TypeScript is upstream's `apps/worker/src/sim` at commit `073c0e4`, in
the checkout at `/Volumes/xc/BOX/src/tower-together` (read it there; do not
copy it into this tree).

## Files and names

- One Rust file for each TypeScript file, at the same path under
  `sim/src/`: `carriers/advance.ts` is `sim/src/carriers/advance.rs`;
  hyphens become underscores (`route-scoring/score-carrier.ts` is
  `route_scoring/score_carrier.rs`).  A directory `foo/` with `foo.ts`
  beside it becomes `foo.rs` + `foo/` (Rust 2018 module layout), and
  `foo/index.ts` becomes `foo/mod.rs` only where there is no `foo.ts`.
  The crate's skeleton already has every file and every `mod` line; do not
  add or move modules.
- Functions keep their names in snake_case (`advanceCarrierCarState` is
  `advance_carrier_car_state`), types keep theirs (`CarrierRecord`),
  constants keep theirs (`DEPARTURE_SEQUENCE_TICKS`).  Struct fields are
  snake_case with `#[serde(rename_all = "camelCase")]` on the struct, so
  that a snapshot serialises with upstream's field names.
- Everything exported in TypeScript is `pub` in Rust; module-private
  helpers stay private.
- Keep upstream's comments (they carry the binary addresses and the
  reasoning); translate none of them away.  Head each file with
  `// ts: <path>.ts` and keep the order of definitions as upstream has it,
  so the two can be read side by side.

## Numbers

TypeScript numbers are doubles.  Almost all of this simulation's numbers
hold integers, and its arithmetic is integer arithmetic that never leaves
the range a double holds exactly, so:

- Integer-valued numbers are `i64` (fields, locals, parameters).  Use `i64`
  even where `i32` would do: then nothing overflows where JavaScript would
  not, and every value means one thing.  Indices are converted to `usize`
  only at the point of indexing.
- A number that can hold a fraction (a ratio, an average, the result of `/`
  that is not floored or truncated before it is stored) is `f64`.  Check
  each field's every use before deciding; a field that is ever assigned a
  fraction is `f64` everywhere.
- `/` between integers: `Math.floor(a / b)` is `js::floor_div(a, b)`,
  `Math.trunc(a / b)` is `js::trunc_div(a, b)`, `Math.round(x)` is
  `js::round(x)` (JavaScript rounds .5 towards +infinity).  A bare `a / b`
  stored or compared without flooring is f64 arithmetic: write it so.
- `x | 0` is `js::to_int32(x)`, `x >>> 0` is `js::to_uint32(x)`,
  `Math.imul(a, b)` is `js::imul(a, b)`; the shifts and `&`, `|`, `^` act on
  ToInt32 of their operands (`js::shl`, `js::sar`, `js::shr` for `<<`, `>>`,
  `>>>`).  `&` of two values already within 0..2^31 is plain `&`.
- `%` is Rust's `%` for integers (both truncate); for f64 use `%` too.
- `Number.isFinite`, `NaN`: if upstream relies on them, use f64 for that
  value.

## Missing values

- `arr[i]` where `i` may be out of range or negative gives `undefined` in
  JavaScript.  `arr[i] ?? 0` is `js::at_or(&arr, i, 0)` (takes an `i64`
  index).  Writing `arr[i] = v` out of range grows a JavaScript array: find
  out whether upstream ever does it before deciding (it should not).
- An optional field (`foo?: T`) is `Option<T>` with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`.  `x ?? d`
  is `x.unwrap_or(d)`; `x?.y` is `x.map(|x| x.y)` and so on.
- `undefined` and `null` both become `None` unless upstream tells them
  apart; if it does, say so in a comment and use an enum.

## Objects, maps, sets

- An interface is a struct, a union with a `kind` (or `mode`) tag is an
  enum with `#[serde(tag = "kind")]` (matching upstream's tag name).
- `Record<string, T>` (an object used as a map: `world.cells`,
  `world.placedObjects` ...) is `js::JsObject<T>`.  It keeps JavaScript's
  property order exactly: keys that are array indices ("0", "17") first in
  ascending order, then the other keys in insertion order; `delete`
  removes a key so that adding it again puts it last.  Iterating
  (`Object.entries`, `Object.keys`, `for ... in`) goes in that order.
  `Record<number, T>` is a `JsObject` too (its keys are strings in
  JavaScript).
- `Map<K, V>` is `indexmap::IndexMap<K, V>`: insertion order, and
  `map.delete(k)` is `shift_remove`, never `swap_remove`.  `Set<T>` is
  `indexmap::IndexSet<T>` the same way.
- Arrays are `Vec`.  `sort` is stable in JavaScript, as `sort_by` is in
  Rust: translate the comparator exactly (`a - b` is
  `a.cmp(&b)` for integers).  `splice`, `find`, `filter`, `findIndex`,
  `some`, `every` translate directly; keep the order of evaluation.
- `structuredClone` is `.clone()`.

## References and borrowing

JavaScript passes objects by reference and lets a function hold `world`
and `world.carriers[3]` at once.  Rust does not.  The rule:

- When a TypeScript function takes an object together with the container
  it lives in (a `car` and its `carrier`, a `carrier` and the `world`, a
  `sim` and the `world`), the Rust function takes the container and the
  object's index (or key) instead, and reaches the object through the
  container each time.  Name the index parameter after the object:
  `car_index`, `carrier_index`, `sim_index`.  If upstream already passes
  the index (as `advanceCarrierCarState` passes `carIndex`), drop the
  object parameter and use the index.
- When a function takes an object alone, it takes `&mut T` (or `&T` if it
  never writes it).  `WorldState`, `LedgerState`, `TimeState` are taken
  `&mut` / `&` as the function needs, in upstream's parameter order.
- A context object (`{ world, ledger, time }`) is `SimCtx` with those three
  fields, passed `&mut`.
- If upstream keeps a reference to an object across code that could move
  it (removing an element from the array it is in), say so in a comment;
  it has to be resolved by key, not position.
- Never `unsafe`, never `Rc<RefCell<>>`, never a global.

Where a call into another module is needed, call it by the name and
signature these rules give it.  If that module is not translated yet, the
skeleton has only its `// ts:` line: add the function there with the
signature the rules give and `todo!("ts: <path> <name>")` as its body, and
nothing else.  Its owner's translation replaces it.

## Randomness, time, tracing

- The simulation's only randomness is `sampleRng` on the world's LCG;
  translate it exactly (`js::imul`, `to_int32`, `>>> 16`).  `Math.random`
  and `Date.now` are not used by the simulation (check).
- Debugging shims (`__RNG_TRACE`, `__DRAIN_TRACE__`, `console.log`) are
  left out.

## Settled while translating the foundation

These cases came up in the first files (js, world, time, ledger, snapshot,
resources and the small tables) and are now the rule:

- A constant `Record` or `Map` (resources.ts's tables, the prologue table)
  is a `const` slice of pairs in JavaScript property order: numeric keys
  ascending, string keys as written.  `TABLE[key]` is
  `js::record_get(TABLE, key)` (an `Option`); a constant `Set` is a slice
  and `.contains`.
- A number array that upstream grows by writing past its end is a
  `Vec<js::JsSlot>`: a hole is `undefined` (`hole + x` is NaN), and JSON
  writes holes and NaN as null.  So far only `ledger.incomeLedger` (written
  at family codes by facility-refunds.ts and entertainment.ts); write it
  through `js::slot_mut(&mut v, i)`, with `.add_assign(x)` for `+=` and
  `.or(0)` for `?? 0`.
- An anonymous object returned (`{ time, incomeCheckpoint }`) is a tuple in
  the same order; an object-literal parameter (`RouteRequestRing.from(data)`)
  is its fields as parameters; an optional parameter with a default
  (`lobbyMode = DEFAULT_LOBBY_MODE`) is an `Option`.
- Code that reads saved JSON of any age (snapshot.ts's normalize and
  hydrate) works on `serde_json::Value` and deserialises at the end.
- Where a field's type is an inline literal union, the enum is named after
  the field: `RouteDirection`, `SimNotificationKind`, `SimPromptKind`.
- A constant another file needs before its owner is translated is added to
  the owner's file whole (a constant cannot be a `todo!`), as
  ENTITY_POPULATION_BY_TYPE was to sims/states.rs.

## Checking your work

- `cargo check` in `sim/` must pass for the files you own; errors in files
  you do not own are their owners'.
- `cargo test` runs unit tests; write a test where a function is subtle.
- Upstream's own unit tests (`*.test.ts`) may be translated beside the
  module as `#[cfg(test)]` tests where they test something you translated.
- The real check is the trace test and the lockstep harness (README.md).
