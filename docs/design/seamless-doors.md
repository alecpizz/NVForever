# Seamless doors

NVForever's own feature, not the original's: through a load door the original always
shows a loading screen [observed: docs/spec-notes/loading-screens.md]. With
`--seamless-doors` (off by default) a load door opens onto the place it leads to and the
player walks through it, with no fade and no loading screen. Prototype on Goodsprings
(branch `feat/seamless-doors`, fork only for now).

Parts, each on its own switch (`--seamless-doors ahead,fade,keep,portal`; alone or `all`
for every part): `nvf_play::seamless`.

## 1. Ahead: what a near door leads to is prepared

Load doors within 1,200 units of the player (`AHEAD_RADIUS`, at most 4) have their
destination prepared before they are opened [design]:

- physics: the cells the travel loads (the interior, or the 3 × 3 squares round the
  arrival) are prepared on a thread of their own (`nvf_play::phys::warm`) and kept up to
  60 s; a reference of one changed (`RefChanged`, a script's move) drops it;
- renderer (`nvforever::door_ahead`): an interior's models are prepared; outdoors the
  squares round the arrival are read ahead and kept (`world::pin_cells`); the actors
  there are listed for the actor read-ahead (kept indoors: `Job::Actor { door }`).

## 2. Fade: when a door cannot be walked through

A travel that cannot be a walk-through (the first visit before step 3 has spawned the
place, another worldspace, fast travel, a script's move) fades through black: 0.2 s out,
the travel, back over 0.3 s once in; the loading picture only after 0.75 s
(`loading::FADE_*`). Logged: what the fade waits for.

## 3. Keep: the place left stays

When the two places do not overlap (`seamless::keeps`: interiors are drawn at their own
coordinates, which overlap one another and the worldspace's middle [observed: the 376
interiors of FalloutNV.esm span x −7,040..18,378, y −22,592..16,000; Goodsprings is near
(−71,600, 2,300)]), the place left is parked, not dropped:

- physics keeps its cells in the world (`PhysState::kept`), streaming leaves them;
- the renderer keeps its cells' entities, hidden (`world::Parked`); the distant land is
  hidden with them, not rebuilt (`LodState::park`);
- going back draws them again at once.

Ahead of a first visit the place is spawned already (`world.rs` `prespawn_want`,
`prespawn_outdoors`): outdoors, the interior behind the nearest door once its models are
in (the saloon: 461 references over 3 frames, 6.5 ms at a 2 ms budget a frame); indoors
with nothing outdoors kept (a new game in Doc Mitchell's house), the 5 × 5 squares round
the door's arrival (25 squares over 17 frames, 56 ms) [observed 2026-11-14].

## 4. Portal: walking through

Activating a door whose other side is parked opens it (`door_portal.rs`):

- **Frames.** Each door's out is the way its arrival marker faces (markers face away
  from their door [observed: every Goodsprings door, heading = door rotation − 90°]),
  snapped to the door's own axes; the frame is centred on the drawn slab (a model's origin
  may be off its middle); the floor is measured by physics just out from each threshold
  (Doc Mitchell's markers float about 52 units up; his porch is raised).
- **The map** takes a point in front of this door to the matching point inside the other
  door's place, `SHIFT` (56 units) further in: walking into one door is walking out of the
  other. The crossing (feet within 24 units of the plane, inside the doorway, looking in)
  sends the travel with the player exactly where the door's camera stood.
- **The view.** The parked place's entities go on render layer 20 (`PORTAL_LAYER`; those
  wholly behind the far door on 21, which the view leaves out but whose shadows it keeps),
  shown, and a camera on that layer follows the main view through the map into a texture
  the doorway's quad samples at its own screen position. It has the main view's
  multisampling and prepasses (so the pipelines it compiles are those the main view needs
  after the crossing: without, most of an interior was missing for seconds), the place's
  own light (an interior's from its lighting, the outdoors' from the sky state:
  `SkyState::exterior_look`), the outdoor far plane, a copy of the sky outdoors, the parked
  distant land, grass near it and billboards turned to it.
- **Grade.** The image-space pass grades the doorway's outline on screen with the other
  place's grade; only the eye's HDR part eases across the crossing (1.5 s).
- **Room culling** keeps to the interior's own meshes; the portal re-asserts its layers
  while open.
- **Collision** of both doors goes while open (given back on closing).
- **Actors** standing in the parked place are drawn (`ParkedActor`), so a door shows them
  (Sunny Smiles through the saloon's door); a door's activation (the player entering the
  cell, teammates coming along) runs at the crossing, not when it is opened; a door with
  a script opens when its script sends the player through (Doc Mitchell's front door).
- The door behind opens as the player comes out of it, and closes once walked away from.

## 5. Measured

Test aid `NVF_PLAY_TEST_PORTAL_PROBE=<dir>[:<count>]`: the player stood on the door's
arrival spot facing the doorway, a mask frame (the doorway magenta), a frame through the
door, the crossing forced, a frame 3 frames after. Mean absolute difference (0–255) of
the two inside the doorway, walking in / walking out, Goodsprings at noon, no NPC
packages [observed 2026-11-14]:

| door | in | out |
|---|---|---|
| Prospector Saloon front | 10.7 | 20.0 |
| general store | 6.6 | 16.6 |
| Doc Mitchell's | 3.7 | 14.4 |
| saloon back doors (0x00106372, 0x0010AEDC) | 6.9, 8.7 | 21.2, 33.4 |
| houses (0x0013005E, 0x00105228, 0x00177D3A) | 12–19 | 14–18 |

Most of what is left is the player's walk between the two frames (parallax) and, walking
out, the sky's frozen copy. Smoke: `seamless-doors-saloon`.

Cost: standing at the saloon's front door, frames took 6.8 ms on average with the door
open and 5.9 ms with none (the exit line `door portal frames: …`) [observed
2026-11-14, release-quick build].

## 6. Not done

- Doors still vanish when opened. A swing into the doorway with the view's depth set
  back is written and switched off (`SWING_DOORS`): the saloon's front doorway showed a
  dark surface and the door did not move.
- The sky through a door seen from indoors is a copy of the dome and clouds as last drawn
  outdoors; started indoors (a new game in Doc Mitchell's house), a dome coloured as the
  outdoors now (`SkyState::exterior_dome`) with no clouds. The distant land is built round
  the door's arrival from indoors (`LodState::around`), so it does not come in on the
  way out.
- Opened doors dither away (0.3 s) once their view has been drawn (0.35 s, 10 frames:
  before that its texture is empty while pipelines compile).
- Exposure: indoors the eye's adaptation lifts the doorway's outdoors (it is blown out
  from a dark room); it settles as one walks out.
- NPCs going through a door still fade out at it and in at the other side.
- Another worldspace (Freeside's gates: the gate at (−9,816, 102,730) in WastelandNV
  arrives at (9,246, −7,137) in FreesideNorthWorld [observed: `fnvtool sim doors`]) and
  interiors overlapping the place left still fade.
- Imposters, particles and actors' animation level of detail follow the main view only.
