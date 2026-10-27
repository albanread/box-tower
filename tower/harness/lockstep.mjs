// The lockstep check: upstream's TypeScript simulation and the Rust
// translation run the same fixture side by side, and the first tick where
// they differ is named, with the JSON paths that differ and both values.
//
//   node harness/lockstep.mjs FIXTURE [--every N] [--max-ticks M] [--ticks T] [--limit K] [--ts-only]
//
// FIXTURE is a trace fixture's name (elevator, mixed_multicar, ...) or
// fire / fire_accept.  Both halves build the tower as upstream's
// trace.test.ts does: this file in TypeScript (the same code as
// harness/dump-snapshots.mjs), sim/src/bin/tower-lockstep.rs in Rust (the
// code of sim/tests/trace.rs).  Every tick the step's result
// (StepResult: cell patches, notifications, prompts) is compared, and
// every N ticks (default 1) the whole saveState() snapshot.  --limit K
// prints at most K differing paths (default 20).  --ts-only runs the
// TypeScript half alone and checks it against itself (records written to
// JSON and read back), to check the harness without the Rust.
//
// TOWER_UPSTREAM  the tower-together checkout (default
//                 /Volumes/xc/BOX/src/tower-together, at 073c0e4)
// TOWER_CACHE     where the bundle of upstream's sim is written (default
//                 /Volumes/xc/BOX/caches/tower); never in the repo.
//
// What each side does per tick (tower-lockstep.rs says the same):
// - a trace fixture runs to its last trace entry (after
//   dropTerminalDuplicateDump), placing each scheduled batch when the
//   simulation reaches the trace entry with the batch's day and tick, after
//   that tick's step, last batch first, as trace.test.ts does;
// - fire / fire_accept are seeded as their trace tests seed them and run
//   2700 steps, answering the fire prompt (declined / accepted) after the
//   first step whose dayTick reaches fireStartTick + 2.

import { execFileSync, spawn } from "node:child_process";
import { mkdirSync, readFileSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath, pathToFileURL } from "node:url";

const upstream = process.env.TOWER_UPSTREAM ?? "/Volumes/xc/BOX/src/tower-together";
const cacheDir = process.env.TOWER_CACHE ?? "/Volumes/xc/BOX/caches/tower";
const simDir = join(upstream, "apps/worker/src/sim");
const fixtureDir = join(simDir, "fixtures");
const rustDir = fileURLToPath(new URL("../sim", import.meta.url));

// ─── Arguments ──────────────────────────────────────────────────────────────

function usage() {
	console.error("usage: node harness/lockstep.mjs FIXTURE [--every N] [--max-ticks M] [--ticks T] [--limit K] [--ts-only]");
	process.exit(2);
}
let fixture;
let every = 1;
let maxTicks;
let runTicks;
let limit = 20;
let tsOnly = false;
{
	const argv = process.argv.slice(2);
	for (let i = 0; i < argv.length; i++) {
		const a = argv[i];
		const num = () => {
			const v = Number(argv[++i]);
			if (!Number.isInteger(v) || v <= 0) usage();
			return v;
		};
		if (a === "--every") every = num();
		else if (a === "--max-ticks") maxTicks = num();
		else if (a === "--ticks") runTicks = num();
		else if (a === "--limit") limit = num();
		else if (a === "--ts-only") tsOnly = true;
		else if (!a.startsWith("--") && fixture === undefined) fixture = a;
		else usage();
	}
	if (fixture === undefined) usage();
}

// ─── Bundle upstream's sim ──────────────────────────────────────────────────

const require = createRequire(join(upstream, "package.json"));
const esbuild = require("esbuild");
mkdirSync(cacheDir, { recursive: true });
const bundlePath = join(cacheDir, `.lockstep-bundle-${process.pid}.mjs`);
await esbuild.build({
	stdin: {
		contents: `export { TowerSim } from "./index";
export { DAY_TICK_MAX, DAY_TICK_NEW_DAY, NEW_GAME_DAY_TICK } from "./time";
export { GROUND_Y } from "./world";`,
		resolveDir: simDir,
		loader: "ts",
	},
	bundle: true,
	format: "esm",
	platform: "node",
	outfile: bundlePath,
	logLevel: "warning",
});
const { TowerSim, DAY_TICK_MAX, DAY_TICK_NEW_DAY, NEW_GAME_DAY_TICK, GROUND_Y } = await import(
	pathToFileURL(bundlePath).href
);
rmSync(bundlePath, { force: true });

