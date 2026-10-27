# Signs

The tower's words are pictures.  The engine carries no font
(docs/graphics-design.md, section 12 item 7): every sign is an SVG here,
one set a language, its letters drawn as shapes by our own lettering,
`letters.py`.  Nothing is taken from a font, the 1994 game or the web.

| File | What |
| --- | --- |
| `letters.py` | the alphabet and the layout (fit a string to a plate) |
| `gen.py` | every sign's style, size and colours; writes `<lang>/` |
| `<lang>.json` | the words, sign key -> text, for one language |
| `<lang>/*.svg` | the generated signs (`.emit.svg` beside the lit ones) |

Edit the generator or a table and run `python3 art/src/signs/gen.py`
(all languages, or `gen.py en fr`), then `python3 art/tools/render.py`.
Never edit the SVGs by hand.  `gen.py` prints each sign's cap height and
squeeze, flags any condensed below 0.8 or smaller than a 2.6-unit cap,
and fails if one would be under 2.4 (unreadable at 4 px a unit) or if a
letter is missing.

## The lettering

A squared sign-writer's hand: strokes of even weight with round ends and
joins, the round letters drawn as rounded rectangles (O, C, D, G, U,
and the figures), which hold their shape at 12 pixels a cap where ovals
blur.  Each glyph is a skeleton on a grid 10 high; the stroke is part of
the weight.

| Weight | Stroke (of the cap) | Width | Used for |
| --- | --- | --- | --- |
| regular | 0.165 | full | wall plates, neon, wide banners, lit boxes with room |
| bold condensed | 0.23 | x 0.74 | door cards, narrow banners, floor numbers, awnings |

