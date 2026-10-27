# The tower on the GPU -- graphics design

*25 October 2026. A design, not yet code. It covers how the presenters
draw the tower's floors and rooms, its sky and ground, its elevators and
its people, on the PC (GLSL ES 3.00, rosgd/ports/present/rosgd-present.c)
and on the Mac (Metal, qemu/ui/metal.m). Both shader sets are part of the
design and carry equal weight.*

Contents

1. Summary of the decisions
2. What has to be drawn
3. Why a new surface kind, and what is reused
4. Units, coordinates and zoom
5. The data: the live block, the art block, the surface entry
6. Drawing a room: width, height, variants and states
7. The passes and their shaders (GLSL and Metal, each pass in full)
8. Keeping the GL and Metal shaders in step
9. Performance budget
10. What the art must provide
11. Implementation plan
12. Open questions

---------------------------------------------------------------------------

## 1. Summary of the decisions

1. **A new surface kind, ROS_SURF_TOWER (7), not version 4 of the tile
   map.** The tile map's block assumes square tiles, a map of at most 128
   tiles a side, one picture a tile and at most 256 sprites drawn one
   call each. The tower has 375 x 120 cells four times taller than wide,
   rooms up to 31 cells wide and five floors tall, per-room state, and
   several thousand people. The tile map's *machinery* is kept whole:
   the seqlock, the split between a live part and an art part, the
   broker copying the engine's block into a backing, the bind with the
   frame moved on, the quality numbers (classic, smooth, rich), the
   `rate`, the outline, the safe-program fallback and the screenshot
   path.
2. **The room is the unit of drawing, not the cell.** A cell map (one
   texel a cell) names the room covering each cell; a room table (one
   texel a room) says where the room is, which picture it shows, and its
   state. The fragment shader finds the room from the cell, its picture
   from the room, and samples the picture stretched over the room's
   width and height. A room of any width or height costs the same.