// ─── trace.test.ts, verbatim in behaviour ───────────────────────────────────

const FIXTURE_TILE_MAP = {
	office: "office",
	condo: "condo",
	restaurant: "restaurant",
	"fast-food": "fastFood",
	retail: "retail",
	cinema: "cinema",
	"party-hall": "partyHall",
	single: "hotelSingle",
	twin: "hotelTwin",
	suite: "hotelSuite",
	stairs: "stairs",
	security: "security",
	housekeeping: "housekeeping",
	medical: "medical",
	metro: "metro",
	cathedral: "cathedral",
	lobby: "lobby",
};

function loadFixture(name) {
	const spec = JSON.parse(readFileSync(`${fixtureDir}/build_${name}.json`, "utf-8"));
	const trace = readFileSync(`${fixtureDir}/build_${name}.jsonl`, "utf-8")
		.trim()
		.split("\n")
		.map((line) => JSON.parse(line));
	return { spec, trace };
}

function withoutTime(entry) {
	const { day: _day, tick: _tick, ...rest } = entry;
	return rest;
}

function dropTerminalDuplicateDump(trace) {
	if (trace.length < 2) return trace;
	const prev = trace[trace.length - 2];
	const last = trace[trace.length - 1];
	if (JSON.stringify(withoutTime(prev)) !== JSON.stringify(withoutTime(last))) return trace;
	return trace.slice(0, -1);
}

function traceTickToTotalTicks(day, tick) {
	if (day === 0) return (tick - NEW_GAME_DAY_TICK + DAY_TICK_MAX) % DAY_TICK_MAX;
	const day0Length = DAY_TICK_MAX - NEW_GAME_DAY_TICK + DAY_TICK_NEW_DAY;
	return day0Length + (day - 1) * DAY_TICK_MAX + ((tick - DAY_TICK_NEW_DAY + DAY_TICK_MAX) % DAY_TICK_MAX);
}

function placeFacilityList(sim, facilities) {
	for (const fac of facilities) {
		if (fac.type === "elevator" || fac.type === "elevatorExpress" || fac.type === "elevatorService") {
			const bottom = fac.bottom ?? fac.floor ?? 0;
			const top = fac.top ?? fac.floor ?? 0;
			for (let f = bottom; f <= top; f++) {
				sim.submitCommand({ type: "place_tile", x: fac.left, y: GROUND_Y - f, tileType: fac.type });
			}
			const numCars = fac.cars ?? 1;
			const isExpress = fac.type === "elevatorExpress";
			let homeFloors;
			if (isExpress) {
				const validStops = [bottom];
				for (let f = bottom + 1; f <= top; f++) {
					const exe = f + 10;
					if (exe >= 11 && (exe - 10) % 15 === 14) validStops.push(f);
				}
				homeFloors = Array.from({ length: numCars - 1 }, (_, i) => validStops[(i + 1) % validStops.length]);
			} else {
				const span = top - bottom;
				homeFloors = Array.from({ length: numCars - 1 }, (_, i) =>
					numCars <= 1 ? bottom : bottom + Math.floor((span * (i + 1)) / (numCars - 1)),
				);
			}
			for (const homeFloor of homeFloors) {
				sim.submitCommand({ type: "add_elevator_car", x: fac.left, y: GROUND_Y - homeFloor });
			}
			continue;
		}
		const tileType = FIXTURE_TILE_MAP[fac.type];
		if (!tileType) throw new Error(`Unknown fixture tile type: ${fac.type}`);
		if (fac.floor === undefined) throw new Error(`Facility ${fac.type} missing 'floor'`);
		const y = GROUND_Y - fac.floor;
		if (tileType === "lobby" && fac.right !== undefined) {
			for (let x = fac.left; x < fac.right; x++) sim.submitCommand({ type: "place_tile", x, y, tileType });
			continue;
		}
		sim.submitCommand({ type: "place_tile", x: fac.left, y, tileType });
	}
}