A word too wide for its plate is condensed (the skeleton narrows, the
stroke does not) down to 0.68, and only then made smaller.  Spacing is
one stroke plus a tracking step, with kerning for the awkward pairs (AV,
AT, LT, LY, TA, P., L' ...).  The set: A-Z, 0-9, `& - ' . ! /`, the
capital sharp s, and any of A E I O U with acute, grave, circumflex or
diaeresis, A O N with tilde, Å, Ç and Ý (a base letter plus a mark).
Marks sit above the cap line and may use a little of a plate's padding.
`python3 letters.py > specimen.svg` draws the whole alphabet in both
weights.

## Sizes, anchors and colours

Sizes are in units (a cell 4, a floor 16) and are the same in every
language: a translation only changes the words.  Every picture is placed
by its top-left corner; "y" below is within the floor, 0 at the ceiling,
15.2 the walking line.  Each sign carries a `<!-- units: WxH -->` or
`<!-- slot: Nx1 -->` comment for render.py.

| Style | Signs | Size | Look |
| --- | --- | --- | --- |
| hanging banner | `for-rent-N`, `for-sale-N`, `sold-N`, `let-N`, N = 4 6 9 10 12 16 | the room's slot, N x 1 floor | a cloth band on two cords from the ceiling, straight across the room (N >= 9), lying diagonally (6), or a two-line placard on a V cord (4); transparent elsewhere |
| door card | `open`, `closed` | 14x9 | a card on a cord from a hook |
| neon | `open-neon` 16x8, `vacancy` 20x6, `no-vacancy` 24x6 | | tubes on a dark board; `open-neon` has a gold frame tube |
| lit box | `exit` 12x6, `full` 14x6, `hotel` 24x7, `cinema` 28x8, `now-showing` 32x8, `parking` 20x6, `metro` 16x6, `ambulance` 28x6, `police` 20x6, `fire` 16x6, `evacuate.0`/`.1` 24x7 | | a face lit from behind (exit, full, parking, metro, the services, evacuate) or an opaque face with lit letters (hotel, cinema); the cinema and now-showing are marquees with a border of lamps |
| wall plate | `lobby` 16x6, `reception` 22x6, `offices` 18x6, `party-hall` 32x6, `clinic` 20x6 (red cross), `security` 20x6, `staff-only` 20x6, `no-entry` 28x6 (no-entry disc), `car-lift` 16x9 (two lines), `sale` 12x6 | | a flat plate with a darker border (STYLE.md: Sign), in the room family's colours |
| hazard plate | `out-of-order` 14x10 | | yellow and black striped border, two lines |
| awning | `shop` 20x9, `cafe` 16x9, `restaurant` 28x9 | | a striped canopy over a valance carrying the name |
| floor plate | `floor-b10` .. `floor-b1`, `floor-g`, `floor-1` .. `floor-109` | 8x5 | dark plate, pale figures; basements slate blue, the ground floor gold |

Banners: cloth `#f0f0e8` with a red (`for-rent`) or blue (`for-sale`)
hem and letters; `sold` red with pale letters; `let` green.  The light
cloth, dark hem and drop shadow keep them readable over any room.

Signs whose pictures include a drop shadow (all but banners' cords and
the floor plates) are 0.4 wider and 0.5 taller than the plate itself.

## Which sign, where, when

| Room or state | Sign | Place |
| --- | --- | --- |
| office vacant (`office.vacant`) | `for-rent-9` | over the room, at its top-left |
| condo unsold (`condo.vacant`) | `for-sale-16` | over the room |
| condo just sold | `sold-16` (for a few game hours, then nothing) | over the room |
| office just let | `let-9` (a few game hours) | over the room |
| shop vacant (`retail.vacant`) | `for-rent-12` | over the room |
| other widths (4, 6, 10 cells) | the matching `-N` | over the room |
| shop, restaurant, café open by day | `open` card | on the shop front, x centred on the door, y = 4 |
| shop closed (night, or out of hours) | `closed` card | the same place |
| restaurant, fast food, café open at night | `open-neon` (+ emit) | on the wall, y = 2 |
| hotel with rooms free | `vacancy` (+ emit) | on the lobby wall at the hotel's floors, y = 2 |
| hotel full | `no-vacancy` (+ emit) | the same place, instead of `vacancy` |
| parking full / hotel full (alternative) | `full` (+ emit) | at the parking ramp, y = 2 |
| parking | `parking` | at the parking ramp's entrance |
| car lift | `car-lift` | beside the car lift's entrance, y = 3 |
| elevator stopped or broken | `out-of-order` | centred on the landing doors, y = 4 |
| every elevator and stair landing | `floor-<f>` | centred on the landing, y = 1.2 |
| lobby | `lobby`, `reception` | on the lobby wall, y = 2 |
| hotel | `hotel` (+ emit) | on the lobby wall below the hotel's floors |
| office floors | `offices` | on the lobby wall or the floor's first office |
| shop, café, restaurant fronts | `shop`, `cafe`, `restaurant` awnings | over the front, top at y = 0 |
| shop sale event | `sale` | in the shop window, y = 5 |
| cinema | `cinema` marquee; `now-showing` while a film runs | over the entrance, y = 0 of the upper floor |
| party hall | `party-hall` | on the wall by the door |
| medical centre | `clinic` | on the wall by the door |
| security office | `security` | on the wall by the door |
| service rooms (housekeeping, recycling) | `staff-only` | on the door |
| a floor or room not reachable | `no-entry` | at the landing |
| ambulance bay, emergencies | `ambulance`, `police`, `fire` | over the medical centre's bay / street level by the services' vehicles |
| fire or bomb evacuation | `evacuate.0`, `evacuate.1` | flash them every 500 ms on the lobby wall (frame 0 lit) |

Floor names follow the language's pattern: English `B1`..`B10`, `G`,
`1`..`109`; French `-1`..`-10`, `RC`, `1`..`109`.  The file names
(`floor-b3`, `floor-g`, `floor-42`) are the same in every language.

## Night

`.emit.svg` beside a sign is its light, as for every other picture
(STYLE.md: night layers).  Show it only while the sign is lit: neon and
the vacancy signs while the room or hotel is open, `exit` always, the
lit boxes from dusk.  Neon by day shows its tubes in their colour; the
emit layer adds the bright tube, a hot core and two halos.  Plates,
cards, banners, awnings and floor plates have no light of their own.

## Adding a language

1. Copy `en.json` to `<lang>.json` (an ISO 639-1 code: `de.json`).
2. Translate every value.  `\n` forces a line break where a sign has two
   lines (the 4-cell placard, `out-of-order`, `car-lift`); elsewhere the
   generator splits at the most even space when it needs to.  Keep the
   `{n}` in the three `floor-` patterns.
3. Run `python3 art/src/signs/gen.py <lang>`.  If it reports a missing
   letter, add the glyph (or the accented pair) to `letters.py`; if it
   reports a sign too small to read, choose a shorter word (signs say
   `LIBRE`, not `CHAMBRES LIBRES`) rather than changing the sign's size,
   which every language shares.
4. Run `python3 art/tools/render.py` and look at `art/out/8/signs/<lang>/`
   and at 4 px a unit.

`fr.json` is the test language beside English: it uses É È Ê À Ô and a
hyphenated two-line sign.