3. **The engine picks pictures; the shader picks light.** Discrete
   states that change the picture (a dirty hotel room, a parking bay's
   car, a recycling centre's fill, no access) are atlas entries the
   engine chooses. Everything continuous or decorative (night, lamps on
   and off per room, occupancy shown as people in seats, for-rent
   banners, the greyed stuck room, cockroaches, evaluation colours,
   fire's flicker, glass, shadows) is worked out in the shader from a
   handful of generic state bits. The shaders know no room types.
4. **Two blocks.** A small live block (about 700 KB, most of it rarely
   changing) and a large art block (about 36 MB: the atlases), each in a
   backing of its own, both named in the surface's 32 bytes. The art is
   copied and uploaded once; a frame copies and uploads only the sections
   whose generation moved (typically 30 to 100 KB).
5. **Three passes, all drawn per visible rectangle.** P1, the *field*: one
   quad, everything that lies on the grid (sky, skyline, ground, building
   pieces, rooms, fire, overlays). P2, *fixtures*: instanced quads for
   elevator shafts, stairs and escalators. P3, *movers*: instanced quads
   for cars, people, clouds, sun and moon, the helicopter and other
   events. Instance data travels in textures, in both APIs, so that the
   shader text can be the same.
6. **One shader body, two preludes.** Each pass is written once in a
   common subset; a small generator wraps it in a GLSL prelude and a
   Metal prelude and embeds the results in rosgd-present.c and metal.m.
   A CPU twin in Rust is the reference both are compared against.
7. **Everything is a texture; every lookup is an integer fetch except the
   pictures.** Integer textures (R16UI, RGBA16UI, RGBA32UI, RG8UI,
   R8UI) carry the tables; RGBA8 textures with mipmaps carry the
   pictures, sampled at a level of detail fixed by the zoom (a uniform),
   never by derivatives, so no seams appear where a quad of pixels
   straddles two rooms.
8. **Units follow the art tree.** A cell is 4 units wide and a floor 16
   units tall (art/README.md). 1:1 zoom is one desktop pixel a unit (a
   cell 4 x 16 pixels, as tower-together's zoom 1); 4x is four pixels a
   unit. The atlas is rendered at 4 texels a unit, so 4x shows it texel
   for texel and 1:1 reads mip level 2.

---------------------------------------------------------------------------

## 2. What has to be drawn

From upstream's Phaser renderer (apps/client/src/game/GameScene.ts) and the
simulation's records (sim/src/world.rs, resources.rs):

| Thing | Upstream draws it as | Data it comes from |
| --- | --- | --- |
| Sky | two vertical gradients (day `#1a3a6e`/`#5ba8d4`/`#b4ddf0`, night `#04091a`/`#0a1235`/`#0e1f4a`), the night one faded in 20:00-21:00, out 04:00-06:00 (`nightAlpha`) | `time.day_tick` through `dayTickToHour` |
| Clouds | drifting sprites (clouds.ts) | none: decoration |
| Underground | `underground2.webp` tiled across the ten lowest floors | rows 110-119 |
| Rooms | one SVG stretched over the room; a variant chosen by a hash of x, y; hotel rooms have a dirty variant; cinema, party hall, recycling 2 floors, metro 3 | `cells`, `placed_objects` |
| Lobby and plain floor | runs of merged cells; the lobby a repeating 8-cell strip | `cells` |
| Gaps | one world pixel between rooms and below each floor shows what is behind | -- |
| For rent / for sale | a banner centred on the room, aspect 9:4 | `unitStatus` (office > 0x0f, condo > 0x17, others `occupiedFlag == 0`) |
| Stuck room | the room greyed, brightness 0.85 | hotels `unitStatus >= 0x38`; office and condo `occupiedFlag == 0` |
| Cockroaches | 3, 4 or 6 animated bugs wandering the room | hotels `unitStatus >= 0x38` |
| Evaluation | red/yellow/blue at alpha 0.5 over the room while paused | `evalLevel` 0/1/2, with an occupied-but-unscored fallback to blue |
| Parking | variants for full levels, a no-access picture | `coverageFlag` |
| Elevator shafts | outlined rectangles | `overlays`, `carriers` |
| Cars | yellow rectangles with an occupancy number, interpolated between snapshots | `carriers[].cars[]` |
| Stairs, escalators | an SVG over the floor and a third of the floor above | `overlays` |
| People | only those queued at elevators, coloured by trip stress | `sims` |
| Fire | translucent red between the fire fronts on each floor | `event_state.fire_left_pos/right_pos` |

The port's art tree (rosgd/ports/tower/art, in progress) already goes
further: building pieces (slab, floor, walls, roof, basement, construction
frames, rubble), world pieces (soil, rocks, street and its furniture, two
skyline strips, sun, moon, stars, clouds), events (fire left/centre/right
in six frames, smoke, burnt, bomb, explosion, firefighters, helicopter,
fireworks, VIP, guests), and transport pieces (shaft body, top, pit,
landings with four door frames and a "skip" for floors not served, cars
with four door frames and four load levels, stairs, an escalator in four
frames), each with an optional `.emit.svg` layer of what glows at night.
This design draws all of it.

---------------------------------------------------------------------------

## 3. Why a new surface kind, and what is reused

### 3.1 Not tiles v4

| tiles.h v3 | the tower |
| --- | --- |
| map of at most 128 x 128, fixed in the layout and in three readers (broker, PC, Mac) | 375 x 120 |
| square tiles | cells 1:4 |
| one atlas tile a map tile | rooms up to 31 x 5 cells from one picture |
| per-tile class word | per-room state |
| no minification (the art is drawn at or above its size) | 1:1 reads the art at a quarter of its size: mipmaps needed |
| 256 sprites, one draw call each on the PC | about 3,000 to 8,000 movers: instancing needed |
| one block, art inside it | the art (36 MB) wants its own backing |

Bending tiles.h to all of that would change every field the three readers
use. A new kind leaves !Micropolis untouched.

### 3.2 Not the game pane

ROS_SURF_GAMEPANE (gamepane.h, design 50) is a fixed 640 x 360 retro canvas
drawn by the Mac host; on the PC the game pane renders itself as a Wayland
client. Neither suits a scrolling, zooming 1500 x 1920-unit picture inside a
Wimp window.

### 3.3 What is reused, as it is

- The broker's model (modules/rosapps/rosapps.c, `ROSApps_Tiles`): the
  engine writes into its shared buffer, says a word when a frame is whole,
  the broker copies under the seqlock into a backing and binds the icon
  with Wimp_Extend &5204, frame moved on; the engine is told "shown".
- The seqlock rule: `frame` odd while written; a reader that sees odd, or
  a different value either side of its copy, draws the last whole frame.
- The art generation (`art`): pictures uploaded only when it moves.
- Quality: `ROS_TILES_Q_CLASSIC` 0, `_SMOOTH` 1, `_RICH` 2, and the `rate`
  at which a rich drawing is redrawn without a new frame.
- Nothing trusted: every count clamped, every index masked, on the
  presenter's side.
- `safe_program`: a rich program that will not build falls back to the
  classic one; a pass that will not build is left out, never fatal.
- The Mac's `shot` pipelines and the PC's `screenshot_to` (and its scratch
  mode) for the parity tests.

---------------------------------------------------------------------------

## 4. Units, coordinates and zoom

- **Unit.** A cell is 4 units wide, a floor 16 units tall. The grid is
  375 x 120 cells = 1500 x 1920 units. Row 0 is the top; row 109 is the
  ground floor (`GROUND_Y`); rows 110-119 are underground
  (`UNDERGROUND_Y` = 110 is the first row below the ground). The ground
  line is y = 110 x 16 = 1760 units.
- **Map position `m`.** A fragment's place in the tower, in units, float,
  y down. `m = (f / box.wh) * (1500, 1920)` where `f` is the fragment's
  target pixel less the icon's top left.
- **Texel scale `S`.** The atlas is rendered at S texels a unit; S = 4
  (render.py's scale 4). A picture's place in the atlas is in texels.
- **Zoom.** The icon is the whole tower; its size on the screen is the
  zoom. At 1:1 the icon is 1500 x 1920 target pixels; at 4x 6000 x 7680
  (within int16 screen coordinates). On a Retina Mac target pixels are
  physical, so 1:1 is 2 target pixels a unit.
- **Level of detail.** `tpp` (texels a target pixel) = S x 1500 / box.w;
  `lod = clamp(log2(tpp), 0, 3)`. Both are uniforms worked out on the host,
  the same way in both presenters. 1:1 on a plain screen is lod 2; 4x is
  lod 0; 4x on Retina is lod 0, magnified two times (linear).

---------------------------------------------------------------------------

## 5. The data

### 5.1 Who writes what

```
  tower engine (Linux, Rust)       broker (ROSApps)              presenter (PC GL / Mac Metal)
  ---------------------------      ----------------              -----------------------------
  sim tick -> WorldState    --->   "tower" : copy changed  --->  map both backings, copy the live
  publish(): live block            sections under the            part under the seqlock, upload
  in its buffer at LIVE,           seqlock into the live         sections whose gen moved, the art
  art block at ART (once)          backing; the art backing      once per art gen; draw P1, P2, P3
                                   when `art` moves;             per visible rectangle
                                   &5204 bind, frame+1
```

The engine is told `tower LIVE ART` (byte offsets in its buffer; proposed
16 MB and 20 MB, so the art may use up to 44 MB of the 64 MB buffer), and
says `tower` when a frame is whole.

### 5.2 The header file: rosgd/include/rosgd/tower.h

The header is in the tree; its Rust mirror is the `tower-surface` crate
(rosgd/ports/tower/surface, `block.rs`), whose tests check every size and
offset against the C compiler's and read a block written in Rust back,
field for field, in C (`tests/layout.rs`, `tests/c/tower_dump.c`). What
follows is its core; the header also names every bit and constant the
sections below use (`ROS_TW_S_*`, `ROS_TW_A_*`, `ROS_TW_M_*`, the
profiles, the fixture kinds) and the offsets of the art block's pictures
(`ros_tower_art_albedo` .. `ros_tower_art_size`).

Changed from the first proposal, once written: the two bytes after
`outline` are an explicit `reserved0` (the compiler put padding there);
`flags` has `ROS_TW_F_WEEKEND`; the art header was 72 bytes for a stated 64
and now carries `flags` (`ROS_TWA_F_AUX`: the aux picture is present, else
left out of the block) and `lang` (the signs' language) in 64; the lamp
profiles are two tables, weekday and weekend (6.5).

```c
/* tower.h -- a tower the presenter draws (ROS_SURF_TOWER).
 * Two blocks, each in a backing of its own: the live block (this struct,
 * at byte 60 of /dev/rosgd-backing-<live> on the PC, at its offset in the
 * backings region on the Mac) and the art block (struct ros_tower_art).
 * Bound to an icon with Wimp_Extend &5204, kind ROS_SURF_TOWER, data
 * struct ros_surf_tower.  The icon's box is the whole tower.
 * THE SEQLOCK is `frame`, as tiles.h.  NOTHING HERE IS TRUSTED. */

#define ROS_SURF_TOWER        7u
#define ROS_TOWER_MAGIC       0x52574F54u     /* "TOWR" */
#define ROS_TOWER_ART_MAGIC   0x41525754u     /* "TWRA" */
#define ROS_TOWER_VERSION     1u

#define ROS_TW_GRID_W   375
#define ROS_TW_GRID_H   120
#define ROS_TW_STRIDE   384     /* cells a row in the block (and the texture's width) */
#define ROS_TW_ROOMS    16384   /* room 0 is "none" */
#define ROS_TW_FIXTURES 512
#define ROS_TW_SHAFTS   64
#define ROS_TW_ROWS     128     /* rows in the stops table (120 used) */
#define ROS_TW_MOVERS   8192
#define ROS_TW_ART      1024
#define ROS_TW_SPRITES  1024
#define ROS_TW_PROFILES 16
#define ROS_TW_RAMP     48      /* the sky ramp: one column a half hour */

/* the sections, for gen[] */
enum { TW_CELLS, TW_ROOMS, TW_FIX, TW_STOPS, TW_PAL, TW_MOVERS, TW_NSECT };

struct ros_tower_cell    { uint16_t room, back, front, flags; };        /* RGBA16UI */
struct ros_tower_room    { uint32_t w[4]; };                            /* RGBA32UI */
struct ros_tower_fixture { uint32_t w[4]; };                            /* RGBA32UI */
struct ros_tower_mover   { uint16_t x, y, spr, param; };                /* RGBA16UI */

struct ros_tower {
    uint32_t magic, version;
    uint32_t frame;                 /* the seqlock */
    uint32_t art;                   /* the art block's gen this frame was made for */
    uint16_t grid_w, grid_h;        /* 375, 120 */
    uint16_t ground_row;            /* 110: the first row below the ground */
    uint16_t scale;                 /* S, texels a unit, as the art block's */
    uint16_t nrooms;                /* rooms 1 .. nrooms-1 may be named by cells */
    uint16_t nfixtures, nmovers;
    uint8_t  quality;               /* ROS_TILES_Q_* */
    uint8_t  rate;                  /* frames a second for the rich drawing */
    uint8_t  view;                  /* 0 plain, 1 evaluation, 2 a value ramp */
    uint8_t  flags;                 /* ROS_TW_F_WEEKEND */
    uint16_t hour;                  /* time of day in 1/64 hour, 0 .. 1535 */
    uint16_t day;                   /* the day count, for the lamps' seeds */
    int16_t  outline[4];            /* a frame round cells x0 y0 to x1 y1 */
    uint16_t reserved0;
    uint32_t outline_rgba;
    uint32_t back_rgba;             /* past the tower's edge */
    uint32_t gen[8];                /* each section's generation */
    uint32_t room_gen[16];          /* each 1024-room chunk's generation */
    uint32_t reserved[10];          /* the header is 192 bytes */
    struct ros_tower_cell    cell[ROS_TW_GRID_H][ROS_TW_STRIDE];  /* 368,640 */
    struct ros_tower_room    room[ROS_TW_ROOMS];                  /* 262,144 */
    struct ros_tower_fixture fix[ROS_TW_FIXTURES];                /*   8,192 */
    uint8_t                  stops[ROS_TW_SHAFTS][ROS_TW_ROWS];   /*   8,192 */
    uint32_t                 pal[16];                             /*      64 */
    struct ros_tower_mover   mover[ROS_TW_MOVERS];                /*  65,536 */
};                                                    /* 712,960 bytes in all */

struct ros_tower_art {
    uint32_t magic, version, gen, size;
    uint16_t scale;                 /* S */
    uint16_t atlas_w, atlas_h;      /* the rooms' and pieces' picture */
    uint16_t spr_w, spr_h;          /* the sprites' picture */
    uint16_t nart, nsprites;
    uint16_t emit_shift;            /* 1: the emission pictures are half size */
    uint16_t flags;                 /* ROS_TWA_F_AUX: the aux picture is present */
    uint16_t lang;                  /* the signs' language, two letters */
    uint32_t reserved[7];           /* 64 bytes */
    uint32_t art[ROS_TW_ART][4];          /* RGBA32UI, section 5.4 */
    uint32_t spr[ROS_TW_SPRITES][4];      /* RGBA32UI, section 5.5 */
    uint8_t  ramp[4][ROS_TW_RAMP][4];     /* zenith, horizon, ambient, misc */
    uint8_t  prof[2][ROS_TW_PROFILES][24];  /* weekday, weekend: lamp-on chance by hour */
    /* then, each packed tight, row by row:
     *   albedo   atlas_w x atlas_h RGBA8, premultiplied
     *   emission (atlas_w >> emit_shift) x (atlas_h >> emit_shift) RGBA8, premultiplied
     *   aux      atlas_w x atlas_h RG8 (material | slot << 4, group | height << 4),
     *            only when flags has ROS_TWA_F_AUX
     *   sprites  spr_w x spr_h RGBA8, premultiplied
     *   sprite emission spr_w x spr_h RGBA8, premultiplied            */
};

struct ros_surf_tower {             /* Wimp_Extend &5204 R5: 32 bytes */
    uint32_t backing, offset, size, frame;              /* the live block */
    uint32_t art_backing, art_offset, art_size, art_gen;/* the art block */
};
```

The cell table is RGBA16UI so that a cell can carry a *back* piece and a
*front* piece besides its room (5.3).

### 5.3 The cells

One `ros_tower_cell` a cell, row by row (row 0 the top), 384 to a row:

| Field | Meaning |
| --- | --- |
| `room` | the room covering the cell, 0 none. Every cell of a room names it, all its floors included. |
| `back` | an art entry drawn *behind* the room: soil, rock, street, basement, empty floor (`floor.svg`), wall-left/right at the building's edges, roof pieces in the sky row above the top floor, construction frames. 0 none. |
| `front` | an art entry drawn *over* the room: fire (left, centre, right, by the fire fronts), burnt, the slab, dust. 0 none. |
| `flags` | bit 0 inside the building's outline (for the rich mode's spill of light); others spare. |

Fire therefore needs no table of its own: the engine turns
`fire_left_pos`/`fire_right_pos` into `front` entries each tick of a fire.

### 5.4 The art table (RGBA32UI, 1024 x 1)

| Word | Bits | Meaning |
| --- | --- | --- |
| x | 0-15, 16-31 | the picture's left and top in the atlas, texels |
| y | 0-15, 16-31 | its width and height in texels (all its frames) |
| z | 0-15 | `alt`: the "occupied" picture of the same size (6.4), 0 none |
| z | 16-31 | repeat period across, texels (0: the width) |
| w | 0-15 | flags: 1 REPEAT (tile across, as the lobby), 2 LAMP_ALL (the whole room is one lamp group), 4 SOLID (no picture: x holds an RGBA8 colour), 8 GROUND (anchored with its bottom on the ground line), 16 ANIM (its frames run by the clock), 32 EMIT (has an emission picture), 64 AUX (has material and slot texels), 128 FIRE (a fire piece: classic tints its cells red) |
| w | 16-31 | frames, laid side by side across the width (1 if none) |

Entries 1 and 2 are fixed: the far and near skyline strips. Shafts use
eight consecutive entries from a base: body, top, pit, skip, landing door
frames 0-3.

### 5.5 The sprite table (RGBA32UI, 1024 x 1)

| Word | Meaning |
| --- | --- |
| x | left and top of frame 0 in the sprite atlas, texels |
| y | one frame's width and height, texels |
| z | the sprite's size in the tower, quarter units (width, height) |
| w | flags (16 ANIM, 32 EMIT, 64 TINT: the emission picture is a person's tint layer) and frames << 16 |

Sprites 1 to 255 are the signs (art/src/signs/<lang>, packed for one
language: 12.7), so that a room's one-byte `sign` (5.6) names its sprite
directly; their order is fixed by the `tower-surface` crate (`signs.rs`)
and is the same in every language. 256 is the cockroach (four frames).
Cars use nine consecutive sprites: car, doors 0-3, load 0-3.

### 5.6 The room record (RGBA32UI, a 128 x 128 texture)

| Word | Bits | Meaning |
| --- | --- | --- |
| w0 | 0-8 | x0, the left cell |
| w0 | 9-15 | ytop, the top row |
| w0 | 16-24 | width in cells (1-375: lobby and plain-floor runs are long) |
| w0 | 25-27 | height in floors (1-7) |
| w1 | 0-15 | art entry |
| w1 | 16-19 | lamp profile (0 never, 1 always, then office, home, hotel, shop, restaurant, fast food, entertainment, service, parking, lobby ...) |
| w1 | 20-21 | evaluation: 0 none, 1 red, 2 yellow, 3 blue |
| w1 | 24-31 | a value for the value view (rent level, stress, noise ...) |
| w2 | 0-15 | state bits: 1 VACANT, 2 SALE, 4 STUCK, 8 CLOSED, 16 INFESTED, 32 SHOWING, 64 VIP, 128 SELECTED, 256 LIT (forced on), 512 DARK (forced off) |
| w2 | 16-23 | occupancy, 0-255 |
| w2 | 24-31 | sign: the sprite (1-255) drawn over the room's top floor, 0 none: a vacant room's FOR RENT / FOR SALE banner, SOLD, LET (6.3) |
| w3 | all | seed: a hash of x0 and ytop (`seed()` in block.rs; the engine still picks variants with upstream's own hash) |

### 5.7 Fixtures (RGBA32UI, 512 x 1), stops (R8UI, 128 x 64)

| Word | Bits | Meaning |
| --- | --- | --- |
| w0 | 0-8, 9-15, 16-23, 24-30 | x0, ytop, width in cells, rows spanned |
| w1 | 0-15, 16-23 | art base, kind: 0 standard shaft, 1 express, 2 service, 3 stairs, 4 escalator |
| w2 | 0-7, 8-15 | shaft number (row of the stops table), units it rises above its top row (`over`: a shaft's machine room) |
| w3 | all | seed |

A shaft's fixture (kinds 0-2, and 5 the car lift) runs from its highest
floor's row down to the row under its lowest, which holds the pit; its
top piece stands `over` units above the highest floor (the top picture's
height); every row between is a landing, served or skipped.  Stairs and
escalators are one floor.

`stops[shaft][row]`: bit 0 served, bit 1 up call waiting, bit 2 down call
waiting, bits 4-5 the landing doors' frame (0 shut .. 3 open). The engine
takes them from `served_floor_flags`, the floor queues and the cars'
dwell state. As a texture it is 128 wide (rows) by 64 high (shafts).

### 5.8 Movers (RGBA16UI, 256 x 32)

| Field | Meaning |
| --- | --- |
| x, y | the mover's feet (bottom centre), quarter units |
| spr | bits 0-9 sprite; 10 FLIP; 11 BEHIND (hidden where a cell has a room: clouds, sun, moon); 12 LIT (lit from inside: a car) |
| param | bits 0-3 frame (when the sprite is not ANIM); for a person (a TINT sprite), bits 4-7 the top's colour, 8-9 the legs', 10-11 the skin's, 15 tinted (the colours are the shader's tables, from art/src/people/NOTES.md: tops 0-4 the public's, 5-7 staff, 8 stressed, 9 very stressed) |

Drawn in list order: the engine sorts sky things first, then cars (car,
load, doors), then people, then events.

### 5.9 What changes when

| Section | Size | Changes | Typical bytes a frame |
| --- | --- | --- | --- |
| header | 192 B | every frame (hour, counts, gens) | 192 |
| cells | 360 KB | building, demolition, fire, construction | 0 (one row, 3 KB, on a change) |
| rooms | 256 KB | each sim tick a few rooms change state, occupancy or evaluation | one or two 16 KB chunks |
| fixtures | 8 KB | building | 0 |
| stops | 8 KB | every tick (calls, doors) | 8 KB |
| palette | 64 B | when the view changes | 0 |
| movers | 64 KB | every frame | n x 8 (3,000 people: 24 KB) |
| art | ~36 MB | once, or on a change of art set | 0 |

The engine publishes at 30 frames a second, interpolating the cars and
people itself between sim ticks (as upstream's client does), so the
presenter draws what it is given and interpolates nothing. The broker
copies only sections whose gen moved; the presenter uploads only those.

### 5.10 The engine's side (ports/rosapps/tower-engine)

- **Rooms.** From `placed_objects` (one record an anchor), plus one room
  for each run of lobby cells on a row. Two-floor and taller families
  (cinema 18/19, party hall 29/30, recycling 20/21, metro 31-33, cathedral
  36-40) become one record with height 2, 3 or 5. Room indices are kept
  stable while a room stands (a free list), so a state change rewrites
  one record.
- **Cells.** `room` from the rooms; `back` from the cell type and
  position (empty floor, soil variant by hash, street on the ground row
  outside the building, walls at the building's left and right, roof
  pieces above the top floor, construction while a room is being built);
  `front` for fire and burnt cells.
- **Fixtures.** One a shaft (from `carriers`: column, kind, bottom and top
  served floors), one a stair or escalator overlay.
- **Movers.** Cars from `carriers[].cars[]` (position interpolated
  between ticks, three movers each: car, load level by `assigned_count`
  against capacity, doors by dwell); people queued at shafts (upstream's
  `getQueuedSimLayout`), people walking on a floor (interpolated from
  their route state), sprite by figure kind and trip stress, frame by
  walking phase; clouds, sun and moon; events (helicopter, firefighters,
  smoke, bomb, explosion, fireworks, VIP car).
- **Hour.** `dayTickToHour` (GameScene.ts) mapped to a clock: hour 7..31
  becomes 7..24, 0..7.

---------------------------------------------------------------------------

## 6. Drawing a room

### 6.1 Width and height

The fragment's cell `c = floor(m / (4, 16))`; `id = cell[c].room`; the
record gives the room's left `x0`, top `ytop`, width `wc` and height `hf`.
The room's size in units is `size = (wc x 4, hf x 16)` and the fragment's
place in it `l = m - (x0 x 4, ytop x 16)`. The picture's texel is

    p = l / size x (art width, art height)          (stretched: the normal case)
    p = (l.x mod period, l.y / size.y x art height) (REPEAT: lobby runs)

Pictures are drawn to the room's exact size (the art tree's rule:
`viewBox` = cells x 4 by floors x 16), so the stretch is 1 and nothing is
distorted; it is general only so that a lobby run of any length, or a
picture drawn at another scale, still fits. A two-floor cinema is one
record whose cells on both rows name it; the picture is 124 x 32 units
and is sampled once over both floors, with no seam between them.

### 6.2 Keeping the picture clean

The atlas is mipmapped and the level fixed by the zoom. A sample is kept
half a texel of that level inside its picture:
`e = 0.5 x 2^ceil(lod)`, `p = clamp(p, e, size - e)`; the packer also
leaves a 4-texel gutter round each picture, edge texels repeated. Together
they stop neighbours bleeding in at 1:1.

### 6.3 Variants and states: who decides

| State | How it is shown | Decided by |
| --- | --- | --- |
| variant (office.0-4, restaurant.0-4 ...) | the art entry | engine: upstream's hash of x, y |
| dirty hotel room | the `.dirty` art entry | engine: `unitStatus >= 0x28` |
| parking: no access, empty, one of four cars | the art entry | engine: `coverageFlag`, occupied |
| recycling fill 0-4 | the art entry | engine |
| vacant: for rent / for sale, just sold or let | the room's sign (a banner drawn to the room's slot, art/src/signs) over its top floor | engine picks the sign (w2 bits 24-31); the shader draws whatever sign a room names |
| stuck | grey, x 0.85 (as upstream) | shader, from STUCK |
| closed (shop after hours) | lamps off | shader, from CLOSED |
| occupancy | people in their seats (6.4) | shader, from occupancy |
| lamps at night, per room | 6.5 | shader, from profile, seed, hour, day |
| infested | cockroaches crawling the floor | shader, from INFESTED |
| showing (a film, a party) | the art's frames run; screens flicker | shader, from SHOWING |
| evaluation | the palette's colour at its alpha over the room | shader, from the 2-bit level and `view` |
| selected | a frame round the room | shader, from SELECTED |

The engine's mapping, family by family:

| Room | Cells x floors | Art | VACANT / SALE | STUCK | Occupancy | Profile |
| --- | --- | --- | --- | --- | --- | --- |
| office (7) | 9 x 1 | office.0-4 | `unitStatus > 0x0f` | `occupiedFlag == 0` | workers in, of 6 | office |
| condo (9) | 16 x 1 | condo.0 | `unitStatus > 0x17`, SALE | `occupiedFlag == 0` | residents home, of 3 | home |
| hotel single/twin/suite (3/4/5) | 4/6/10 x 1 | hotel-*.0-2, `.dirty` at `>= 0x28` | never (upstream shows none) | `>= 0x38` (+ INFESTED) | guests in | hotel |
| restaurant (6) | 24 x 1 | restaurant.0-4 | `occupiedFlag == 0` | -- | `current_population / capacity` | restaurant; CLOSED out of hours |
| fast food (12) | 16 x 1 | fast-food.0-3 | as restaurant | -- | as restaurant | fast food |
| retail (10) | 12 x 1 | retail.0-4 | as restaurant | -- | as restaurant | shop |
| cinema (18/19) | 31 x 2 | cinema | `occupiedFlag == 0` | -- | attendance | entertainment; SHOWING in phase 2 |
| party hall (29/30) | 27 x 2 | party-hall | as cinema | -- | attendance | entertainment |
| lobby (runs) | n x 1 | lobby (REPEAT) | -- | -- | people passing | lobby (always) |
| security (14), housekeeping (15) | 16, 15 x 1 | security, housekeeping | -- | -- | staff in | service |
| medical (13) | 26 x 1 | medical | -- | -- | `pending_visitors_count` | service |
| recycling (20/21) | 25 x 2 | recycling.0-4 by fill | -- | inadequate | -- | service |
| parking (24) | 4 x 1 | parking.empty / .0-3 / .no-access | -- | -- | -- | parking |
| parking ramp (44) | 15 x 1 | parking-ramp | -- | -- | -- | parking |
| metro (31-33) | 30 x 3 | metro (to come) | -- | -- | passengers | always |
| cathedral (36-40) | 28 x 5 | cathedral (to come; SOLID until then) | -- | -- | visitors | entertainment |

Plain floor is not a room: it is the cells' `back` piece (`floor.svg`).
Stairs, escalators and shafts are fixtures (P2).

### 6.4 Occupancy as people

A room picture with the AUX flag carries a *slot* number (1-15) on the
texels where a person would be (a chair, a bed, a desk). Its art entry's
`alt` names a picture of the same room with people in it. A slot texel
shows the occupied picture when

    hash(seed ^ slot x 0x27d4eb2f) x 255 < occupancy

so as occupancy rises, seats fill one by one in an order fixed for the
room. A room without slots shows occupancy only by its lamps.

### 6.5 Lamps

The art block's lamp profiles give, for each of 16 profiles and each
hour, the chance (0-255) that a lamp is on; it is sampled with linear
filtering, so the chance moves smoothly through the hour. A lamp group
(the aux texel's group, 0 for the whole room, or 0 always when
LAMP_ALL) is lit when

    h01(seed ^ group x 0x9e3779b9 ^ day x 0x85ebca6b) < chance(profile, hour)

so a room's lamps switch on at a moment of their own each evening, the
pattern differs day to day, and a room is the same from one frame to the
next. CLOSED and DARK force them off; LIT forces them on.

The art block holds two sets of profiles, weekday and weekend (12.10). The
shader still sees one 24 x 16 table: the presenter uploads the set the
live header's `ROS_TW_F_WEEKEND` names, again whenever that flag or the
art gen changes, so no uniform and no shader text depends on the day.

---------------------------------------------------------------------------

## 7. The passes and their shaders

### 7.1 The quality modes

| | classic (0) | smooth (1) | rich (2) |
| --- | --- | --- | --- |
| sky | upstream's two gradients and night fade, exactly | the art's sky ramp, stars, skyline strips | + the sun's disc and glow |
| rooms | the pictures as drawn, upstream's 1-unit gaps showing what is behind | lamps by profile, rooms dark at night unless lit, emission, windows show the sky | + ceiling and corner shading, contact shadows, light through partitions from lit neighbours, glass streaks |
| fire | translucent red (as upstream) plus the fire pieces | the fire pieces, emission at night | + noise flames and heat glow |
| movers | as drawn | night shading, emission | + soft drop shadow |
| rate | as frames come | as frames come | `rate` (default 20; 30 on the PC and on the Mac's key window) |

### 7.2 Uniforms, the same in both APIs

Only 16-byte members, so that GLSL's std140 block, Metal's struct and the
C struct agree byte for byte (480 bytes).

```c
typedef struct TwU {            /* C, GLSL (std140 block U), Metal (constant TwU &) */
    float box[4];       /* the whole tower on the target: x, y (down), w, h, target pixels */
    float clip[4];      /* the piece being drawn (Metal's vertex shader; GL scissors) */
    float view[4];      /* target w, target h, tpp, lod */
    float clock[4];     /* hour 0..24, seconds (wraps hourly), day, panel height (GL's flip) */
    float zen[4], hor[4], amb[4];   /* the sky at this hour: zenith, horizon, ambient */
    float misc[4];      /* daylight 0..1, stars, sun, glass reflection */
    int32_t grid[4];    /* grid w, grid h, ground row, S */
    int32_t counts[4];  /* nrooms, nart, nsprites, quality */
    int32_t sizes[4];   /* atlas w, h, sprite atlas w, h */
    int32_t mode[4];    /* view, emission shift, nfixtures, nmovers */
    int32_t outline[4]; /* cells x0 y0 x1 y1 */
    float ocol[4];
    float pal[16][4];   /* premultiplied: 1-3 evaluation red, yellow, blue; 4-15 the value ramp */
} TwU;
```

`zen`, `hor`, `amb` and `misc` are the sky ramp read at the hour on the
CPU, by one C function, `tw_light()`, which both presenters carry (the
generator emits it into both): linear interpolation between the two
half-hour columns, in float, from the bytes. Doing it on the CPU saves four
fetches a pixel and cannot differ between GPUs.

Evaluation colours (upstream's, premultiplied): red (221, 51, 51) at 0.5,
yellow (221, 204, 0) at 0.5, blue (68, 136, 255) at 0.5.

### 7.3 Textures and their state

| Name | Size | GL format | Metal format | Sampling |
| --- | --- | --- | --- | --- |
| cells | 384 x 120 | RGBA16UI | RGBA16Uint | texelFetch / read |
| rooms | 128 x 128 | RGBA32UI | RGBA32Uint | texelFetch / read |
| arttab, sprtab | 1024 x 1 | RGBA32UI | RGBA32Uint | texelFetch / read |
| fixtab | 512 x 1 | RGBA32UI | RGBA32Uint | texelFetch / read |
| stops | 128 x 64 | R8UI | R8Uint | texelFetch / read |
| movers | 256 x 32 | RGBA16UI | RGBA16Uint | texelFetch / read |
| albedo | atlas, levels 0-3 | RGBA8 | RGBA8Unorm | linear, mip linear, clamp |
| emis | half the atlas, levels 0-2 | RGBA8 | RGBA8Unorm | as albedo |
| aux | atlas, level 0 | RG8UI | RG8Uint | texelFetch / read |
| sprites, spremis | sprite atlas, levels 0-3 | RGBA8 | RGBA8Unorm | as albedo |
| prof | 24 x 16 | R8 | R8Unorm | linear, repeat across, clamp down |

On GL the sampling state lives on the texture object (set once in
`tw_take`); on Metal it is a `constexpr sampler` in the shader. The two
must say the same thing; section 8.3 lists them.

The PC presenter's tile map uses texture units 5 to 10; the tower uses
units 5 to 15 (P1 needs ten at once, which GLES 3.0's minimum of sixteen
fragment units allows).

### 7.4 P1, the field -- GLSL ES 3.00

Drawn with the presenter's own quad vertex shader (`vsh`), once per
visible rectangle, scissored, as `draw_tiles` does.

```glsl
#version 300 es
/* tw_field -- the tower: sky, ground, building pieces, rooms and overlays,
 * one quad over each visible piece of the icon. */
precision highp float;
precision highp int;
precision highp usampler2D;
precision highp sampler2D;

layout(std140) uniform TwU {
    vec4 box; vec4 clip; vec4 view; vec4 clock;
    vec4 zen; vec4 hor; vec4 amb; vec4 misc;
    ivec4 grid; ivec4 counts; ivec4 sizes; ivec4 mode;
    ivec4 outline; vec4 ocol;
    vec4 pal[16];
} U;

uniform usampler2D cells, rooms, arttab, sprtab, aux;
uniform sampler2D albedo, emis, sprites, spremis, prof;
out vec4 colour;

const vec2 CELL = vec2(4.0, 16.0);                     /* units a cell */
const uint S_VACANT = 1u, S_SALE = 2u, S_STUCK = 4u, S_CLOSED = 8u, S_INFESTED = 16u,
           S_SHOWING = 32u, S_SELECTED = 128u, S_LIT = 256u, S_DARK = 512u;
const uint A_REPEAT = 1u, A_LAMP_ALL = 2u, A_SOLID = 4u, A_GROUND = 8u, A_ANIM = 16u,
           A_EMIT = 32u, A_AUX = 64u, A_FIRE = 128u;
const uint M_GLASS = 2u, M_SCREEN = 7u;
const uint ART_SKY_FAR = 1u, ART_SKY_NEAR = 2u;
const uint SPR_ROACH = 256u;                           /* sprites 1-255 are the signs */
const vec3 LAMP = vec3(1.00, 0.90, 0.74);              /* a room lit at night */
const vec3 DARK_IN = vec3(0.10, 0.12, 0.20);           /* a room unlit at night */
const vec3 SLAB = vec3(0.46, 0.46, 0.48), PART = vec3(0.24, 0.24, 0.27);
const vec3 LUMA = vec3(0.299, 0.587, 0.114);

/* ---- hashing and noise (the tile map's, unchanged) ---------------------- */
uint hash(uint x) {
    x ^= x >> 16; x *= 0x7feb352du;
    x ^= x >> 15; x *= 0x846ca68bu;
    x ^= x >> 16;
    return x;
}
float h01(uint x) { return float(hash(x) >> 8) * (1.0 / 16777216.0); }
float hash2(int ix, int iy, uint seed) { return h01(uint(ix) + hash(uint(iy) + seed)); }
float vnoise(vec2 p, uint seed) {
    vec2 i = floor(p), f = p - i;
    vec2 u = f * f * (3.0 - 2.0 * f);
    ivec2 k = ivec2(i);
    float a = hash2(k.x, k.y, seed), b = hash2(k.x + 1, k.y, seed);
    float c = hash2(k.x, k.y + 1, seed), d = hash2(k.x + 1, k.y + 1, seed);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

/* ---- the tables --------------------------------------------------------- */
uvec4 artrec(uint a) { return texelFetch(arttab, ivec2(int(min(a, uint(U.counts.y) - 1u)), 0), 0); }
uvec4 sprrec(uint s) { return texelFetch(sprtab, ivec2(int(min(s, uint(U.counts.z) - 1u)), 0), 0); }
uvec4 room(uint id) { return texelFetch(rooms, ivec2(int(id & 127u), int(id >> 7)), 0); }
uvec4 cellat(ivec2 c) {
    if (c.x < 0 || c.y < 0 || c.x >= U.grid.x || c.y >= U.grid.y) return uvec4(0u);
    uvec4 k = texelFetch(cells, c, 0);
    if (k.x >= uint(U.counts.x)) k.x = 0u;             /* nothing is trusted */
    return k;
}

/* an art entry's rectangle, frame fr of it: x, y, w, h in texels */
uvec4 rect(uvec4 A, uint fr) {
    uint n = max(A.w >> 16, 1u), w = (A.y & 0xffffu) / n;
    return uvec4((A.x & 0xffffu) + (fr % n) * w, A.x >> 16, w, A.y >> 16);
}
/* the picture (premultiplied) at p, texels in r, kept inside r at this lod */
vec4 atl(uvec4 r, vec2 p, float lod) {
    float e = 0.5 * exp2(ceil(lod));
    vec2 q = clamp(p, vec2(e), max(vec2(r.zw) - e, vec2(e))) + vec2(r.xy);
    return textureLod(albedo, q / vec2(U.sizes.xy), lod);
}
vec4 atl_emis(uvec4 r, vec2 p, float lod) {             /* the same place, half size */
    float e = 0.5 * exp2(ceil(lod));
    vec2 q = clamp(p, vec2(e), max(vec2(r.zw) - e, vec2(e))) + vec2(r.xy);
    return textureLod(emis, q / vec2(U.sizes.xy), max(lod - float(U.mode.y), 0.0));
}
uvec2 auxat(uvec4 r, vec2 p) {
    ivec2 q = clamp(ivec2(floor(p)), ivec2(0), ivec2(r.zw) - 1) + ivec2(r.xy);
    return texelFetch(aux, q, 0).rg;
}
/* sprite s, frame fr, at uv 0..1 in it */
vec4 spr(uint s, uint fr, vec2 uv, float lod, out vec4 glow) {
    uvec4 S = sprrec(s);
    uint n = max(S.w >> 16, 1u);
    vec2 sz = vec2(float(S.y & 0xffffu), float(S.y >> 16));
    float e = 0.5 * exp2(ceil(lod));
    vec2 q = clamp(clamp(uv, 0.0, 1.0) * sz, vec2(e), max(sz - e, vec2(e)))
           + vec2(float((S.x & 0xffffu) + (fr % n) * (S.y & 0xffffu)), float(S.x >> 16));
    vec2 t = q / vec2(U.sizes.zw);
    glow = (S.w & A_EMIT) != 0u ? textureLod(spremis, t, lod) : vec4(0.0);
    return textureLod(sprites, t, lod);
}
vec3 over(vec3 under, vec4 top) { return top.rgb + under * (1.0 - top.a); }
vec4 unpack8(uint w) { return vec4(uvec4(w, w >> 8, w >> 16, w >> 24) & 255u) / 255.0; }

/* ---- light -------------------------------------------------------------- */
float night_alpha(float hour) {                       /* upstream's nightAlpha */
    float h = hour < 7.0 ? hour + 24.0 : hour;
    if (h < 20.0) return 0.0;
    if (h < 21.0) return h - 20.0;
    if (h < 28.0) return 1.0;
    if (h < 30.0) return 1.0 - (h - 28.0) * 0.5;
    return 0.0;
}
/* what the night does to a thing lit only by the sky: x colour, + glow */
vec3 outdoors(vec3 c, vec3 glow) {
    if (U.counts.w == 0) return c;
    return c * U.amb.rgb + glow * (1.0 - U.misc.x);
}
float lamp(uvec4 R, uint group) {
    uint st = R.z & 0xffffu;
    if ((st & (S_DARK | S_CLOSED)) != 0u) return 0.0;
    if ((st & S_LIT) != 0u) return 1.0;
    float chance = textureLod(prof, vec2((U.clock.x + 0.5) / 24.0,
                                         (float((R.y >> 16) & 15u) + 0.5) / 16.0), 0.0).r;
    return h01(R.w ^ group * 0x9e3779b9u ^ uint(U.clock.z) * 0x85ebca6bu) < chance ? 1.0 : 0.0;
}

/* ---- the sky and the ground --------------------------------------------- */
vec3 grad3(vec3 a, vec3 b, vec3 c, float mid, float t) {
    return t < mid ? mix(a, b, t / mid) : mix(b, c, (t - mid) / (1.0 - mid));
}
vec3 sky(vec2 m) {
    float ground = float(U.grid.z) * CELL.y;
    float t = clamp(m.y / ground, 0.0, 1.0);
    if (U.counts.w == 0) {
        vec3 d = grad3(vec3(0.102, 0.227, 0.431), vec3(0.357, 0.659, 0.831),
                       vec3(0.706, 0.867, 0.941), 0.6, t);
        vec3 n = grad3(vec3(0.016, 0.035, 0.102), vec3(0.039, 0.071, 0.208),
                       vec3(0.055, 0.122, 0.290), 0.5, t);
        return mix(d, n, night_alpha(U.clock.x));
    }
    vec3 c = mix(U.zen.rgb, U.hor.rgb, t * t);
    /* stars: about one in 256 squares of 10 units, a pixel or two across */
    ivec2 sq = ivec2(floor(m / 10.0));
    uint h = hash(uint(sq.x) * 73856093u ^ uint(sq.y) * 19349663u);
    if ((h & 255u) == 0u) {
        vec2 sp = (vec2(sq) + vec2(float((h >> 8) & 255u), float((h >> 16) & 255u)) / 255.0) * 10.0;
        float d = length(m - sp) * float(U.grid.w) / U.view.z;           /* target pixels */
        c += U.misc.y * (1.0 - smoothstep(0.6, 1.6, d)) * vec3(0.90, 0.94, 1.00);
    }
    if (U.counts.w >= 2 && U.misc.z > 0.0) {                            /* the sun */
        float k = clamp((U.clock.x - 6.0) / 13.0, 0.0, 1.0);
        vec2 sun = vec2(1500.0 * (0.1 + 0.8 * k), ground * (0.9 - 0.7 * sin(3.14159265 * k)));
        float d = length(m - sun);
        c += U.misc.z * (vec3(1.0, 0.95, 0.80) * (1.0 - smoothstep(14.0, 15.0, d))
                         + vec3(1.0, 0.80, 0.50) * 0.25 * exp2(-d / 75.0));
    }
    /* the skylines, far then near, standing on the ground */
    for (uint s = ART_SKY_FAR; s <= ART_SKY_NEAR && U.counts.w >= 1; s++) {
        uvec4 A = artrec(s), r = rect(A, 0u);
        float S = float(U.grid.w);
        float hgt = float(r.w) / S;                                      /* units */
        float y = m.y - (ground - hgt);
        if (y >= 0.0) {
            vec2 p = vec2(mod(m.x * S, float(r.z)), y * S);
            vec4 g = (A.w & A_EMIT) != 0u ? atl_emis(r, p, U.view.w) : vec4(0.0);
            vec4 a = atl(r, p, U.view.w);
            c = outdoors(a.rgb, g.rgb) + c * (1.0 - a.a);
        }
    }
    return c;
}

/* a back or front piece: grid-anchored, repeating every its own width;
 * GROUND pieces stand on the ground line */
vec4 piece(uint a, vec2 m, ivec2 c, out vec3 glow) {
    uvec4 A = artrec(a);
    if ((A.w & A_SOLID) != 0u) { glow = vec3(0.0); return unpack8(A.x); }
    uint fr = (A.w & A_ANIM) != 0u ? uint(U.clock.y * 8.0) + hash(uint(c.x) * 31u + uint(c.y)) : 0u;
    uvec4 r = rect(A, fr);
    float S = float(U.grid.w);
    vec2 p = vec2(mod(m.x * S, float(r.z)),
                  (A.w & A_GROUND) != 0u ? m.y * S - (float(U.grid.z) * CELL.y * S - float(r.w))
                                         : mod(m.y * S, float(r.w)));
    glow = (A.w & A_EMIT) != 0u ? atl_emis(r, p, U.view.w).rgb : vec3(0.0);
    return atl(r, p, U.view.w);
}

/* ---- a room ------------------------------------------------------------- */
vec3 roaches(uvec4 R, vec2 l, vec2 size, float floor_y, vec3 col) {
    int n = min(6, 2 + int((R.x >> 16) & 511u) / 4);
    for (int k = 0; k < 6; k++) {
        if (k >= n) break;
        uint hk = hash(R.w + uint(k) * 0x9e3779b9u);
        float dir = (hk & 1u) != 0u ? 1.0 : -1.0;
        float speed = 1.5 + float((hk >> 1) & 7u) * 0.25;               /* units a second */
        float bx = mod(float(hk >> 8) + dir * speed * U.clock.y, size.x - 4.0) + 2.0;
        float by = floor_y - 0.5 - float(k & 1);
        vec2 d = (l - vec2(bx - 1.25, by - 2.0)) / vec2(2.5, 2.0);       /* 2.5 x 2 units */
        if (d.x >= 0.0 && d.y >= 0.0 && d.x < 1.0 && d.y < 1.0) {
            if (dir < 0.0) d.x = 1.0 - d.x;
            vec4 g;
            vec4 b = spr(SPR_ROACH, uint(U.clock.y * 9.0) + uint(k), d, U.view.w, g);
            col = over(col, b);
        }
    }
    return col;
}

vec4 shade_room(uvec4 R, vec2 m, ivec2 c) {
    float x0 = float(R.x & 511u), yt = float((R.x >> 9) & 127u);
    float wc = float((R.x >> 16) & 511u), hf = float(max((R.x >> 25) & 7u, 1u));
    uint st = R.z & 0xffffu, occ = (R.z >> 16) & 255u;
    vec2 size = vec2(wc, hf) * CELL;
    vec2 l = m - vec2(x0, yt) * CELL;                   /* units into the room, y down */
    float fy = mod(l.y, CELL.y);                         /* units into its floor */
    int q = U.counts.w;
    float lod = U.view.w, S = float(U.grid.w);

    /* the structure: classic leaves upstream's gaps (what is behind shows);
     * the others draw a partition and the slab */
    float gx = q == 0 ? 1.0 : 0.75, gy = 1.0;
    if (l.x >= size.x - gx || fy >= CELL.y - gy) {
        if (q == 0) return vec4(0.0);
        vec3 s = fy >= CELL.y - gy ? SLAB : PART;
        return vec4(s * mix(vec3(0.45), vec3(1.0), U.misc.x), 1.0);
    }

    uvec4 A = artrec(R.y & 0xffffu);
    vec3 col, glow = vec3(0.0);
    uint mat = 0u, grp = 0u, ht = 0u;
    uvec4 r = uvec4(0u);
    vec2 p = vec2(0.0);
    if ((A.w & A_SOLID) != 0u) {
        col = unpack8(A.x).rgb;
    } else {
        uint fr = ((A.w & A_ANIM) != 0u || (st & S_SHOWING) != 0u) ? uint(U.clock.y * 6.0) : 0u;
        r = rect(A, fr);
        uint period = A.z >> 16;
        p = (A.w & A_REPEAT) != 0u
            ? vec2(mod(l.x * S, float(period != 0u ? period : r.z)), l.y / size.y * float(r.w))
            : l / size * vec2(r.zw);
        vec4 a = atl(r, p, lod);
        if ((A.w & A_AUX) != 0u) {
            uvec2 x = auxat(r, p);
            uint slot = x.r >> 4;
            mat = x.r & 15u; grp = x.g & 15u; ht = x.g >> 4;
            uint alt = A.z & 0xffffu;
            if (slot != 0u && alt != 0u &&
                float(occ) > h01(R.w ^ slot * 0x27d4eb2fu) * 255.0)
                a = atl(rect(artrec(alt), fr), p, lod);                 /* someone is there */
        }
        col = a.rgb;
        if ((A.w & A_EMIT) != 0u) glow = atl_emis(r, p, lod).rgb;
    }

    if (q >= 1) {
        float on = lamp(R, (A.w & A_LAMP_ALL) != 0u ? 0u : grp);
        float day = U.misc.x;
        vec3 base = col;
        col = base * mix(mix(DARK_IN, vec3(1.0), day), mix(LAMP, vec3(1.0), day), on)
            + glow * on * (1.0 - day);
        if (mat == M_GLASS) col = mix(col, sky(m), 0.7 - 0.35 * on);
        if (mat == M_SCREEN && (st & S_SHOWING) != 0u)
            col += vec3(0.6, 0.7, 0.9) * (0.25 + 0.2 * vnoise(vec2(U.clock.y * 4.0, x0), 51u));
        if (q >= 2) {
            /* the ceiling and the corners darker; objects throw a short
             * shadow down and to the right from the ceiling's lamps */
            float ao = (1.0 - 0.22 * (1.0 - smoothstep(0.0, 2.5, fy)))
                     * (1.0 - 0.18 * (1.0 - smoothstep(0.0, 2.0, min(l.x, size.x - gx - l.x))));
            float sh = 0.0;
            if ((A.w & A_AUX) != 0u)
                for (int k = 1; k <= 3; k++) {
                    uvec2 y = auxat(r, p - vec2(1.0, 2.0) * float(k) * exp2(lod));
                    if (float(y.g >> 4) > float(ht) + float(k)) sh = max(sh, 1.0 - 0.25 * float(k));
                }
            col *= ao * (1.0 - 0.35 * sh);
            /* lit neighbours' light through the partitions, at night */
            if (day < 0.95) {
                uvec4 kl = cellat(ivec2(int(x0) - 1, c.y)), kr = cellat(ivec2(int(x0 + wc), c.y));
                float s = 0.0;
                if (kl.x != 0u) s += lamp(room(kl.x), 0u) * exp2(-l.x / 6.0);
                if (kr.x != 0u) s += lamp(room(kr.x), 0u) * exp2(-(size.x - l.x) / 6.0);
                col += base * LAMP * s * 0.35 * (1.0 - day);
            }
            /* the glass in front of it all: the sky, faintly, in streaks */
            float w = fract((m.x + m.y * 0.35) / 225.0);
            float streak = smoothstep(0.40, 0.45, w) - smoothstep(0.52, 0.60, w);
            col = mix(col, U.hor.rgb, U.misc.w * (0.4 + 0.6 * streak));
        }
    }

    if ((st & S_STUCK) != 0u) col = vec3(dot(col, LUMA) * 0.85);
    uint sign = R.z >> 24;                              /* the room's sign: a banner drawn */
    if (sign != 0u && l.y < CELL.y) {                   /* to the slot, over the top floor */
        vec4 g;
        col = over(col, spr(sign, 0u, l / vec2(size.x, CELL.y), lod, g));
    }
    if ((st & S_INFESTED) != 0u) col = roaches(R, l, size, size.y - gy, col);
    uint ev = (R.y >> 20) & 3u;
    if (U.mode.x == 1 && ev != 0u) col = over(col, U.pal[int(ev)]);
    if (U.mode.x == 2) col = over(col, U.pal[4 + int((R.y >> 24) & 255u) * 12 / 256]);
    if ((st & S_SELECTED) != 0u) {
        float e = 2.0 * U.view.z / S;                    /* two target pixels, in units */
        if (l.x < e || l.y < e || l.x > size.x - gx - e || l.y > size.y - gy - e) col = U.ocol.rgb;
    }
    return vec4(col, 1.0);
}

/* rich fire: flames rising through the fire pieces, and their glow */
vec3 burn(vec3 col, vec2 m, float fy) {
    float up = 1.0 - fy / CELL.y;                        /* 0 at the floor, 1 at the ceiling */
    float n = vnoise(vec2(m.x / 3.0, m.y / 3.0 + U.clock.y * 2.5), 61u) * 0.7
            + vnoise(vec2(m.x / 1.25, m.y / 1.25 + U.clock.y * 5.0), 62u) * 0.3;
    float flame = smoothstep(0.35, 0.75, n * (1.25 - up));
    vec3 hot = mix(vec3(0.85, 0.20, 0.05), vec3(1.0, 0.85, 0.35), flame);
    return mix(col * 0.30, hot, flame) + vec3(0.35, 0.10, 0.02) * (1.0 - up);
}

/* ---- the fragment ------------------------------------------------------- */
vec4 field(vec2 f) {
    vec2 world = vec2(U.grid.xy) * CELL;
    vec2 m = f / U.box.zw * world;                       /* units */
    if (m.x < 0.0 || m.y < 0.0 || m.x >= world.x || m.y >= world.y) return vec4(0.0);
    ivec2 c = ivec2(floor(m / CELL));
    uvec4 k = cellat(c);
    vec3 col = c.y < U.grid.z ? sky(m) : vec3(0.24, 0.13, 0.06);
    vec3 g;
    if (k.y != 0u) {                                     /* behind the room */
        vec4 b = piece(k.y, m, c, g);
        col = outdoors(b.rgb, g) + col * (1.0 - b.a);
    }
    if (k.x != 0u) {
        vec4 r = shade_room(room(k.x), m, c);
        col = mix(col, r.rgb, r.a);
    }
    if (k.z != 0u) {                                     /* over it: fire, burnt, slab */
        vec4 b = piece(k.z, m, c, g);
        if (U.counts.w == 0 && (artrec(k.z).w & A_FIRE) != 0u)
            col = mix(col, vec3(0.87, 0.20, 0.20), 0.5 * b.a);
        else if (U.counts.w >= 2) col = mix(col, burn(col, m, mod(m.y, CELL.y)), b.a * 0.6);
        col = over(col, vec4(outdoors(b.rgb, g), b.a));
    }
    ivec4 o = U.outline;
    if (o.z > o.x && c.x >= o.x && c.y >= o.y && c.x < o.z && c.y < o.w) {
        vec2 kk = U.box.zw / world;                      /* target pixels a unit */
        vec2 lo = (m - vec2(o.xy) * CELL) * kk, hi = (vec2(o.zw) * CELL - m) * kk;
        if (min(min(lo.x, lo.y), min(hi.x, hi.y)) < 2.0) col = U.ocol.rgb;
    }
    return vec4(clamp(col, 0.0, 1.0), 1.0);
}

void main() {
    vec2 f = vec2(gl_FragCoord.x, U.clock.w - gl_FragCoord.y) - U.box.xy;
    colour = field(f);
    if (colour.a == 0.0) discard;
}
```

In classic, a fire piece (art flag FIRE) tints the cell red by its
coverage: upstream's translucent red between the fire fronts, once the
engine writes fire pieces there; other front pieces (burnt, the slab)
only draw. The vacant banner was upstream's 9:4 box centred in the room
with the words drawn by the engine; with signage as art (12.7) the
banners are drawn to the room's slot, so the room record names the sign
and the shader stretches it over the top floor as it does the room. GLSL ES 3.00 has no `unpackUnorm4x8` (only the 2x16 pair),
so the body carries the tile map's own `unpack8`, and the Metal side uses
the same function rather than `unpack_unorm4x8_to_float`.

### 7.5 P1, the field -- Metal

The same functions, in the same order, with the same names and
arithmetic. Metal has no mutable globals and no uniform blocks, so the
uniforms and textures travel in a `Res` passed by reference; `mod` is
written out as GLSL defines it (`x - y * floor(x / y)`), since Metal's
`fmod` truncates.

```metal
#include <metal_stdlib>
using namespace metal;

struct TwU {
    float4 box, clip, view, clock;
    float4 zen, hor, amb, misc;
    int4   grid, counts, sizes, mode, outline;
    float4 ocol;
    float4 pal[16];
};
struct Res {
    constant TwU *u;
    texture2d<ushort> cells;  texture2d<uint> rooms;
    texture2d<uint>   arttab; texture2d<uint> sprtab;
    texture2d<ushort> aux;
    texture2d<float>  albedo, emis, sprites, spremis, prof;
};
#define U (*R.u)

constexpr sampler TRI(coord::normalized, filter::linear, mip_filter::linear,
                      address::clamp_to_edge);
constexpr sampler PROF(coord::normalized, filter::linear,
                       s_address::repeat, t_address::clamp_to_edge);

constant float2 CELL = float2(4.0, 16.0);
constant uint S_VACANT = 1, S_SALE = 2, S_STUCK = 4, S_CLOSED = 8, S_INFESTED = 16,
              S_SHOWING = 32, S_SELECTED = 128, S_LIT = 256, S_DARK = 512;
constant uint A_REPEAT = 1, A_LAMP_ALL = 2, A_SOLID = 4, A_GROUND = 8, A_ANIM = 16,
              A_EMIT = 32, A_AUX = 64, A_FIRE = 128;
constant uint M_GLASS = 2, M_SCREEN = 7;
constant uint ART_SKY_FAR = 1, ART_SKY_NEAR = 2;
constant uint SPR_ROACH = 256;
constant float3 LAMP = float3(1.00, 0.90, 0.74);
constant float3 DARK_IN = float3(0.10, 0.12, 0.20);
constant float3 SLAB = float3(0.46, 0.46, 0.48), PART = float3(0.24, 0.24, 0.27);
constant float3 LUMA = float3(0.299, 0.587, 0.114);

static float  gmod(float a, float b)   { return a - b * floor(a / b); }
static float2 gmod(float2 a, float2 b) { return a - b * floor(a / b); }
static float4 unpack8(uint w) { return float4(uint4(w, w >> 8, w >> 16, w >> 24) & 255u) / 255.0; }

static uint hash(uint x) {
    x ^= x >> 16; x *= 0x7feb352du;
    x ^= x >> 15; x *= 0x846ca68bu;
    x ^= x >> 16;
    return x;
}
static float h01(uint x) { return float(hash(x) >> 8) * (1.0 / 16777216.0); }
static float hash2(int ix, int iy, uint seed) { return h01(uint(ix) + hash(uint(iy) + seed)); }
static float vnoise(float2 p, uint seed) {
    float2 i = floor(p), f = p - i;
    float2 u = f * f * (3.0 - 2.0 * f);
    int2 k = int2(i);
    float a = hash2(k.x, k.y, seed), b = hash2(k.x + 1, k.y, seed);
    float c = hash2(k.x, k.y + 1, seed), d = hash2(k.x + 1, k.y + 1, seed);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

static uint4 artrec(thread const Res &R, uint a) {
    return R.arttab.read(uint2(min(a, uint(U.counts.y) - 1u), 0));
}
static uint4 sprrec(thread const Res &R, uint s) {
    return R.sprtab.read(uint2(min(s, uint(U.counts.z) - 1u), 0));
}
static uint4 room(thread const Res &R, uint id) { return R.rooms.read(uint2(id & 127u, id >> 7)); }
static uint4 cellat(thread const Res &R, int2 c) {
    if (c.x < 0 || c.y < 0 || c.x >= U.grid.x || c.y >= U.grid.y) return uint4(0);
    uint4 k = uint4(R.cells.read(uint2(c)));
    if (k.x >= uint(U.counts.x)) k.x = 0;
    return k;
}
static uint4 rect(uint4 A, uint fr) {
    uint n = max(A.w >> 16, 1u), w = (A.y & 0xffffu) / n;
    return uint4((A.x & 0xffffu) + (fr % n) * w, A.x >> 16, w, A.y >> 16);
}
static float4 atl(thread const Res &R, uint4 r, float2 p, float lod) {
    float e = 0.5 * exp2(ceil(lod));
    float2 q = clamp(p, float2(e), max(float2(r.zw) - e, float2(e))) + float2(r.xy);
    return R.albedo.sample(TRI, q / float2(U.sizes.xy), level(lod));
}
static float4 atl_emis(thread const Res &R, uint4 r, float2 p, float lod) {
    float e = 0.5 * exp2(ceil(lod));
    float2 q = clamp(p, float2(e), max(float2(r.zw) - e, float2(e))) + float2(r.xy);
    return R.emis.sample(TRI, q / float2(U.sizes.xy), level(max(lod - float(U.mode.y), 0.0)));
}
static uint2 auxat(thread const Res &R, uint4 r, float2 p) {
    int2 q = clamp(int2(floor(p)), int2(0), int2(r.zw) - 1) + int2(r.xy);
    return uint2(R.aux.read(uint2(q)).rg);
}
static float4 spr(thread const Res &R, uint s, uint fr, float2 uv, float lod, thread float4 &glow) {
    uint4 S = sprrec(R, s);
    uint n = max(S.w >> 16, 1u);
    float2 sz = float2(float(S.y & 0xffffu), float(S.y >> 16));
    float e = 0.5 * exp2(ceil(lod));
    float2 q = clamp(clamp(uv, 0.0, 1.0) * sz, float2(e), max(sz - e, float2(e)))
             + float2(float((S.x & 0xffffu) + (fr % n) * (S.y & 0xffffu)), float(S.x >> 16));
    float2 t = q / float2(U.sizes.zw);
    glow = (S.w & A_EMIT) != 0 ? R.spremis.sample(TRI, t, level(lod)) : float4(0);
    return R.sprites.sample(TRI, t, level(lod));
}
static float3 over(float3 under, float4 top) { return top.rgb + under * (1.0 - top.a); }

static float night_alpha(float hour) {
    float h = hour < 7.0 ? hour + 24.0 : hour;
    if (h < 20.0) return 0.0;
    if (h < 21.0) return h - 20.0;
    if (h < 28.0) return 1.0;
    if (h < 30.0) return 1.0 - (h - 28.0) * 0.5;
    return 0.0;
}
static float3 outdoors(thread const Res &R, float3 c, float3 glow) {
    if (U.counts.w == 0) return c;
    return c * U.amb.rgb + glow * (1.0 - U.misc.x);
}
static float lamp(thread const Res &R, uint4 Rm, uint group) {
    uint st = Rm.z & 0xffffu;
    if ((st & (S_DARK | S_CLOSED)) != 0) return 0.0;
    if ((st & S_LIT) != 0) return 1.0;
    float chance = R.prof.sample(PROF, float2((U.clock.x + 0.5) / 24.0,
                                (float((Rm.y >> 16) & 15u) + 0.5) / 16.0), level(0.0)).r;
    return h01(Rm.w ^ group * 0x9e3779b9u ^ uint(U.clock.z) * 0x85ebca6bu) < chance ? 1.0 : 0.0;
}

static float3 grad3(float3 a, float3 b, float3 c, float mid, float t) {
    return t < mid ? mix(a, b, t / mid) : mix(b, c, (t - mid) / (1.0 - mid));
}
static float3 sky(thread const Res &R, float2 m) {
    float ground = float(U.grid.z) * CELL.y;
    float t = clamp(m.y / ground, 0.0, 1.0);
    if (U.counts.w == 0) {
        float3 d = grad3(float3(0.102, 0.227, 0.431), float3(0.357, 0.659, 0.831),
                         float3(0.706, 0.867, 0.941), 0.6, t);
        float3 n = grad3(float3(0.016, 0.035, 0.102), float3(0.039, 0.071, 0.208),
                         float3(0.055, 0.122, 0.290), 0.5, t);
        return mix(d, n, night_alpha(U.clock.x));
    }
    float3 c = mix(U.zen.rgb, U.hor.rgb, t * t);
    int2 sq = int2(floor(m / 10.0));
    uint h = hash(uint(sq.x) * 73856093u ^ uint(sq.y) * 19349663u);
    if ((h & 255u) == 0) {
        float2 sp = (float2(sq) + float2(float((h >> 8) & 255u), float((h >> 16) & 255u)) / 255.0) * 10.0;
        float d = length(m - sp) * float(U.grid.w) / U.view.z;
        c += U.misc.y * (1.0 - smoothstep(0.6, 1.6, d)) * float3(0.90, 0.94, 1.00);
    }
    if (U.counts.w >= 2 && U.misc.z > 0.0) {
        float k = clamp((U.clock.x - 6.0) / 13.0, 0.0, 1.0);
        float2 sun = float2(1500.0 * (0.1 + 0.8 * k), ground * (0.9 - 0.7 * sin(3.14159265 * k)));
        float d = length(m - sun);
        c += U.misc.z * (float3(1.0, 0.95, 0.80) * (1.0 - smoothstep(14.0, 15.0, d))
                         + float3(1.0, 0.80, 0.50) * 0.25 * exp2(-d / 75.0));
    }
    for (uint s = ART_SKY_FAR; s <= ART_SKY_NEAR && U.counts.w >= 1; s++) {
        uint4 A = artrec(R, s), r = rect(A, 0);
        float S = float(U.grid.w);
        float hgt = float(r.w) / S;
        float y = m.y - (ground - hgt);
        if (y >= 0.0) {
            float2 p = float2(gmod(m.x * S, float(r.z)), y * S);
            float4 g = (A.w & A_EMIT) != 0 ? atl_emis(R, r, p, U.view.w) : float4(0);
            float4 a = atl(R, r, p, U.view.w);
            c = outdoors(R, a.rgb, g.rgb) + c * (1.0 - a.a);
        }
    }
    return c;
}

static float4 piece(thread const Res &R, uint a, float2 m, int2 c, thread float3 &glow) {
    uint4 A = artrec(R, a);
    if ((A.w & A_SOLID) != 0) { glow = float3(0); return unpack8(A.x); }
    uint fr = (A.w & A_ANIM) != 0 ? uint(U.clock.y * 8.0) + hash(uint(c.x) * 31u + uint(c.y)) : 0u;
    uint4 r = rect(A, fr);
    float S = float(U.grid.w);
    float2 p = float2(gmod(m.x * S, float(r.z)),
                      (A.w & A_GROUND) != 0 ? m.y * S - (float(U.grid.z) * CELL.y * S - float(r.w))
                                            : gmod(m.y * S, float(r.w)));
    glow = (A.w & A_EMIT) != 0 ? atl_emis(R, r, p, U.view.w).rgb : float3(0);
    return atl(R, r, p, U.view.w);
}

static float3 roaches(thread const Res &R, uint4 Rm, float2 l, float2 size, float floor_y, float3 col) {
    int n = min(6, 2 + int((Rm.x >> 16) & 511u) / 4);
    for (int k = 0; k < 6; k++) {
        if (k >= n) break;
        uint hk = hash(Rm.w + uint(k) * 0x9e3779b9u);
        float dir = (hk & 1u) != 0 ? 1.0 : -1.0;
        float speed = 1.5 + float((hk >> 1) & 7u) * 0.25;
        float bx = gmod(float(hk >> 8) + dir * speed * U.clock.y, size.x - 4.0) + 2.0;
        float by = floor_y - 0.5 - float(k & 1);
        float2 d = (l - float2(bx - 1.25, by - 2.0)) / float2(2.5, 2.0);
        if (d.x >= 0.0 && d.y >= 0.0 && d.x < 1.0 && d.y < 1.0) {
            if (dir < 0.0) d.x = 1.0 - d.x;
            float4 g;
            float4 b = spr(R, SPR_ROACH, uint(U.clock.y * 9.0) + uint(k), d, U.view.w, g);
            col = over(col, b);
        }
    }
    return col;
}

static float4 shade_room(thread const Res &R, uint4 Rm, float2 m, int2 c) {
    float x0 = float(Rm.x & 511u), yt = float((Rm.x >> 9) & 127u);
    float wc = float((Rm.x >> 16) & 511u), hf = float(max((Rm.x >> 25) & 7u, 1u));
    uint st = Rm.z & 0xffffu, occ = (Rm.z >> 16) & 255u;
    float2 size = float2(wc, hf) * CELL;
    float2 l = m - float2(x0, yt) * CELL;
    float fy = gmod(l.y, CELL.y);
    int q = U.counts.w;
    float lod = U.view.w, S = float(U.grid.w);

    float gx = q == 0 ? 1.0 : 0.75, gy = 1.0;
    if (l.x >= size.x - gx || fy >= CELL.y - gy) {
        if (q == 0) return float4(0);
        float3 s = fy >= CELL.y - gy ? SLAB : PART;
        return float4(s * mix(float3(0.45), float3(1.0), U.misc.x), 1.0);
    }

    uint4 A = artrec(R, Rm.y & 0xffffu);
    float3 col, glow = float3(0);
    uint mat = 0, grp = 0, ht = 0;
    uint4 r = uint4(0);
    float2 p = float2(0);
    if ((A.w & A_SOLID) != 0) {
        col = unpack8(A.x).rgb;
    } else {
        uint fr = ((A.w & A_ANIM) != 0 || (st & S_SHOWING) != 0) ? uint(U.clock.y * 6.0) : 0u;
        r = rect(A, fr);
        uint period = A.z >> 16;
        p = (A.w & A_REPEAT) != 0
            ? float2(gmod(l.x * S, float(period != 0 ? period : r.z)), l.y / size.y * float(r.w))
            : l / size * float2(r.zw);
        float4 a = atl(R, r, p, lod);
        if ((A.w & A_AUX) != 0) {
            uint2 x = auxat(R, r, p);
            uint slot = x.r >> 4;
            mat = x.r & 15u; grp = x.g & 15u; ht = x.g >> 4;
            uint alt = A.z & 0xffffu;
            if (slot != 0 && alt != 0 && float(occ) > h01(Rm.w ^ slot * 0x27d4eb2fu) * 255.0)
                a = atl(R, rect(artrec(R, alt), fr), p, lod);
        }
        col = a.rgb;
        if ((A.w & A_EMIT) != 0) glow = atl_emis(R, r, p, lod).rgb;
    }

    if (q >= 1) {
        float on = lamp(R, Rm, (A.w & A_LAMP_ALL) != 0 ? 0u : grp);
        float day = U.misc.x;
        float3 base = col;
        col = base * mix(mix(DARK_IN, float3(1.0), day), mix(LAMP, float3(1.0), day), on)
            + glow * on * (1.0 - day);
        if (mat == M_GLASS) col = mix(col, sky(R, m), 0.7 - 0.35 * on);
        if (mat == M_SCREEN && (st & S_SHOWING) != 0)
            col += float3(0.6, 0.7, 0.9) * (0.25 + 0.2 * vnoise(float2(U.clock.y * 4.0, x0), 51u));
        if (q >= 2) {
            float ao = (1.0 - 0.22 * (1.0 - smoothstep(0.0, 2.5, fy)))
                     * (1.0 - 0.18 * (1.0 - smoothstep(0.0, 2.0, min(l.x, size.x - gx - l.x))));
            float sh = 0.0;
            if ((A.w & A_AUX) != 0)
                for (int k = 1; k <= 3; k++) {
                    uint2 y = auxat(R, r, p - float2(1.0, 2.0) * float(k) * exp2(lod));
                    if (float(y.g >> 4) > float(ht) + float(k)) sh = max(sh, 1.0 - 0.25 * float(k));
                }
            col *= ao * (1.0 - 0.35 * sh);
            if (day < 0.95) {
                uint4 kl = cellat(R, int2(int(x0) - 1, c.y)), kr = cellat(R, int2(int(x0 + wc), c.y));
                float s = 0.0;
                if (kl.x != 0) s += lamp(R, room(R, kl.x), 0u) * exp2(-l.x / 6.0);
                if (kr.x != 0) s += lamp(R, room(R, kr.x), 0u) * exp2(-(size.x - l.x) / 6.0);
                col += base * LAMP * s * 0.35 * (1.0 - day);
            }
            float w = fract((m.x + m.y * 0.35) / 225.0);
            float streak = smoothstep(0.40, 0.45, w) - smoothstep(0.52, 0.60, w);
            col = mix(col, U.hor.rgb, U.misc.w * (0.4 + 0.6 * streak));
        }
    }

    if ((st & S_STUCK) != 0) col = float3(dot(col, LUMA) * 0.85);
    uint sign = Rm.z >> 24;
    if (sign != 0 && l.y < CELL.y) {
        float4 g;
        col = over(col, spr(R, sign, 0u, l / float2(size.x, CELL.y), lod, g));
    }
    if ((st & S_INFESTED) != 0) col = roaches(R, Rm, l, size, size.y - gy, col);
    uint ev = (Rm.y >> 20) & 3u;
    if (U.mode.x == 1 && ev != 0) col = over(col, U.pal[ev]);
    if (U.mode.x == 2) col = over(col, U.pal[4 + int((Rm.y >> 24) & 255u) * 12 / 256]);
    if ((st & S_SELECTED) != 0) {
        float e = 2.0 * U.view.z / S;
        if (l.x < e || l.y < e || l.x > size.x - gx - e || l.y > size.y - gy - e) col = U.ocol.rgb;
    }
    return float4(col, 1.0);
}

static float3 burn(thread const Res &R, float3 col, float2 m, float fy) {
    float up = 1.0 - fy / CELL.y;
    float n = vnoise(float2(m.x / 3.0, m.y / 3.0 + U.clock.y * 2.5), 61u) * 0.7
            + vnoise(float2(m.x / 1.25, m.y / 1.25 + U.clock.y * 5.0), 62u) * 0.3;
    float flame = smoothstep(0.35, 0.75, n * (1.25 - up));
    float3 hot = mix(float3(0.85, 0.20, 0.05), float3(1.0, 0.85, 0.35), flame);
    return mix(col * 0.30, hot, flame) + float3(0.35, 0.10, 0.02) * (1.0 - up);
}

static float4 field(thread const Res &R, float2 f) {
    float2 world = float2(U.grid.xy) * CELL;
    float2 m = f / U.box.zw * world;
    if (m.x < 0.0 || m.y < 0.0 || m.x >= world.x || m.y >= world.y) return float4(0);
    int2 c = int2(floor(m / CELL));
    uint4 k = cellat(R, c);
    float3 col = c.y < U.grid.z ? sky(R, m) : float3(0.24, 0.13, 0.06);
    float3 g;
    if (k.y != 0) {
        float4 b = piece(R, k.y, m, c, g);
        col = outdoors(R, b.rgb, g) + col * (1.0 - b.a);
    }
    if (k.x != 0) {
        float4 r = shade_room(R, room(R, k.x), m, c);
        col = mix(col, r.rgb, r.a);
    }
    if (k.z != 0) {
        float4 b = piece(R, k.z, m, c, g);
        if (U.counts.w == 0 && (artrec(R, k.z).w & A_FIRE) != 0)
            col = mix(col, float3(0.87, 0.20, 0.20), 0.5 * b.a);
        else if (U.counts.w >= 2) col = mix(col, burn(R, col, m, gmod(m.y, CELL.y)), b.a * 0.6);
        col = over(col, float4(outdoors(R, b.rgb, g), b.a));
    }
    int4 o = U.outline;
    if (o.z > o.x && c.x >= o.x && c.y >= o.y && c.x < o.z && c.y < o.w) {
        float2 kk = U.box.zw / world;
        float2 lo = (m - float2(o.xy) * CELL) * kk, hi = (float2(o.zw) * CELL - m) * kk;
        if (min(min(lo.x, lo.y), min(hi.x, hi.y)) < 2.0) col = U.ocol.rgb;
    }
    return float4(clamp(col, 0.0, 1.0), 1.0);
}

struct TwV { float4 pos [[position]]; };

vertex TwV tw_vs(uint id [[vertex_id]], constant TwU &u [[buffer(0)]]) {
    float2 k = float2(id & 1, (id >> 1) & 1);
    float2 p = mix(u.clip.xy, u.clip.zw, k);
    TwV o;
    o.pos = float4(p / u.view.xy * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}

fragment float4 tw_field(TwV v [[stage_in]], constant TwU &u [[buffer(0)]],
        texture2d<ushort> cells   [[texture(0)]], texture2d<uint>  rooms   [[texture(1)]],
        texture2d<uint>   arttab  [[texture(2)]], texture2d<uint>  sprtab  [[texture(3)]],
        texture2d<float>  albedo  [[texture(4)]], texture2d<float> emis    [[texture(5)]],
        texture2d<ushort> aux     [[texture(6)]], texture2d<float> sprites [[texture(7)]],
        texture2d<float>  spremis [[texture(8)]], texture2d<float> prof    [[texture(9)]])
{
    Res R = { &u, cells, rooms, arttab, sprtab, aux, albedo, emis, sprites, spremis, prof };
    float4 c = field(R, v.pos.xy - u.box.xy);       /* Metal's origin is the top left */
    if (c.a == 0.0) discard_fragment();
    return c;
}
```

Where the two differ, and why, is all in the entry points: GL flips
`gl_FragCoord.y` with the panel height, Metal's position is already y-down;
GL draws the presenter's full-panel quad and scissors, Metal draws the
clip rectangle (the tile map's `tl_vs`). Both produce the same fragments
at the same pixel centres.

### 7.6 P2, fixtures -- GLSL ES 3.00

Instanced: `glDrawArraysInstanced(GL_TRIANGLE_STRIP, 0, 4, nfixtures)`,
no vertex attributes (corners from `gl_VertexID`, the record from
`gl_InstanceID`), once per visible rectangle under its scissor. Blending
premultiplied (`GL_ONE, GL_ONE_MINUS_SRC_ALPHA`), as the tile map's
sprites.

```glsl
#version 300 es
/* tw_fix_vs */
precision highp float; precision highp int; precision highp usampler2D;
layout(std140) uniform TwU { /* as P1 */ vec4 box; vec4 clip; vec4 view; vec4 clock;
    vec4 zen; vec4 hor; vec4 amb; vec4 misc; ivec4 grid; ivec4 counts; ivec4 sizes;
    ivec4 mode; ivec4 outline; vec4 ocol; vec4 pal[16]; } U;
uniform usampler2D fixtab;
flat out uvec4 F;
out vec2 lp;                                    /* units into the fixture */
void main() {
    uvec4 r = texelFetch(fixtab, ivec2(min(gl_InstanceID, U.mode.z - 1), 0), 0);
    const vec2 CELL = vec2(4.0, 16.0);
    float over_ = float((r.z >> 8) & 255u);
    vec2 o  = vec2(float(r.x & 511u), float((r.x >> 9) & 127u)) * CELL - vec2(0.0, over_);
    vec2 sz = vec2(float((r.x >> 16) & 255u) * CELL.x, float((r.x >> 24) & 127u) * CELL.y + over_);
    vec2 k  = vec2(float(gl_VertexID & 1), float(gl_VertexID >> 1));
    lp = k * sz;
    vec2 world = vec2(U.grid.xy) * CELL;
    vec2 t = U.box.xy + (o + lp) / world * U.box.zw;              /* target pixels, y down */
    gl_Position = vec4(t.x / U.view.x * 2.0 - 1.0, 1.0 - t.y / U.view.y * 2.0, 0.0, 1.0);
    F = r;
}
```

```glsl
#version 300 es
/* tw_fix -- shafts as body, top, pit and landings; stairs; escalators */
precision highp float; precision highp int; precision highp usampler2D; precision highp sampler2D;
layout(std140) uniform TwU { /* as P1 */ ... } U;
uniform usampler2D arttab, stops;
uniform sampler2D albedo, emis;
flat in uvec4 F;
in vec2 lp;
out vec4 colour;
/* artrec, rect, atl, atl_emis, outdoors as P1 (the generator includes them) */
void main() {
    const vec2 CELL = vec2(4.0, 16.0);
    uint kind = (F.y >> 16) & 255u, base = F.y & 0xffffu;
    float S = float(U.grid.w), lod = U.view.w;
    float over_ = float((F.z >> 8) & 255u);
    uint a;
    vec2 p;
    float fy = mod(lp.y - over_, CELL.y);
    if (kind <= 2u || kind == 5u) {                             /* a shaft, or the car lift */
        if (lp.y < over_) {                                     /* the top, standing on the highest floor */
            uvec4 r = rect(artrec(base + 1u), 0u);
            vec4 c = atl(r, vec2(lp.x * S, (lp.y - over_) * S + float(r.w)), lod);
            colour = vec4(outdoors(c.rgb, vec3(0.0)), c.a);
            return;
        }
        int top = int((F.x >> 9) & 127u), rows = int((F.x >> 24) & 127u);
        int row = top + int(floor((lp.y - over_) / CELL.y));
        p = vec2(lp.x * S, fy * S);
        if (row >= top + rows - 1) {                            /* the pit, under the lowest floor */
            uvec4 r = rect(artrec(base + 2u), 0u);
            if (p.y >= float(r.w)) { colour = vec4(0.0); return; }
            vec4 c = atl(r, p, lod);
            colour = vec4(outdoors(c.rgb, vec3(0.0)), c.a);
            return;
        }
        uint s = texelFetch(stops, ivec2(clamp(row, 0, 127), int(F.z & 63u)), 0).r;
        a = (s & 1u) != 0u ? base + 4u + ((s >> 4) & 3u)        /* landing, its doors' frame */
          : base + 3u;                                          /* not served: skip */
        uvec4 bodyr = rect(artrec(base), 0u);
        vec4 body = atl(bodyr, p, lod);                             /* the shaft behind */
        uvec4 r = rect(artrec(a), 0u);
        vec4 front = atl(r, p, lod);
        vec4 g = (artrec(a).w & A_EMIT) != 0u ? atl_emis(r, p, lod) : vec4(0.0);
        /* the call lamps: the landing's emission lights while someone waits */
        float calling = (s & 6u) != 0u ? 1.0 : 0.25;
        vec3 c = over(outdoors(body.rgb, vec3(0.0)), vec4(outdoors(front.rgb, g.rgb * calling), front.a));
        colour = vec4(c, max(body.a, front.a));
    } else {                                                    /* stairs, escalator */
        uvec4 A = artrec(base);
        uint fr = (A.w & A_ANIM) != 0u ? uint(U.clock.y * 8.0) : 0u;
        uvec4 r = rect(A, fr);
        vec2 sz = vec2(float((F.x >> 16) & 255u) * CELL.x,
                       float((F.x >> 24) & 127u) * CELL.y + over_);
        p = lp / sz * vec2(r.zw);
        vec4 c = atl(r, p, lod);
        vec4 g = (A.w & A_EMIT) != 0u ? atl_emis(r, p, lod) : vec4(0.0);
        colour = vec4(outdoors(c.rgb, g.rgb), c.a);              /* premultiplied */
    }
}
```

### 7.7 P2, fixtures -- Metal

```metal
/* in the same library as P1, after its functions */
/* ---- P2, the fixtures (design 7.7) --------------------------------------- */
constant uint FIX_CARLIFT = 5;

struct TwF { float4 pos [[position]]; float2 lp; uint4 F [[flat]]; };

vertex TwF tw_fix_vs(uint id [[vertex_id]], uint inst [[instance_id]],
                     constant TwU &u [[buffer(0)]], texture2d<uint> fixtab [[texture(0)]])
{
    uint4 r = fixtab.read(uint2(min(int(inst), u.mode.z - 1), 0));
    float over_ = float((r.z >> 8) & 255u);
    float2 o  = float2(float(r.x & 511u), float((r.x >> 9) & 127u)) * CELL - float2(0.0, over_);
    float2 sz = float2(float((r.x >> 16) & 255u) * CELL.x, float((r.x >> 24) & 127u) * CELL.y + over_);
    float2 k  = float2(id & 1, (id >> 1) & 1);
    TwF v;
    v.lp = k * sz;
    float2 world = float2(u.grid.xy) * CELL;
    float2 t = u.box.xy + (o + v.lp) / world * u.box.zw;
    v.pos = float4(t / u.view.xy * float2(2, -2) + float2(-1, 1), 0, 1);
    v.F = r;
    return v;
}

fragment float4 tw_fix(TwF v [[stage_in]], constant TwU &u [[buffer(0)]],
        texture2d<ushort> cells   [[texture(0)]], texture2d<uint>  rooms   [[texture(1)]],
        texture2d<uint>   arttab  [[texture(2)]], texture2d<uint>  sprtab  [[texture(3)]],
        texture2d<float>  albedo  [[texture(4)]], texture2d<float> emis    [[texture(5)]],
        texture2d<ushort> aux     [[texture(6)]], texture2d<float> sprites [[texture(7)]],
        texture2d<float>  spremis [[texture(8)]], texture2d<float> prof    [[texture(9)]],
        texture2d<ushort> stops   [[texture(10)]])
{
    Res R = { &u, cells, rooms, arttab, sprtab, aux, albedo, emis, sprites, spremis, prof };
    uint4 F = v.F;
    uint kind = (F.y >> 16) & 255u, base = F.y & 0xffffu;
    float S = float(u.grid.w), lod = u.view.w;
    float over_ = float((F.z >> 8) & 255u);
    if (kind <= 2 || kind == FIX_CARLIFT) {
        if (v.lp.y < over_) {                       /* the top, above the highest floor, standing on it */
            uint4 r = rect(artrec(R, base + 1u), 0);
            float2 p = float2(v.lp.x * S, (v.lp.y - over_) * S + float(r.w));
            float4 c = atl(R, r, p, lod);
            return float4(outdoors(R, c.rgb, float3(0)), c.a);
        }
        int top = int((F.x >> 9) & 127u), rows = int((F.x >> 24) & 127u);
        int row = top + int(floor((v.lp.y - over_) / CELL.y));
        float fy = gmod(v.lp.y - over_, CELL.y);
        float2 p = float2(v.lp.x * S, fy * S);
        if (row >= top + rows - 1) {                /* the pit, under the lowest floor */
            uint4 r = rect(artrec(R, base + 2u), 0);
            if (p.y >= float(r.w)) return float4(0);
            float4 c = atl(R, r, p, lod);
            return float4(outdoors(R, c.rgb, float3(0)), c.a);
        }
        uint s = uint(stops.read(uint2(clamp(row, 0, 127), F.z & 63u)).r);
        uint a = (s & 1u) != 0 ? base + 4u + ((s >> 4) & 3u) : base + 3u;
        float4 body = atl(R, rect(artrec(R, base), 0), p, lod);
        uint4 r = rect(artrec(R, a), 0);
        float4 front = atl(R, r, p, lod);
        float4 g = (artrec(R, a).w & A_EMIT) != 0 ? atl_emis(R, r, p, lod) : float4(0);
        float calling = (s & 6u) != 0 ? 1.0 : 0.25;
        float3 c = over(outdoors(R, body.rgb, float3(0)),
                        float4(outdoors(R, front.rgb, g.rgb * calling), front.a));
        return float4(c, max(body.a, front.a));
    }
    uint4 A = artrec(R, base);
    uint fr = (A.w & A_ANIM) != 0 ? uint(u.clock.y * 8.0) : 0u;
    uint4 r = rect(A, fr);
    float2 sz = float2(float((F.x >> 16) & 255u) * CELL.x, float((F.x >> 24) & 127u) * CELL.y + over_);
    float2 p = v.lp / sz * float2(r.zw);
    float4 c = atl(R, r, p, lod);
    float4 g = (A.w & A_EMIT) != 0 ? atl_emis(R, r, p, lod) : float4(0);
    return float4(outdoors(R, c.rgb, g.rgb), c.a);
}
```

(Unused texture members of `Res` are left default-constructed; if the
Metal compiler on the oldest macOS we support objects, the fixture pass
binds the P1 set at the same indices and fills `Res` whole.)

### 7.8 P3, movers -- GLSL ES 3.00

```glsl
#version 300 es
/* tw_mov_vs: a mover, its feet at (x, y), its sprite's size from the table */
precision highp float; precision highp int; precision highp usampler2D;
layout(std140) uniform TwU { /* as P1 */ ... } U;
uniform usampler2D movers, sprtab;
flat out uvec4 M;
flat out uvec4 SP;
out vec2 uv;
out vec2 at;                                     /* units, for BEHIND */
void main() {
    int i = min(gl_InstanceID, U.mode.w - 1);
    uvec4 mv = texelFetch(movers, ivec2(i & 255, i >> 8), 0);
    uvec4 s = texelFetch(sprtab, ivec2(int(min(mv.z & 1023u, uint(U.counts.z) - 1u)), 0), 0);
    vec2 feet = vec2(mv.xy) * 0.25;
    vec2 sz = vec2(float(s.z & 0xffffu), float(s.z >> 16)) * 0.25;
    vec2 k = vec2(float(gl_VertexID & 1), float(gl_VertexID >> 1));
    vec2 p = feet + (k - vec2(0.5, 1.0)) * sz;
    vec2 world = vec2(U.grid.xy) * vec2(4.0, 16.0);
    vec2 t = U.box.xy + p / world * U.box.zw;
    gl_Position = vec4(t.x / U.view.x * 2.0 - 1.0, 1.0 - t.y / U.view.y * 2.0, 0.0, 1.0);
    uv = vec2((mv.z & 1024u) != 0u ? 1.0 - k.x : k.x, k.y);
    at = p;
    M = mv; SP = s;
}
```

```glsl
#version 300 es
/* tw_mov */
precision highp float; precision highp int; precision highp usampler2D; precision highp sampler2D;
layout(std140) uniform TwU { /* as P1 */ ... } U;
uniform usampler2D cells, sprtab;
uniform sampler2D sprites, spremis;
flat in uvec4 M;
flat in uvec4 SP;
in vec2 uv;
in vec2 at;
out vec4 colour;
/* spr, outdoors as P1 */
void main() {
    if ((M.z & 2048u) != 0u) {                   /* BEHIND: hidden by any room */
        ivec2 c = ivec2(floor(at / vec2(4.0, 16.0)));
        if (c.x >= 0 && c.y >= 0 && c.x < U.grid.x && c.y < U.grid.y &&
            texelFetch(cells, c, 0).x != 0u) discard;
    }
    uint fr = (SP.w & 16u) != 0u ? uint(U.clock.y * 8.0) : (M.w & 15u);
    vec4 g;
    vec4 a = spr(M.z & 1023u, fr, uv, U.view.w, g);
    if ((SP.w & 64u) != 0u) {                    /* a person: the tint layer's weights */
        vec4 w = g;
        g = vec4(0.0);
        if ((M.w & 0x8000u) != 0u)
            a.rgb = a.rgb * (1.0 - w.r - w.g - w.b)
                  + (TOPS[(M.w >> 4) & 15u] * w.r + SKINS[(M.w >> 10) & 3u] * w.g + LEGS[(M.w >> 8) & 3u] * w.b) * a.a;
    }
    if (a.a <= 0.0 && g.a <= 0.0) discard;
    vec3 c = (M.z & 4096u) != 0u && U.counts.w >= 1
           ? a.rgb * mix(LAMP, vec3(1.0), U.misc.x) + g.rgb * (1.0 - U.misc.x)    /* lit inside */
           : outdoors(a.rgb, g.rgb);
    colour = vec4(c, a.a);
}
```

The BEHIND test samples the cell under each fragment (`at` interpolated),
so a cloud passes behind the tower's edge exactly.  `TOPS`, `LEGS` and
`SKINS` are constant tables in both languages (as the Metal text below);
`spr` reads the emission picture for a TINT sprite as for an EMIT one,
since that is where the tint layer is packed.

### 7.9 P3, movers -- Metal

```metal
/* ---- P3, the movers (design 7.9) ------------------------------------------ */
constant uint SPR_TINT = 64, P_TINTED = 0x8000;
constant float3 TOPS[16] = {
    float3(0x2f, 0x5d, 0x50) / 255.0, float3(0x3b, 0x5b, 0x8a) / 255.0, float3(0x6b, 0x5a, 0x7e) / 255.0,
    float3(0x7a, 0x6a, 0x4f) / 255.0, float3(0x4a, 0x4f, 0x57) / 255.0, float3(0xb9, 0xa3, 0xd6) / 255.0,
    float3(0x2f, 0x4a, 0x7a) / 255.0, float3(0xe9, 0xee, 0xf2) / 255.0, float3(0xff, 0x5f, 0xa2) / 255.0,
    float3(0xd8, 0x19, 0x19) / 255.0, float3(0), float3(0), float3(0), float3(0), float3(0), float3(0) };
constant float3 LEGS[4] = { float3(0x3b, 0x42, 0x50) / 255.0, float3(0x2e, 0x34, 0x40) / 255.0,
                            float3(0xa3, 0x3a, 0x6c) / 255.0, float3(0x8a, 0x10, 0x10) / 255.0 };
constant float3 SKINS[4] = { float3(0xf1, 0xc2, 0x7d) / 255.0, float3(0xe0, 0xac, 0x69) / 255.0,
                             float3(0xc6, 0x86, 0x42) / 255.0, float3(0x8d, 0x55, 0x24) / 255.0 };

struct TwM { float4 pos [[position]]; float2 uv; float2 at; uint4 M [[flat]]; uint4 SP [[flat]]; };

vertex TwM tw_mov_vs(uint id [[vertex_id]], uint inst [[instance_id]],
                     constant TwU &u [[buffer(0)]],
                     texture2d<ushort> movers [[texture(11)]], texture2d<uint> sprtab [[texture(3)]])
{
    int i = min(int(inst), u.mode.w - 1);
    uint4 mv = uint4(movers.read(uint2(i & 255, i >> 8)));
    uint4 s = sprtab.read(uint2(min(mv.z & 1023u, uint(u.counts.z) - 1u), 0));
    float2 feet = float2(mv.xy) * 0.25;
    float2 sz = float2(float(s.z & 0xffffu), float(s.z >> 16)) * 0.25;
    float2 k = float2(id & 1, (id >> 1) & 1);
    float2 p = feet + (k - float2(0.5, 1.0)) * sz;
    float2 world = float2(u.grid.xy) * CELL;
    float2 t = u.box.xy + p / world * u.box.zw;
    TwM o;
    o.pos = float4(t / u.view.xy * float2(2, -2) + float2(-1, 1), 0, 1);
    o.uv = float2((mv.z & 1024u) != 0 ? 1.0 - k.x : k.x, k.y);
    o.at = p;
    o.M = mv; o.SP = s;
    return o;
}

fragment float4 tw_mov(TwM v [[stage_in]], constant TwU &u [[buffer(0)]],
        texture2d<ushort> cells   [[texture(0)]], texture2d<uint>  rooms   [[texture(1)]],
        texture2d<uint>   arttab  [[texture(2)]], texture2d<uint>  sprtab  [[texture(3)]],
        texture2d<float>  albedo  [[texture(4)]], texture2d<float> emis    [[texture(5)]],
        texture2d<ushort> aux     [[texture(6)]], texture2d<float> sprites [[texture(7)]],
        texture2d<float>  spremis [[texture(8)]], texture2d<float> prof    [[texture(9)]])
{
    Res R = { &u, cells, rooms, arttab, sprtab, aux, albedo, emis, sprites, spremis, prof };
    if ((v.M.z & 2048u) != 0) {                     /* BEHIND: hidden by any room */
        int2 c = int2(floor(v.at / CELL));
        if (c.x >= 0 && c.y >= 0 && c.x < u.grid.x && c.y < u.grid.y &&
            cells.read(uint2(c)).x != 0) discard_fragment();
    }
    uint fr = (v.SP.w & 16u) != 0 ? uint(u.clock.y * 8.0) : (v.M.w & 15u);
    float4 g;
    float4 a = spr(R, v.M.z & 1023u, fr, v.uv, u.view.w, g);
    if ((v.SP.w & SPR_TINT) != 0) {                 /* a person: the tint layer's weights */
        float4 w = g;
        g = float4(0);
        if ((v.M.w & P_TINTED) != 0) {
            float3 top = TOPS[(v.M.w >> 4) & 15u], legs = LEGS[(v.M.w >> 8) & 3u], skin = SKINS[(v.M.w >> 10) & 3u];
            a.rgb = a.rgb * (1.0 - w.r - w.g - w.b) + (top * w.r + skin * w.g + legs * w.b) * a.a;
        }
    }
    if (a.a <= 0.0 && g.a <= 0.0) discard_fragment();
    float3 c = (v.M.z & 4096u) != 0 && u.counts.w >= 1
             ? a.rgb * mix(LAMP, float3(1.0), u.misc.x) + g.rgb * (1.0 - u.misc.x)
             : outdoors(R, a.rgb, g.rgb);
    return float4(c, a.a);
}
```

### 7.10 The host side

**PC (rosgd-present.c), beside `draw_tiles`:**

```c
static struct {
    uint32_t live_backing, art_backing;  ino_t live_ino, art_ino;
    const uint8_t *live, *artmem;  size_t live_size, art_size;
    uint32_t frame, art_gen, gen[8], room_gen[16];
    int have;
    GLuint field, fix, mov, ubo;                 /* programs and the uniform buffer */
    GLuint cells, rooms, arttab, sprtab, fixtab, stops, movers;
    GLuint albedo, emis, aux, sprites, spremis, prof;
    struct ros_tower head;                       /* its header only */
} TW;

static int  tw_take(const struct ros_surf_tower *d);  /* map both, seqlock copy of the header,
                                                         then each section whose gen moved
                                                         (glTexSubImage2D of those rows);
                                                         the art when art_gen moves:
                                                         glTexSubImage2D + glGenerateMipmap */
static void draw_tower(const struct ros_surface *e, int H, float s)
{
    /* programs once (rich, else classic; a pass that fails is left out);
     * tw_take; the uniforms (tw_light for the sky; tpp and lod from the box);
     * glBufferSubData(ubo); bind units 5-15; then for each visible rect:
     *   scissor; P1: glDrawArrays(GL_TRIANGLE_STRIP, 0, 4) with vsh;
     *   P2: glDrawArraysInstanced(..., nfixtures); P3: (..., nmovers) */
}
```

The uniform block is bound with `glUniformBlockBinding` and
`glBindBufferBase(GL_UNIFORM_BUFFER, 0, ubo)`. Attribute-less instanced
draws are legal in GLES 3.0 (the default vertex array).

**Mac (metal.m), beside `surf_tiles`:**

```objc
static void surf_tower(id<MTLRenderCommandEncoder> enc, bool shot, CGSize target,
                       uint32_t xres, uint32_t yres)
{
    /* tw_first (the first ROSGD_SURF_TOWER icon, both blocks inside the
     * backings region); tw_take (as GL, replaceRegion; per-frame sections
     * into the next of three textures, section 8.4); TwU filled the same
     * way; for each rect: setScissorRect and u.clip, then
     *   tw_field (4 vertices), tw_fix (4, nfixtures instances),
     *   tw_mov (4, nmovers instances) */
}
```

and `surf_face_due` learns ROSGD_SURF_TOWER's `rate` as it knows the tile
map's.

---------------------------------------------------------------------------

## 8. Keeping the GL and Metal shaders in step

### 8.1 The rule

Same inputs, same arithmetic, same order of operations, same names. The
uniform struct, texture list, sampler state, constants and functions are
described once (this document, then `tw_shaders.txt`) and expressed twice.
The tile map's shaders were kept in step by hand, function for function
against a CPU twin (`emu.py` and `tl_map_hd.glsl`, which their comments
name; those files are not in this tree). The tower keeps its twin and its
shader source in the tree.

### 8.2 One body, two preludes

The shaders are written once, in `rosgd/ports/tower/gpu/tw_body.inc`, in a
subset both languages share once a prelude has defined a few words:

| In the body | GLSL prelude | Metal prelude |
| --- | --- | --- |
| `vec2 vec3 vec4 ivec2 uvec4 ...` | native | `typedef float2 vec2;` and so on |
| `FETCH(t, p)` | `texelFetch(t, p, 0)` | `uint4(R.t.read(uint2(p)))` |
| `LOD(t, smp, uv, l)` | `textureLod(t, uv, l)` | `R.t.sample(smp, uv, level(l))` |
| `OUT(T)` | `out T` | `thread T &` |
| `RES` / `RES_` | nothing / nothing | `thread const Res &R` / `R, ` |
| `CONST` | `const` | `constant` |
| `U.box` | the std140 block's instance | `(*R.u).box` |
| `mod(a, b)` | native | `gmod`, as GLSL defines it |
| `discard` | `discard` | `discard_fragment()` |

Every function takes `RES_` as its first parameter where it touches
uniforms or textures (`vec3 sky(RES_ vec2 m)`). The entry points
(`main`, `tw_field`, the vertex shaders) are written per API, in
`tw_gl.inc` and `tw_metal.inc`; they are short and are the only place the
two may differ.

`tools/twgen.py` concatenates prelude, body and entry points, checks the
results compile (glslangValidator for ES 3.00; `xcrun metal` on a Mac),
and writes them into rosgd-present.c and metal.m between marker comments
(`/* twgen: tw_field begin */ ... end */`), with `tw_light()` and the
`TwU` struct. `twgen.py --check` fails if either file's embedded text
differs from what it would write, so a hand edit to one side is caught.

### 8.3 What must match outside the shader

| Matter | GL | Metal |
| --- | --- | --- |
| albedo/emis/sprites sampling | `GL_LINEAR_MIPMAP_LINEAR`, `GL_LINEAR`, clamp to edge | `TRI` |
| profile sampling | `GL_LINEAR`, repeat S, clamp T | `PROF` |
| mipmaps | `glGenerateMipmap` | `generateMipmapsForTexture:` (blit) |
| max level | `GL_TEXTURE_MAX_LEVEL` 3 | texture made with 4 levels |
| blending | premultiplied `ONE, ONE_MINUS_SRC_ALPHA` | same factors (`tl_pipe`) |
| fast maths | Mesa does not reassociate by default | **turn it off**: `MTLCompileOptions.mathMode = MTLMathModeSafe` (macOS 15), else `fastMathEnabled = NO`. The tile map's library is built with `options:nil`, i.e. fast maths on; the tower must not copy that. |
| colour space | linear bytes, no sRGB views | `BGRA8Unorm`, not the sRGB format |
| pixel centres | `gl_FragCoord` .5, y flipped by `clock.w` | `[[position]]` .5, y down |

### 8.4 Features one API makes hard for the other

- **Instance data.** GLES 3.0 has no storage buffers (they are 3.1), so
  instance records come from textures by `gl_InstanceID`. Metal would
  rather read a buffer; it reads the same textures, so the body is shared.
  The cost is nil at these sizes.
- **Uniform size.** Metal's `setFragmentBytes` takes at most 4 KB; TwU is
  480 bytes. Mover data (64 KB) cannot go that way, which is another
  reason it is a texture.
- **Updating a texture in flight.** GL orders `glTexSubImage2D` against
  earlier draws for us. Metal's `replaceRegion` writes at once, even
  while the GPU reads the last frame from the same texture. The tile map
  does this today and gets away with it because its map changes slowly;
  the tower's movers and stops change every frame, so on Metal the
  per-frame textures (movers, stops, the header-driven room chunks) are a
  ring of three, advanced each frame, as the Mac's quad buffers already
  are (`METAL_RING`).
- **No mutable globals in Metal.** The body passes everything as
  parameters; GLSL is happy with that too.
- **Integer textures.** Both have them; neither filters them. All tables
  are read with integer fetches, so this costs nothing. RG8UI, R8UI,
  R16UI, RGBA16UI and RGBA32UI are all core in GLES 3.0 and present on
  every Metal device.
- **Texture arrays.** GLES 3.0 has them (`sampler2DArray`, at least 256
  layers) and Metal has `texture2d_array`, but every layer must be the
  same size, which fits rooms 1 to 31 cells wide badly; a shelf-packed 2D
  atlas suits both and avoids the question.
- **Mipmapped non-power-of-two textures.** Fine in GLES 3.0 and Metal.
- **`gl_VertexID` and attribute-less draws.** Fine in GLES 3.0; Metal's
  `vertex_id` the same. No base instance in GLES 3.0, which we do not
  need.
- **`unpackUnorm4x8`.** Not in GLSL ES 3.00; the body uses its own
  `unpack8` on both sides.
- **Derivatives.** Not used; the level of detail is a uniform, so quads of
  pixels that straddle two rooms (whose texture coordinates jump) cannot
  pick a wrong level, and the two APIs' different derivative rules do not
  matter.
- **Precision.** GLSL needs `highp` throughout (map positions reach 1920
  units with sub-texel fractions at S = 4); Metal's `float` is 32-bit.
  Integer wraparound on `uint` multiplication is defined in both.
- **Bit exactness.** It is not attainable: trilinear weights, `exp2`,
  `sin` and the `smoothstep`s differ in the last bits between Mesa's
  back ends and Apple's. Classic at lod 0 (4x) is expected to match the
  CPU twin exactly; the rest within the tolerance of 8.5.

### 8.5 The twin and the parity test

`tower-surface/src/twin.rs` implements the body's functions in Rust, one
for one, same names, using `f32`, and renders a block to a PNG for a given
box, hour, seconds and quality. It samples as GL defines it (bilinear,
texel centres at .5, clamp to edge, mip levels made by a rounded 2 x 2 box
filter, linear between levels) and writes colours as `round(x * 255)`. So
far it draws P1 classic; the other qualities, P2 and P3 come with their
units. The fixtures are towers built from
upstream's trace snapshots (harness/dump-snapshots.mjs) at fixed ticks,
plus a synthetic tower with every room type in every state, fire on two
floors, an evaluation view, and a hundred people.

- PC: the presenter's scratch mode draws the fixture and `screenshot_to`
  writes the frame; compared with the twin.
- Mac: the `shot` pipelines (as the tile map's) draw into an RGBA texture
  and read it back; compared with the twin.
- Pass: classic at 4x, no channel differs; everything else, at most 2/255
  on 99.9 per cent of pixels and 8/255 on all. A failure writes a
  difference picture.

The check runs on every change to the body, on both machines, before the
change lands.

---------------------------------------------------------------------------

## 9. Performance budget

### 9.1 The targets

| Target | GPU | Path | fp32 peak | Texturing | Memory |
| --- | --- | --- | --- | --- | --- |
| Mac Pro 2013 | FirePro D300 (Pitcairn, 20 CU, 850 MHz); D500/D700 more | GLES 3.0 on radeonsi | about 2.0 TFLOPS | about 68 Gtexel/s | 160 GB/s GDDR5 |
| N97 mini PC | Intel UHD, 24 EU Xe-LP, 1.2 GHz | GLES 3.0 on iris | about 0.46 TFLOPS | about 14 Gtexel/s | one channel DDR4/5, about 25-38 GB/s, shared |
| Mac mini 2018 | Intel UHD 630, 24 EU Gen9.5, 1.15 GHz | Metal | about 0.44 TFLOPS | about 14 Gtexel/s | two channels DDR4-2666, about 42 GB/s, shared |

(Peak figures are the vendors' arithmetic; the Intel parts are the
constraint, the FirePro has four times their margin.)

### 9.2 The load

Only P1 covers the window; P2 and P3 cover a few per cent of it. The pixel
count is the window's in target pixels:

| Window | Target pixels | at 30 fps | at 60 fps |
| --- | --- | --- | --- |
| 2560 x 1440 on a plain screen | 3.7 M | 111 M/s | 221 M/s |
| 2560 x 1440 points on a 4K screen at 2x (mini) | 8.3 M (3840 x 2160 visible) | 249 M/s | 498 M/s |
| 2560 x 1440 points on 5K | 14.7 M | 442 M/s | 885 M/s |

The tower at 1:1 is 1500 x 1920 target pixels, so at 1:1 in a large
window part of the window is the back colour (the pass discards there and
costs almost nothing). Zoom does not change the per-pixel cost, except
that 1:1 reads mip level 2, which is cache-friendlier than level 0 at 4x.

Estimated per pixel of P1 (worst case: built area at night):

| Quality | Fetches | ALU ops | At 3.7 M px, 60 fps, on a UHD 630 / N97 |
| --- | --- | --- | --- |
| classic | 6-8 (cell, room, art, picture, piece x 2) | ~80 | ~4% ALU, ~12% texturing |
| smooth | 10-12 (+ aux, emission, profile) | ~150 | ~8% ALU, ~18% texturing |
| rich | 18-22 (+ 3 shadow taps, 2 neighbours and their rooms, occupied picture, noise) | ~350 | ~18% ALU, ~33% texturing |

Sky pixels cost a third of that; underground pixels with no room less
than half.

Rule of thumb for planning: take these estimates twice over (dependent
fetches, divergent branches at room edges, Gen9's weaker integer rate),
and keep a frame's tower drawing under 8 ms on the Intel parts.

| Case | Intel parts | FirePro |
| --- | --- | --- |
| classic / smooth, any window, 60 fps | within budget | within budget |
| rich, 2560 x 1440 plain, 30 fps (the rate) | within budget (about 4-6 ms) | within budget |
| rich, 4K at 2x | 30 fps only; about 8-12 ms | within budget |
| rich, 5K at 2x | over budget: falls back to smooth | within budget |

### 9.3 Keeping within it

- **Rate.** Rich is drawn at its `rate` (20-30), as the tile map's;
  classic and smooth only when a frame comes (30 a second while the game
  runs, none while it is paused and nothing moves).
- **A measured fallback.** Both presenters time the tower's passes
  (Metal: the command buffer's GPU start and end; GL: `GL_EXT_disjoint_timer_query`,
  present on iris and radeonsi). If the average over 60 frames passes
  8 ms, the presenter draws one quality lower and says so in its log; it
  tries the higher again after a change of window size or zoom.
- **Scissoring.** Only visible rectangles are drawn (the Wimp's).
- **Uploads.** About 30-100 KB a frame (movers, stops, a room chunk);
  the art (about 36 MB) once, with its mipmaps made on the GPU. On the N97
  the first upload takes about 50 ms; the engine shows a frame of sky
  first.
- **Memory.** About 36 MB of pictures (albedo 16, half-size emission 4,
  aux 8, sprites and their emission 8) plus 9 MB of mip levels and under
  1 MB of tables: under 50 MB of GPU memory on each target.
- **Contingency.** If measurement shows the rich pass too dear on the
  N97, the static part of P1 (everything but cockroaches, fire and the
  screens) is drawn into a target-sized texture only when the rooms, the
  hour (to the game minute), the zoom or the scroll change, and copied
  each frame, with the animated parts drawn over it. Not planned unless
  needed: scrolling would redraw it every frame anyway.
- **Compile time.** The rich program is the largest the presenters have
  built; radeonsi and iris compile it in a fraction of a second, once,
  and Mesa's shader cache keeps it. The classic program is the fallback
  if it will not build.

---------------------------------------------------------------------------

## 10. What the art must provide

The art tree (rosgd/ports/tower/art, README.md and render.py) already sets
the rules this design depends on: a cell 4 units by a floor 16; every
room's `viewBox` its exact size; `name.svg` the picture by day,
`name.emit.svg` what glows at night; frames as `name.0.svg ...`; no
lettering. This section adds what the shaders need beyond that, so that a
separate art effort can produce it.

### 10.1 Resolution and the atlas

- Rendered at scale 4 (render.py's `--scales 4`): a cell 16 x 64 texels.
  This is the atlas the presenters use; 1:1 reads it at mip level 2.
  Scale 8 is the Mac's Retina set (12.2).
- The packer is `towerpack`, a binary of the `tower-surface` crate
  (rosgd/ports/tower/surface; the library is `pack.rs`, so the engine and
  the twin's tests pack in memory with the same code). It reads
  art/out/<scale> (4; 8 for the Mac's Retina set) and the signs of one
  language (`--lang`), shelf-packs the pictures (tallest first) into an
  atlas 1024 x S texels wide with a gutter of one unit (S texels; 4 at
  scale 4) round each picture, its edge texels repeated, both sides
  rounded to multiples of 8 so mip levels 0-3 halve exactly, and writes
  the art block: albedo, emission at half size, sprites and their
  emission, the art and sprite tables, the sky ramp and the weekday and
  weekend lamp profiles. It checks every table entry lies inside its
  atlas before writing, and the same art gives the same bytes; `--dump`
  writes the atlases as PNG and `--manifest` every entry's number, name
  and place.
- Measured, at scale 4: albedo 4096 x 880 (150 entries), sprites 2048 x
  928 (359): a block of 33 MB. At scale 8: 8192 x 1760 and 4096 x 1848,
  133 MB, more than the engine buffer's 44 MB for the art (12.3: the
  buffer grows for it; or the Mac reads the art from a file).
- Rooms, building pieces, world strips (soil, rock, street, skylines),
  shaft pieces, stairs and escalators (P2 reads them through the art
  table and the albedo atlas) and the fire pieces (front pieces) go into
  the room atlas; anything drawn as a free quad (people, cars and their
  doors and loads, the car lift's platform, clouds, sun, moon, stars,
  other events, vehicles, signs, cockroaches) into the sprite atlas.
- `name.N` files are frames of one entry (laid side by side; ANIM where
  the clock runs them: fire, construction, the roof beacon, escalators,
  events, vehicles, the cockroach) for the families `pack.rs` names, and
  otherwise variants, consecutive entries the engine picks between
  (office.0-4, the landings' door steps, dust). People facing left are
  not packed (the shader flips); the people's `.tint` layers are not
  packed yet (the movers' pass, unit 10, will add a tint picture).
- Absent layers: no picture has material, group, slot or height layers
  yet.  Until it does, the glass is found by colour: the crate's
  data/glass.txt (art/src/glass.txt when the art has one) names, picture
  by picture, the colours of the panes that look out (office.0, condo.0,
  restaurant.4, recycling), and towerpack writes material 2 on those
  texels of the aux picture and sets AUX on their entries.  With no glass
  anywhere the aux picture is left out (`ROS_TWA_F_AUX` clear) and the
  presenter binds a 1 x 1 zero texture in its place.  The aux picture
  adds 7 MB at scale 4 (the block is 40 MB, inside the 44 the buffer
  leaves it).

### 10.2 Layers, per picture

| Layer | File | Required | What the shader does with it |
| --- | --- | --- | --- |
| albedo | `name.svg` | yes | the picture by day, as lit by even daylight with lamps off |
| emission | `name.emit.svg` | where anything glows | added at night, times the room's lamp state; its colours are the light's colours |
| material | `name.mat.svg` | optional, rich mode | flat key colours, one per material: wall, floor, glass (an exterior window: shows the sky), lamp, solid object, sign, metal, screen (flickers while SHOWING); 16 at most |
| height | `name.mat.svg`'s second set of keys, or `name.height.svg` in greys | optional, rich mode | 0-15: how far a thing stands from the back wall; casts the short contact shadows |
| lamp groups | `name.groups.svg` | optional | key colours 1-15 marking which lamps switch together; without it the whole room is one group |
| slots | `name.slots.svg` | optional | key colours 1-15 where a person would be |
| occupied | `name.full.svg` (with `name.full.emit.svg`) | with slots | the same room with people in every slot |

The key-colour palette for `.mat`, `.groups` and `.slots` is fixed in
STYLE.md (to be added) and checked by render.py: any other colour in
those layers is an error.

### 10.3 The pieces this design names

Besides the rooms already in art/src/rooms:

- **Fixed entries:** skyline far and near (world/skyline-far, -near:
  repeating strips standing on the ground); the banners' ribbon (two
  sprites, the words drawn by the engine); cockroach, four frames.
- **Shafts, per kind (standard, express, service):** body, top, pit,
  skip, landing door frames 0-3 (with emission for the call lamps), in
  that order. Already drawn (art/src/transport).
- **Cars, per kind:** car, doors 0-3, load 0-3 (sprites). Already drawn.
- **People** (art/src/people, to come): a figure about 3 x 10 units, per
  kind (office worker, resident, hotel guest, shopper, diner, cinema-goer,
  staff, security, VIP), standing, four walking frames, waiting, riding;
  low, medium and high stress as separate sprites (upstream colours
  medium pink and high red); facing right only (the shader flips).
- **Missing rooms:** metro (30 x 3), cathedral (28 x 5), plain floor's
  per-cell piece exists (building/floor.svg).
- **Sky ramp:** 48 columns (half hours) of zenith, horizon, ambient and
  misc (daylight, stars, sun, glass reflection), supplied as a small PNG
  (48 x 4, art/src/world/sky-ramp.png) the artist paints; until then
  towerpack's default: upstream's day and night colours, with a dawn
  (05:30 to 07:30: a mauve, then a warm horizon) and a dusk (18:30 to
  21:30: warm, orange, then a dusky purple) between them, keyed by hour
  in pack.rs (`default_ramp`).
- **Lamp profiles:** a table, not a picture: 16 profiles x 24 hours of
  chances, weekday and weekend, in a text file (art/src/lamps.txt when the
  art supplies one; until then the crate's data/lamps.txt, same format).

### 10.4 In the meantime: the classic look from upstream's art

Classic mode is upstream's look and needs nothing new:

- The room SVGs already in art/src/rooms (taken from tower-together and
  fixed to the slot sizes) render at scale 4 into the atlas as they are;
  no emission, no aux.
- tower-together's own `apps/client/public/rooms` can be packed directly
  if a room is missing from the art tree: render.py's slot check aside,
  their viewBoxes are 20 units a cell (hotels 80), so towerpack renders
  them with resvg to the slot's size in texels, which scales them
  correctly.
- `underground2.webp` (1778 x 592) as the soil until the soil pieces are
  in: a back piece, REPEAT, GROUND-anchored.
- Cathedral and metro, with no art: SOLID colour entries (upstream's
  colour table) until drawn.
- People: upstream's `sim-*.svg` (6 x 20 viewBox, drawn 3 x 10 units).
- Elevator cars: upstream draws a yellow rectangle; a SOLID sprite until
  the transport art is packed.

tower-together is MIT (LICENSE.md, Patrick Hulin); its notice goes in
the art block's credits and the application's About box. Its two raster
files `office.png` and `fast-food.jpg` are not used by its own renderer and
are not used here.

---------------------------------------------------------------------------

## 11. Implementation plan

Each unit is small, lands on its own, and is proved before the next.

1. **tower.h and its Rust mirror.** The structs, constants and static
   asserts; `#[repr(C)]` mirror (the `tower-surface` crate, which the
   engine will use) with size and offset tests.
   *Proof:* both build; a test writes a block in Rust and reads every
   field back in C.
2. **towerpack.** art/out/4 to an art block: shelf packing, gutters, half
   emission, aux from optional layers, tables, ramp and profiles. (A
   binary of `tower-surface`; scale 8 and the signs' language by flags.)
   *Proof:* it dumps the atlas as PNG; every table entry lies inside its
   atlas; re-running gives the same bytes.
3. **The twin, classic.** twin.rs renders P1 classic from a block.
   *Proof:* pictures of three upstream snapshot towers at noon and
   midnight, looked at beside upstream's renderer's rules (upstream keeps
   no screenshots of its fixtures); the same bytes on two runs. (Done in
   tower-surface: tests/twin.rs builds the fixtures as sim/tests do and
   publishes them through tests/seed, the seed of unit 4; at 4x a room's
   pixels equal its picture's texels exactly.)
4. **The engine's publisher.** WorldState to cells, rooms, fixtures,
   stops; the section gens. *Proof:* unit tests on the mapping tables in
   6.3; every non-empty cell's room covers it; gens move only when a
   section's bytes do.
5. **The broker.** `ROSApps_Tiles` learns kind 7 (or a new
   `ROSApps_Tower`), the two backings, the per-section copy, the bind.
   *Proof:* on the box, a test engine publishing a fixed block; the
   presenter logs whole frames; a deliberately slow writer never tears.
6. **PC, P1 classic in GLSL.** `tw_take`, `draw_tower`, the field program.
   *Proof:* the scratch-mode screenshot of each fixture equals the twin's
   at 4x, all channels.
7. **Mac, P1 classic in Metal.** `surf_tower`, the same fixtures through
   the shot pipelines. *Proof:* equal to the twin at 4x.
8. **twgen.** The shared body, the preludes, `--check`; units 6 and 7 move
   onto it. *Proof:* both still equal the twin; an edit to one embedded
   copy fails the check.
9. **P2 fixtures, both APIs and the twin.** *Proof:* the fixture's
   shafts, served and unserved floors, doors and stairs, against the twin
   on both machines.
10. **P3 movers, both APIs and the twin.** Instancing, BEHIND, flips,
    frames; the Metal ring of three. *Proof:* 8,192 movers against the
    twin; the Mac shows no tearing of movers under a GPU capture.
11. **Smooth.** Profiles, lamps, emission, the sky ramp, windows showing
    the sky, the skylines. *Proof:* twin parity within tolerance at 1:1
    and 4x at 06:00, 12:00, 20:30 and 02:00.
12. **Rich.** Shading, contact shadows, spill, glass, flames, cockroaches,
    the sun. *Proof:* twin parity within tolerance; the fallback to
    classic when the rich program is made to fail.
13. **Views and overlays.** Evaluation, value view, selection, the tool's
    outline. *Proof:* the fixture's evaluation view against the twin; a
    pick in the window outlines the room under the pointer.
14. **Measurement and the fallback.** Timer queries; the 8 ms rule.
    *Proof:* a table of pass times on the Mac Pro, the N97 and the Mac
    mini at 2560 x 1440 (and the mini at 4K), zoom 1, 2 and 4, each
    quality; the fallback seen to engage on the mini at 5K.
15. **The real art.** Swap the interim art for the art tree's, layer by
    layer, as it lands; an art change at run time is a new art gen.
    *Proof:* contact sheet and in-window pictures of each room at each
    quality, by day and by night, on both machines.

Units 6 and 7 can run in parallel once 1 to 5 are in; 9 and 10 likewise.

Where it stands (25 October 2026): 1 to 7 are done; 9 and 10 are done in
Metal, in GLSL and in the twin.  The PC presenter (rosgd-present.c,
draw_tower) runs the GLSL of 7.4, 7.6 and 7.8 on the 2018 Mac mini (Mesa
iris, UHD 630, a 4K panel showing a 1920 x 1080 desktop, so 1:1 is two
panel pixels a unit).  Against the twin (twincheck, classic), the fixture
towers Mixed and Sky office at noon and midnight at 1:1 and 4x, and dense
office on a busy morning at 1:1 and 4x, all lie within the tolerance of
8.5, at worst 1/255 (3/255 on one or two pixels among people); 6 to 13
per cent of pixels differ by 1/255.  Classic is not exact on Intel even
texel for texel (zoom 2x on that panel): the sky's gradient (a division,
which Mesa makes a reciprocal and a multiply on this GPU, and mix fused
into multiply-adds) and the premultiplied compositing of the fixtures and
movers round differently in the last bit.  Mesa has no switch for either
in GLSL ES 3.00.  The engine (ports/rosapps/tower-engine, movers.rs)
publishes what P3 draws: the cars at their smoothed floor with their doors
and load, the people waiting in queues at the landings (upstream's
layout, coloured by their trip's stress), and the walks the simulation
does not model (in from the street to the lobby's queue, from a car to a
room or out of the lobby), and the street's traffic.

Units 11 and 12 (25 October 2026, evening) are done in the twin, in Metal
and in GLSL.  The engine takes `quality N` (0 classic, 1 smooth, 2 rich)
and `hour H` (the clock held at any hour) from the shell; for rigs, a
file light.request beside the engine holds those, `speed N` and `fixture
N` (lighting.rs).  Both presenters build the rich program (`TW_Q` 2) and,
if it will not build, the classic one (`TW_Q` 0, the smooth and rich
parts left out by the preprocessor), then draw classic whatever is asked;
ROSGD_TOWER_FAIL_RICH=1 (or, on the PC, a file /run/tower-fail-rich)
makes the rich one fail.  Parity (twincheck) on the dense hotel and the
sky office, smooth and rich at 06:00, 12:00, 20:30 and 02:00: under QEMU
on the Studio at 1:1 and 4x, at worst 1/255 on at most 0.16 per cent of
pixels; on the Mac mini (iris) at 1:1 (two panel pixels a unit) and at
the largest zoom (eight), at worst 1/255 on 8 to 16 per cent.  With the
rich program made to fail and rich asked for, the Mac's picture equals
the twin's classic exactly at 4x; the mini's log shows the rich field
left out and the classic programs built.  One pixel among the people at
4x on the Mac differs by 1/255 in classic (the tint's arithmetic in P3),
so classic at 4x is exact on the Mac only where no tinted person stands.

---------------------------------------------------------------------------

## 12. Decisions on the open questions (25 October 2026)

1. **Zoom range.** Yes to a 1/2 overview (a whole tower in 750 x 960);
   the level of detail reaches 3.
2. **A Retina atlas.** Yes: a scale-8 atlas on the Mac.
3. **The engine buffer.** The broker's buffer grows for tower sessions if
   the art needs it.
4. **Walking people.** Yes: people walking along floors and seated in
   rooms are drawn, not only those queued for elevators (the emergency
   services, with medics riding the elevators, need it).
5. **The evaluation view.** A view the user switches on (`view` = 1).
6. **Interpolation.** The engine interpolates and publishes at 30 fps;
   the presenter takes it over only if that proves uneven.
7. **Signage, not a font.** The words (OPEN, CLOSED, FOR RENT, FOR SALE,
   SOLD, VACANCY, NO VACANCY, FULL ...) are pictures in art/src/signs/,
   per language, their letters drawn as shapes by our own lettering
   generator; the engine picks the sign picture as it picks any other.
   The engine carries no font.
8. **tower-together's art.** Covered by its MIT licence (the review found
   it the author's own work); its sounds are not, and are not used.
9. **Metal structs holding textures.** To be confirmed on the oldest
   macOS the Mac app supports; explicit parameters if refused.
10. **Lamp profiles by day.** Yes, weekday and weekend profiles from the
    start.
11. **People show traffic, not a census** (the user, 25 October 2026). The
    people drawn represent the tower's traffic; their numbers need not be
    exact. Queues, riders, walkers and crowds may be sampled, capped or
    synthesised (as the engine's walks to and from the lobby already are),
    so long as busy looks busy and quiet looks quiet. The simulation's own
    counts stay exact; only the picture is free.