function placeTilesFromSpec(sim, spec) {
	sim.freeBuild = true;
	const groundExtent = spec.floor_extent["0"];
	if (groundExtent) {
		for (let x = groundExtent.left; x < groundExtent.right; x++) {
			sim.submitCommand({ type: "place_tile", x, y: GROUND_Y, tileType: "lobby" });
		}
	}
	const floors = Object.keys(spec.floor_extent)
		.map(Number)
		.filter((f) => f !== 0)
		.sort((a, b) => a - b);
	for (const floor of floors) {
		const extent = spec.floor_extent[String(floor)];
		const y = GROUND_Y - floor;
		const tileType = floor % 15 === 14 ? "lobby" : "floor";
		for (let x = extent.left; x < extent.right; x++) sim.submitCommand({ type: "place_tile", x, y, tileType });
	}
	placeFacilityList(sim, spec.facilities);
	sim.freeBuild = false;
}

function computeRngState(seed, calls) {
	let state = seed;
	for (let i = 0; i < calls; i++) state = (Math.imul(state, 0x15a4e35) + 1) | 0;
	return state;
}

function prepareFromTrace(spec, trace) {
	const sim = TowerSim.create("trace-test", "Trace Test", "perfect-parity");
	placeTilesFromSpec(sim, spec);
	const snap = sim.saveState();
	snap.ledger.cashBalance = trace[0].cash;
	snap.world.starCount = trace[0].stars;
	snap.world.currentPopulation = trace[0].population;
	if (trace[0].rng_calls !== undefined) {
		snap.world.rngCallCount = trace[0].rng_calls;
		snap.world.rngState = computeRngState(1, trace[0].rng_calls);
	}
	snap.world.eventState.disableNewsEvents = true;
	for (const refCarrier of trace[0].carriers ?? []) {
		const carrier = snap.world.carriers.find((c) => c.column === refCarrier.column);
		if (!carrier) continue;
		for (let c = 0; c < refCarrier.cars.length && c < carrier.cars.length; c++) {
			const refCar = refCarrier.cars[c];
			const ourCar = carrier.cars[c];
			ourCar.currentFloor = refCar.currentFloor;
			ourCar.targetFloor = refCar.targetFloor;
			ourCar.prevFloor = refCar.prevFloor ?? refCar.currentFloor;
		}
	}
	return TowerSim.fromSnapshot(snap);
}

// The fire tests' seeding: build_fire_accept's test leaves weekendFlag,
// currentPopulation, rescueCountdown and helicopterExtinguishPos alone.
function seedFire(spec, seed, towerId, name, accept) {
	const fire = seed.fire;
	if (!fire) throw new Error("fixture must include fire state");
	const sim = TowerSim.create(towerId, name, "perfect-parity");
	placeTilesFromSpec(sim, spec);
	const snap = sim.saveState();
	snap.time.dayCounter = seed.day;
	snap.time.dayTick = seed.tick;
	snap.time.daypartIndex = seed.daypart;
	if (!accept) snap.time.weekendFlag = seed.day % 3 === 2 ? 1 : 0;
	snap.world.starCount = seed.stars;
	if (!accept) snap.world.currentPopulation = seed.population;
	snap.ledger.cashBalance = seed.cash;
	snap.world.eventState.disableNewsEvents = true;
	snap.world.eventState.gameStateFlags = fire.flags;
	snap.world.eventState.fireFloor = fire.fire_floor;
	snap.world.eventState.fireTile = fire.fire_tile;
	snap.world.eventState.fireStartTick = fire.fire_start_tick;
	if (!accept) {
		snap.world.eventState.rescueCountdown = fire.rescue_countdown;
		snap.world.eventState.helicopterExtinguishPos = fire.helicopter_extinguish_pos;
	}
	snap.world.eventState.fireLeftPos = snap.world.eventState.fireLeftPos.map(() => 0xffff);
	snap.world.eventState.fireRightPos = snap.world.eventState.fireRightPos.map(() => 0xffff);
	for (const [k, v] of Object.entries(fire.fire_left_pos)) snap.world.eventState.fireLeftPos[Number(k)] = Number(v);
	for (const [k, v] of Object.entries(fire.fire_right_pos)) snap.world.eventState.fireRightPos[Number(k)] = Number(v);
	return TowerSim.fromSnapshot(snap);
}

// ─── The TypeScript driver (tower-lockstep.rs, line for line) ───────────────

const FIRE_STEPS = 2700;

/** As JSON would carry it: holes and NaN to null, undefined dropped. */
const norm = (v) => JSON.parse(JSON.stringify(v));

