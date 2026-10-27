# How the tower's pictures are drawn

This note is for anyone adding or changing a picture, so that it sits
beside the others without looking borrowed.  The look is tower-together's
(Patrick Hulin's room pictures, MIT): flat shapes, soft colours, small
rounded corners, no outlines.  README.md has the hard rules (the grid,
no text, layers, frames); this file has the vocabulary.

## Units and coordinates

A cell is 4 units wide and a floor 16 units tall, so a room's viewBox is
`cells*4 x floors*16`.  Rendered at 4, 8 and 16 pixels a unit, a floor is
64, 128 or 256 pixels high.  Design for 8 and check at 4: at 4 a unit is
four pixels and anything under half a unit thick goes soft.

The pictures taken from tower-together keep upstream's own coordinates
(20 to a cell, 80 to a floor, or 320 to a floor for the hotel rooms)
inside one `<g transform="scale(k)">`, with k = 0.2 or 0.05.  That is a
single uniform scale, never a stretch: the drawings were already in the
right proportion for their slots.  New pictures (metro, cathedral) are
drawn straight in our units.  Either way is fine; do not mix both in one
group.

The bottom 0.8 unit of a floor (4 of upstream's 80) is the floor strip,
and the 0.8 above it the skirting.  People stand on the top of the floor
strip, at y = 15.2 within each floor, so keep that line clear of
furniture fronts that would hide their feet.

## Palette by family

Each room family has a shell: wall, skirting, floor.  Furniture is drawn
in darker and lighter steps of the wall colour, with one or two accents.

| Family | Wall | Skirting | Floor | Accents |
| --- | --- | --- | --- | --- |
| Office | `#9aacbb` | `#7a8c9c` | `#8494a4` | desks `#6e8090`, chairs `#4a5c6c`, screens `#6a8ea8`, plants `#4a8a4a` |
| Condo | `#d4c478` | `#a09050` | `#b0a060` | wood `#8a6a40` `#9a7a50`, glass `#a8c8d8` `#98b8c8`, lamp `#ece0a8` |
| Hotel single | `#d98a98` | `#b56071` | `#c87789` | linen `#f0d4da` `#f3c3cf`, wood `#8a4b5a` `#9a5a6a`, screen `#463039` |
| Hotel twin / suite | `#c87798`, `#c47889` | `#8a4868`, `#7a3848` | `#a85878`, `#5a3a2a` | as single |
| Hotel, being cleaned (`.dirty`) | the same, with browns `#886358` `#745848` and litter | | | |
| Restaurant | one theme a variant: wine `#6a2838`, trattoria `#e8d4a8`, izakaya `#2a1a10`, diner `#68b8b0`, seafood `#c8e0e8` | | | cloth `#f0f0e8`, dark wood `#4a2818` |
| Fast food | diner red `#c45050`, pizzeria `#f4d8a4`, café `#d4b890`, noodle bar `#2a1818` | | | sign gold `#f8c878` on `#1a0808` |
| Shop | cream `#e0d4b8`, electronics `#c8d0dc`, books `#e8d8b8`, jewellery `#f0d8d8`, toys `#b8e0e8` | | | track light `#888078` |
| Cinema | `#481018` | `#300810` | `#200408` | seats `#8a2030`, lamps `#ffe098` |
| Party hall | `#3a1a4a` | `#4a2060` | `#6a3080` | gold `#d0a848`, beams |
| Recycling | `#9a9690` | `#2a3840` | | doors `#6088a0`, hazard `#d8c040` on `#2a2a2a` |
| Lobby | `#8f6d48` .. `#fbf4e8` | | | warm stone and cream |
| Security | `#3a4858` | `#202830` | `#405060` | monitors |
| Housekeeping | `#aab8b0` | `#889890` | `#607068` | linen `#f0f0e8`, red `#d04040`, blue `#4080c0` |
| Medical | `#c8dcd0` | `#98b4a8` | `#88a498` | cross `#d04040`, curtain `#c8a8d0` |
| Parking | `#7a7a76` | `#585854` | | stripes `#f0f0e8`, hazard `#d8c040`, sign blue `#2a5090` |
| Metro | tile `#e0d8c8` (grout `#c8c0b0`), vault `#a8a090` | `#9a9284` | platform `#787880` | line band `#c04040`, sign blue `#2a5090`, edge line `#d8c040`, track `#303038` `#1a1a20`, rail `#a8a8a8` |
| Cathedral | stone `#e8e0d0` `#d4ccbc` `#bfb6a4` | | | slate `#5a6478` `#4a5266`, lead `#3a3a48`, gold `#d8b040`, glass `#3a6ab0` `#c04050` `#e0b040` `#3a9a6a` `#7a4aa0` |
| Laundry (BOX) | `#b8c8d0` | `#8a9aa4` | `#9aa8b0` | machines `#eef2f4`, water `#3a8ac0`, linen `#f4f4ee` `#e8d0d4` `#d8e4ec` |
| Guest services (BOX) | `#e8d8b8` | `#a88858` | `#c8a878` | desk `#7a4a2a`, brass `#c8a040`, bell `#d8b040` |
| Premier lounge (BOX) | `#3a2a20` | `#24180f` | `#5a3a26` | leather `#6a3a2a`, brass `#8a6a3a`, window `#2a4a6a` |
| Swimming pool (BOX) | tile `#d8eef0` (grout `#c0dade`) | `#9ac0c8` | `#b8d8de` | water `#3aa0d0` `#5ab8e0`, band `#4a90b8`, lifeguard red `#d84a3a` |
| Gym (BOX) | `#c8ccd0` | `#5a5e64` | rubber `#3a3a40` | mirror `#a8c0cc`, stripe `#e07a2a`, machines `#2a2a30` |
| Convention centre (BOX) | `#5a6478` | `#3a4252` | `#8a6a4a` | stage `#3a2a20`, seats `#a03a4a`, drapes `#8a2a3a`, chandeliers `#d8b040` |

The BOX rooms and `security.1`, `security.2` are drawn by
`src/rooms/amenities.py`, each also as `.busy` (people in it).

**Vacant and unlit.** An empty room keeps its family's shell with every
colour multiplied by (0.80, 0.82, 0.90): daylight only, a little cool.

## Shapes

All corners are slightly rounded (`rx` of a tenth to a quarter of the
shape's short side).  No strokes except for thin things that are lines in
life: rails, cables, rods, chair legs, clock hands.

- **Wall, skirting, floor**: three full-width rectangles, as above.
- **Ceiling light**: a flat fitting against the ceiling (a wide low
  rectangle) with a narrower, paler diffuser under it.  Track lights are
  a bar with small discs.
- **Window**: a frame in the skirting colour, four panes, the top two a
  step paler than the bottom two (sky above, reflection below).  Blinds
  are horizontal bars across the panes; curtains are two panels with
  three darker fold stripes each.
- **Desk**: a thin top, two leg rectangles, a monitor as a dark frame with
  a paler screen and a short stand; the chair a rounded back and seat
  on a stalk and a foot bar.
- **Bed**: headboard post, mattress in the pale linen colour, a pillow
  as a rounded bar, the cover a rectangle in the mid linen colour.
  A made bed is flat; a slept-in or dirty bed has a rumpled cover and
  things on the floor.
- **Counter**: a slab top a step paler than its front, stools as a disc
  seat on a post.
- **Lamp**: a shade (a short trapezoid or rounded rectangle in a pale warm
  colour) on a thin stem with a small base.
- **Sign**: a plate with a darker border.  What it says is a pictogram of
  what is sold (a cup, a slice, a bowl, a fish, a burger, a bottle, a car)
  or rounded bars standing for lines of writing.  Never letters, numbers
  or glyphs of any script; never a real company's mark or anything close
  to one (the metro's sign is a train's face on a blue plate for that
  reason).
- **Picture on a wall**: a dark frame round flat shapes (hill, sun, sky
  band).
- **Boxes**: cardboard `#b8946a`, a paler lid band, a tape strip down the
  middle.

## Night layers (`name.emit.svg`)

The presenters darken the day picture by night and add the emission on
top, so the emit layer holds only what gives out light, in the colour of
that light, on transparent.  Rules:

1. **A room in use is lit from inside.**  Its emit layer is the day
   picture under warm interior light: the day file placed with `<image
   href="name.svg">`, through a colour matrix (red 1, green 0.9, blue
   0.68), at opacity 0.6 (0.45 for back rooms: parking, recycling,
   housekeeping).  It follows any change to the day picture.  The
   program shows it only when the room is occupied or open.
2. **Vacant rooms have no emit layer.**  An empty office, condo, shop or
   hotel room is dark at night; that is how a vacancy reads after dusk.
3. **Hand-drawn emission** is for pictures seen from outside or with
   strong sources (metro, cathedral): lamps and their light pools as
   low-opacity (0.1-0.25) shapes, screens and boards in their own
   colour, signal lights.  Colours: lamps `#fff4d0`, warm lanterns
   `#ffd890`, screens `#a0e0ff`, boards `#ffc050`, go `#50f090`, stop
   `#ff5050`, a train's or cabin's lit windows `#ffe8b0`, stained glass
   in its own colours.
4. Same viewBox as the day picture, and nothing in it that is not in the
   day picture.

`render.py`'s contact sheets show each emit layer on night blue and then a
night preview (day darkened, emission screened over), so check there.

## States and frames

| File | Meaning |
| --- | --- |
| `office.0..4` | occupied offices, a variant chosen per position |
| `office.vacant`, `condo.vacant`, `retail.vacant` | to let / for sale: empty, unlit (the label is the program's) |
| `hotel-*.0..2`, `hotel-*.N.dirty` | occupied; checked out and awaiting housekeeping |
| `hotel-*.vacant` | made up and empty, curtains drawn |
| `parking.empty`, `parking.0..3`, `parking.no-access` | a bay empty, with one of four cars, unreachable |
| `recycling.0..4` | fill level, empty to full |
| `metro.0`, `metro.1` | waiting, train in |
| `cockroach.0..3` | walk cycle (a 2.5 x 2 unit sprite) |
