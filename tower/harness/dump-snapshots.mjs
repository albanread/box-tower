// Dump upstream's own saveState() snapshots for the Rust data-model check.
//
// Builds each trace fixture's tower exactly as upstream's trace.test.ts
// prepareFromTrace does, steps it, and writes sim.saveState() as JSON at
// several points.  sim/tests/snapshot_roundtrip.rs reads them back.
//
//   node harness/dump-snapshots.mjs [FIXTURE ...]
//
// TOWER_UPSTREAM  the tower-together checkout (default
//                 /Volumes/xc/BOX/src/tower-together, at 073c0e4)
// TOWER_SNAPSHOTS where the dumps go (default
//                 /Volumes/xc/BOX/caches/tower/snapshots); never in the repo.
//
// Upstream's TypeScript is bundled with the esbuild in its own node_modules.

import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const upstream = process.env.TOWER_UPSTREAM ?? "/Volumes/xc/BOX/src/tower-together";
const outDir = process.env.TOWER_SNAPSHOTS ?? "/Volumes/xc/BOX/caches/tower/snapshots";
const simDir = join(upstream, "apps/worker/src/sim");
const fixtureDir = join(simDir, "fixtures");

// ─── Bundle upstream's sim ──────────────────────────────────────────────────

const require = createRequire(join(upstream, "package.json"));
const esbuild = require("esbuild");
mkdirSync(outDir, { recursive: true });
const bundlePath = join(outDir, ".sim-bundle.mjs");
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
const UNDERGROUND_FLOORS = 10;
const { TowerSim, DAY_TICK_MAX, DAY_TICK_NEW_DAY, NEW_GAME_DAY_TICK, GROUND_Y } =
	await import(pathToFileURL(bundlePath).href);

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
	return (
		day0Length +
		(day - 1) * DAY_TICK_MAX +
		((tick - DAY_TICK_NEW_DAY + DAY_TICK_MAX) % DAY_TICK_MAX)
	);
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

// ─── Dumping ────────────────────────────────────────────────────────────────

let written = 0;
function dump(fixture, label, value) {
	const dir = join(outDir, fixture);
	mkdirSync(dir, { recursive: true });
	writeFileSync(join(dir, `${label}.json`), JSON.stringify(value));
	written++;
}

/** The live state, not saveState()'s copy: carries pending notifications and prompts. */
function live(sim) {
	return JSON.parse(JSON.stringify({ time: sim.time, world: sim.world, ledger: sim.ledger }));
}

function stats(snap) {
	const w = snap.world;
	const moving = w.sims.filter((s) => s.route.mode !== "idle").length;
	const pending = w.carriers.reduce((n, c) => n + c.pendingRoutes.length, 0);
	return `sims=${w.sims.length} routed=${moving} carriers=${w.carriers.length} pendingRoutes=${pending}`;
}

// Points to dump beyond the trace's own end where a trace is short: the
// state is upstream's whatever the binary would say.
const DUMP_POINTS = [
	[0, 2560],
	[0, 200],
	[0, 1200],
	[1, 600],
	[1, 1200],
	[2, 400],
	[2, 1000],
	[2, 1300],
	[2, 1800],
	[3, 1100],
];

function runTraceFixture(name) {
	const { spec, trace: rawTrace } = loadFixture(name);
	const trace = dropTerminalDuplicateDump(rawTrace);

	const fresh = TowerSim.create("trace-test", "Trace Test", "perfect-parity");
	dump(name, "00-create", fresh.saveState());

	// prepareFromTrace
	const sim0 = TowerSim.create("trace-test", "Trace Test", "perfect-parity");
	placeTilesFromSpec(sim0, spec);
	const snap = sim0.saveState();
	dump(name, "01-placed", snap);
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
	dump(name, "02-seeded", snap);
	const sim = TowerSim.fromSnapshot(snap);
	dump(name, "03-hydrated", sim.saveState());

	// Scheduled placements fire when the test reaches a matching trace entry.
	const simEntries = trace.slice(1);
	const scheduled = (spec.scheduled_facilities ?? [])
		.filter((b) => simEntries.some((e) => e.day === b.day && e.tick === b.tick))
		.map((b) => ({ at: traceTickToTotalTicks(b.day, b.tick), facilities: [...b.facilities] }));
	const last = simEntries.length > 0 ? simEntries[simEntries.length - 1] : trace[0];
	const points = DUMP_POINTS.map(([d, t]) => ({ at: traceTickToTotalTicks(d, t), label: `d${d}t${t}` }));
	points.push({ at: traceTickToTotalTicks(last.day, last.tick), label: `end-d${last.day}t${last.tick}` });
	points.sort((a, b) => a.at - b.at);

	let n = 4;
	const applyScheduled = () => {
		for (let i = scheduled.length - 1; i >= 0; i--) {
			if (scheduled[i].at === sim.simTime) {
				placeFacilityList(sim, scheduled[i].facilities);
				scheduled.splice(i, 1);
			}
		}
	};
	for (const p of points) {
		while (sim.simTime < p.at) {
			sim.step();
			applyScheduled();
		}
		const s = sim.saveState();
		const label = `${String(n++).padStart(2, "0")}-${p.label}`;
		dump(name, label, s);
		console.log(`${name} ${label} ${stats(s)}`);
	}
	dump(name, `${String(n++).padStart(2, "0")}-live`, live(sim));
}