function record(sim, step, snap) {
	const state = sim.saveState();
	return {
		t: sim.simTime,
		day: state.time.dayCounter,
		dayTick: state.time.dayTick,
		step: step === null ? null : norm(step),
		snap: snap ? norm(state) : null,
	};
}

/** Yields the records the Rust driver writes, one per tick. */
function* tsDriver() {
	if (fixture === "fire" || fixture === "fire_accept") {
		const accept = fixture === "fire_accept";
		const { spec, trace } = loadFixture(fixture);
		const seed = trace[0];
		const [towerId, name] = accept ? ["trace-fire-accept", "Trace Fire Accept"] : ["trace-fire", "Trace Fire"];
		const sim = seedFire(spec, seed, towerId, name, accept);
		const respondAt = seed.fire.fire_start_tick + 2;
		let responded = false;
		const steps = maxTicks === undefined ? FIRE_STEPS : Math.min(maxTicks, FIRE_STEPS);
		yield record(sim, null, true);
		for (let i = 1; i <= steps; i++) {
			const r = sim.step();
			if (!responded && sim.saveState().time.dayTick >= respondAt) {
				sim.submitCommand({ type: "prompt_response", promptId: `fire_${seed.day}`, accepted: accept });
				responded = true;
			}
			yield record(sim, r, i % every === 0 || i === steps);
		}
		return;
	}

	const { spec, trace: rawTrace } = loadFixture(fixture);
	const trace = dropTerminalDuplicateDump(rawTrace);
	const simEntries = trace.slice(1);
	const sim = prepareFromTrace(spec, trace);
	const scheduled = (spec.scheduled_facilities ?? [])
		.filter((b) => simEntries.some((e) => e.day === b.day && e.tick === b.tick))
		.map((b) => ({ at: traceTickToTotalTicks(b.day, b.tick), facilities: [...b.facilities] }));
	const applyScheduled = () => {
		for (let i = scheduled.length - 1; i >= 0; i--) {
			if (scheduled[i].at === sim.simTime) {
				const [batch] = scheduled.splice(i, 1);
				placeFacilityList(sim, batch.facilities);
			}
		}
	};
	const lastEntry = simEntries[simEntries.length - 1];
	let end = lastEntry === undefined ? sim.simTime : traceTickToTotalTicks(lastEntry.day, lastEntry.tick);
	if (maxTicks !== undefined) end = Math.min(end, sim.simTime + maxTicks);
	// --ticks T: exactly T steps, past the recording's end if need be
	if (runTicks !== undefined) end = sim.simTime + runTicks;

	applyScheduled();
	yield record(sim, null, true);
	let i = 0;
	while (sim.simTime < end) {
		const r = sim.step();
		i++;
		applyScheduled();
		const last = sim.simTime >= end;
		yield record(sim, r, i % every === 0 || last);
	}
}

// ─── Comparing ──────────────────────────────────────────────────────────────

// The snapshot's maps whose key order is the simulation's iteration order
// (JavaScript property order): a different order is a difference.
const ORDERED_MAPS = new Set([
	"$.world.cells",
	"$.world.cellToAnchor",
	"$.world.overlays",
	"$.world.overlayToAnchor",
	"$.world.placedObjects",
	"$.world.currentPopulationBuckets",
]);

function show(v) {
	const s = v === undefined ? "(absent)" : JSON.stringify(v);
	return s.length > 300 ? `${s.slice(0, 300)}… (${s.length} chars)` : s;
}

/** Collects up to `limit` differences between `ts` and `rs` into `out`. */
function diff(ts, rs, path, out) {
	if (out.length >= limit) return;
	const isObj = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
	if (Array.isArray(ts) && Array.isArray(rs)) {
		if (ts.length !== rs.length) out.push({ path: `${path}.length`, ts: ts.length, rs: rs.length });
		const n = Math.min(ts.length, rs.length);
		for (let i = 0; i < n && out.length < limit; i++) diff(ts[i], rs[i], `${path}[${i}]`, out);
		return;
	}
	if (isObj(ts) && isObj(rs)) {
		const kt = Object.keys(ts);
		const kr = Object.keys(rs);
		for (const k of kt) {
			if (out.length >= limit) return;
			if (!(k in rs)) out.push({ path: `${path}.${k}`, ts: ts[k], rs: undefined });
			else diff(ts[k], rs[k], `${path}.${k}`, out);
		}
		for (const k of kr) {
			if (out.length >= limit) return;
			if (!(k in ts)) out.push({ path: `${path}.${k}`, ts: undefined, rs: rs[k] });
		}
		if (ORDERED_MAPS.has(path) && out.length < limit && kt.length === kr.length) {
			const i = kt.findIndex((k, j) => k !== kr[j]);
			if (i >= 0) out.push({ path: `${path} (key order at ${i})`, ts: kt[i], rs: kr[i] });
		}
		return;
	}
	if (ts !== rs) out.push({ path, ts, rs });
}

