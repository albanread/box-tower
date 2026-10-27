# People

All drawn by `gen.py` in this directory (`python3 gen.py` rewrites every
SVG here).  Edit the generator, not the SVGs.

## Size and anchor

Every figure is 6x12 units: the body is about 3 wide and 10 tall (as
upstream's figure, 0.75 of a cell by 2.5 cells), centred on x = 3, feet on
y = 12.  To stand a figure on a floor, put its bottom edge on that floor's
surface (y = 15 of the floor) and its centre on the person's position.

## The set

Roles: `civilian`, `worker` (office: collar, tie, briefcase), `guest`
(hotel: pulls a suitcase), `housekeeping` (head scarf, apron), `security`
(peaked cap, badge, belt), `medical` (white cap with a cross, coat edge).

| File | What |
| --- | --- |
| `ROLE-stand-right.svg`, `-left` | standing or waiting in a queue |
| `ROLE-walk-right.N.svg`, `-left` | walking, N = 0..3: stride, pass, stride, pass |
| `ROLE-front.svg` | facing the viewer: in a room, or riding if the program draws riders |

Left-facing pictures are the right-facing ones mirrored; a presenter may
flip the right ones instead and drop the left files from its atlas.  Walk
at about 4 frames a cell walked.

## Recolouring: the `.tint.svg` layer

Every picture `x.svg` has `x.tint.svg`, same viewBox.  Its channels say
which parts take which colour:

| Channel | Region | Colour to put there |
| --- | --- | --- |
| red | upper clothes: shirt, jacket, sleeves | stress or role colour |
| green | skin: face, neck, hands | skin tone |
| blue | lower clothes: trousers, skirt | trouser colour |

Elsewhere the tint layer is transparent or black (black marks things that
keep their own colour: hair, caps, badges, aprons, luggage, shoes).  Read
the weights premultiplied, `w = tint.rgb * tint.a`, and

    out.rgb = pic.rgb * (1 - wr - wg - wb) + top * wr + skin * wg + legs * wb
    out.a   = pic.a

Back limbs carry 0.8, so a fifth of the picture's darker shading stays and
the far arm and leg still read as behind.  The untinted picture uses grey
clothes and a mid skin tone, so it is fine as it is (contact sheets).

### Colours, as the game needs them

Stress is shown while a person waits (upstream shows figures only then).

| Stress | top (red) | legs (blue) |
| --- | --- | --- |
| low, civilian/worker/guest | one of `#2f5d50` `#3b5b8a` `#6b5a7e` `#7a6a4f` `#4a4f57`, by a hash of the person | `#3b4250` or `#2e3440` |
| low, staff | role colour: housekeeping `#b9a3d6`, security `#2f4a7a`, medical `#e9eef2` | `#3b4250` |
| medium | `#ff5fa2` | `#a33a6c` |
| high | `#d81919` | `#8a1010` |

Medium and high colour the whole clothes, so the figure reads pink or red
as upstream's does (its colours are `ENTITY_STRESS_COLORS`: medium
`0xff5fa2`, high `0xd81919`).  Skin tones, by a hash of the person as
upstream varies them: `#f5d3b0` `#d9a87a` `#a8744a` `#6e4426`.