function runFireFixture(name, accept) {
	const { spec, trace } = loadFixture(name);
	const sim0 = TowerSim.create(`trace-${name}`, "Trace Fire", "perfect-parity");
	placeTilesFromSpec(sim0, spec);
	const seed = trace[0];
	const fire = seed.fire;
	const snap = sim0.saveState();
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
	dump(name, "00-seeded", snap);
	const sim = TowerSim.fromSnapshot(snap);
	dump(name, "01-hydrated", sim.saveState());
	const respondAt = fire.fire_start_tick + 2;
	let responded = false;
	let n = 2;
	for (let i = 0; i < 2700; i++) {
		sim.step();
		if (!responded && sim.saveState().time.dayTick >= respondAt) {
			// The prompt is waiting in the live world: dump it before answering.
			dump(name, `${String(n++).padStart(2, "0")}-prompt-live`, live(sim));
			sim.submitCommand({ type: "prompt_response", promptId: `fire_${seed.day}`, accepted: accept });
			responded = true;
			dump(name, `${String(n++).padStart(2, "0")}-responded`, sim.saveState());
		}
		if (i === 20 || i === 200 || i === 1000 || i === 2699) {
			const s = sim.saveState();
			dump(name, `${String(n++).padStart(2, "0")}-step${i + 1}`, s);
			console.log(`${name} step${i + 1} helpers=${s.world.eventState.fireRescueHelpers.length} ${stats(s)}`);
		}
	}
	dump(name, `${String(n++).padStart(2, "0")}-live`, live(sim));
}

// Not a trace fixture: mixed_elevator's tower with parking, a parking ramp
// and a recycling center added underground, so that service_request
// sidecars, the parking demand log and a pending prompt appear in a dump.
function runExtras(name) {
	const { spec } = loadFixture("mixed_elevator");
	const sim = TowerSim.create("extras", "Extras", "perfect-parity");
	placeTilesFromSpec(sim, spec);
	sim.freeBuild = true;
	const results = [];
	const place = (x, floor, tileType) =>
		results.push([tileType, sim.submitCommand({ type: "place_tile", x, y: GROUND_Y - floor, tileType }).accepted]);
	for (const floor of [-1, -2, -3]) for (let x = 100; x < 200; x++) place(x, floor, "floor");
	place(100, -1, "parkingRamp");
	for (let x = 115; x < 160; x += 4) place(x, -1, "parking");
	// Beyond the ramp's reach (a gap): uncovered, so in the demand log.
	for (let x = 170; x < 190; x += 4) place(x, -1, "parking");
	place(120, -3, "recyclingCenter");
	// A second car on each carrier, so that one can be asked to go.
	for (const c of sim.saveState().world.carriers) {
		const floor = c.topServedFloor - UNDERGROUND_FLOORS;
		results.push(["car", sim.submitCommand({ type: "add_elevator_car", x: c.column, y: GROUND_Y - floor }).accepted]);
	}
	sim.freeBuild = false;
	const rejected = {};
	for (const [t, a] of results) if (!a) rejected[t] = (rejected[t] ?? 0) + 1;
	console.log(`${name} placed ${results.length}, rejected ${JSON.stringify(rejected)}`);
	const snap = sim.saveState();
	snap.world.starCount = 3;
	snap.world.eventState.disableNewsEvents = true;
	const s2 = TowerSim.fromSnapshot(snap);
	dump(name, "00-placed", s2.saveState());
	let n = 1;
	for (const [d, t] of [[0, 1200], [1, 600], [2, 1000]]) {
		while (s2.simTime < traceTickToTotalTicks(d, t)) s2.step();
		const s = s2.saveState();
		const kinds = [...new Set(s.world.sidecars.map((r) => r.kind))].join(",");
		dump(name, `${String(n++).padStart(2, "0")}-d${d}t${t}`, s);
		console.log(`${name} d${d}t${t} sidecars=${kinds} demandLog=${s.world.parkingDemandLog.length} ${stats(s)}`);
	}
	// Ask to remove a car from a busy carrier: the prompt waits in the world.
	for (let i = 0; i < 2000; i++) {
		const s = s2.saveState();
		const busy = s.world.carriers.find(
			(c) => c.cars.filter((car) => car.active).length > 1 && c.pendingRoutes.length > 0,
		);
		if (busy) {
			const r = s2.submitCommand({ type: "remove_elevator_car", x: busy.column, y: GROUND_Y - (busy.bottomServedFloor - 10) });
			const l = live(s2);
			console.log(`${name} remove car: ${JSON.stringify(r)} prompts=${l.world.pendingPrompts.length}`);
			dump(name, `${String(n++).padStart(2, "0")}-prompt-live`, l);
			break;
		}
		s2.step();
	}
}

const TRACE_FIXTURES = [
	"commercial",
	"condo",
	"condo_office_elevator",
	"condo_stuck",
	"dense_hotel",
	"dense_office",
	"elevator",
	"hotel",
	"lobby_only",
	"mixed",
	"mixed_elevator",
	"mixed_elevator_delayed",
	"mixed_multicar",
	"metro",
	"offices",
	"sky_office",
];
const FIRE_FIXTURES = { fire: false, fire_accept: true };

const wanted = process.argv.slice(2);
const all = [...TRACE_FIXTURES, ...Object.keys(FIRE_FIXTURES), "extras"];
for (const name of wanted.length ? wanted : all) {
	rmSync(join(outDir, name), { recursive: true, force: true });
	if (name in FIRE_FIXTURES) runFireFixture(name, FIRE_FIXTURES[name]);
	else if (TRACE_FIXTURES.includes(name)) runTraceFixture(name);
	else if (name === "extras") runExtras(name);
	else throw new Error(`unknown fixture ${name}`);
}
rmSync(bundlePath, { force: true });
console.log(`${written} snapshots in ${outDir}`);