// ─── Running ────────────────────────────────────────────────────────────────

if (tsOnly) {
	// Two TypeScript drivers in step, one through a JSON round trip: the
	// tower is built twice and must come out the same both times.
	const a = tsDriver();
	const b = tsDriver();
	let n = 0;
	for (let x = a.next(), y = b.next(); !x.done; x = a.next(), y = b.next()) {
		const out = [];
		diff(x.value, JSON.parse(JSON.stringify(y.value)), "$", out);
		if (out.length > 0) {
			console.log(`TypeScript differs from itself at simTime=${x.value.t}:`);
			for (const d of out) console.log(`  ${d.path}\n    a: ${show(d.ts)}\n    b: ${show(d.rs)}`);
			process.exit(1);
		}
		n++;
	}
	console.log(`${fixture}: the TypeScript half ran ${n - 1} ticks, deterministic`);
	process.exit(0);
}

console.error(`building tower-lockstep (cargo, release) in ${rustDir}`);
execFileSync("cargo", ["build", "--release", "--quiet", "--bin", "tower-lockstep"], { cwd: rustDir, stdio: "inherit" });
const rustArgs = [fixture, "--every", String(every)];
if (maxTicks !== undefined) rustArgs.push("--max-ticks", String(maxTicks));
if (runTicks !== undefined) rustArgs.push("--ticks", String(runTicks));
const child = spawn(join(rustDir, "target/release/tower-lockstep"), rustArgs, {
	stdio: ["ignore", "pipe", "inherit"],
	env: { ...process.env, TOWER_UPSTREAM: upstream },
});
const exited = new Promise((resolve) => child.on("exit", (code, signal) => resolve({ code, signal })));
const lines = createInterface({ input: child.stdout, crlfDelay: Number.POSITIVE_INFINITY });

const ts = tsDriver();
let ticks = 0;
let snapshots = 0;
let failed = false;
for await (const line of lines) {
	const rs = JSON.parse(line);
	const next = ts.next();
	if (next.done) {
		console.log(`DIFFERENT at simTime=${rs.t}: the Rust side ran on after the TypeScript side ended`);
		failed = true;
		break;
	}
	const t = next.value;
	const out = [];
	diff({ t: t.t, day: t.day, dayTick: t.dayTick }, { t: rs.t, day: rs.day, dayTick: rs.dayTick }, "$", out);
	diff(t.step, rs.step, "$step", out);
	if (t.snap !== null || rs.snap !== null) {
		diff(t.snap, rs.snap, "$", out);
		snapshots++;
	}
	if (out.length > 0) {
		const what = t.step === null ? "the built tower (before the first step)" : `the step to simTime=${t.t}`;
		console.log(`DIFFERENT after ${what}: day ${t.day}, dayTick ${t.dayTick} (Rust: simTime=${rs.t}, day ${rs.day}, dayTick ${rs.dayTick})`);
		for (const d of out) console.log(`  ${d.path}\n    ts:   ${show(d.ts)}\n    rust: ${show(d.rs)}`);
		if (out.length >= limit) console.log(`  (the first ${limit}; --limit K for more)`);
		failed = true;
		break;
	}
	if (t.step !== null) ticks++;
}
if (failed) {
	child.kill();
	process.exit(1);
}
const { code, signal } = await exited;
if (code !== 0) {
	console.log(`the Rust side stopped after ${ticks} ticks (exit ${code ?? signal}); its error is above`);
	process.exit(1);
}
if (!ts.next().done) {
	console.log(`DIFFERENT: the Rust side ended after ${ticks} ticks, the TypeScript side runs on`);
	process.exit(1);
}
console.log(`${fixture}: ${ticks} ticks in lockstep, ${snapshots} snapshots compared, no difference`);
