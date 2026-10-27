# The tower's art

Everything here is SVG, drawn by hand (or by code) for this port, or taken
from tower-together and fixed.  `tools/render.py` turns it into PNGs at the
scales the presenters need; `out/` is never committed.

## Rules every picture follows

- **The grid.** One cell is 4 units wide, one floor 16 units tall (as
  tower-together's world units).  A room's SVG has `viewBox="0 0 W H"` with
  W = cells x 4 and H = floors x 16, exactly: nothing is stretched to fit.
  Tile widths are in `sim/src/resources.rs`.  Pictures are rendered at
  4, 8 and 16 pixels a unit (a cell 16, 32 or 64 pixels wide).
- **The style.** Flat shapes, no outlines, no gradients except where a
  material needs one (glass, metal), no text or lettering of any kind,
  except in `src/signs/`: the game's signage (OPEN, CLOSED, FOR RENT,
  SOLD ...) is drawn there as pictures whose letters are shapes made by
  our own lettering generator, one set of pictures per language, never
  SVG `<text>` and never a font file.  STYLE.md has the palette.
- **Layers.** `name.svg` is the picture as lit by day.  `name.emit.svg`
  (optional, same viewBox) is what gives out light at night: lamps,
  lit windows, screens, in their own colours on transparent.  The
  presenters' shaders darken the picture by night and add the emission.
- **Frames.** An animation's frames are `name.0.svg`, `name.1.svg` ...
- **Origin.** Never trace, copy or imitate a picture from the 1994 game,
  its sequels or any other game, and take nothing from the web.  Files
  from tower-together (MIT, Patrick Hulin) keep a comment saying so;
  PROVENANCE.md lists them and what we changed.

## Layout

| Directory | What |
| --- | --- |
| `src/rooms/` | every room type and its states |
| `src/transport/` | elevator shafts and cars, stairs, escalators |
| `src/people/` | the people: standing, walking, riding, by stress |
| `src/building/` | floor slab, walls, roof, ground, empty floor, construction |
| `src/world/` | sky pieces, clouds, soil, the street |
| `src/signs/` | the signage, per language (`en/`, ...), and its lettering generator |
| `src/events/` | fire, smoke, bomb, the helicopter, VIP, Santa, the cathedral's visitors |
| `tools/` | render.py and the contact sheet |
