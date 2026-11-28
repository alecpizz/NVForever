# Which references are multibounds in New Vegas, and do exterior occlusion planes hide them?

Priority: P3
Needs: a person at the keyboard or the capture plugin with the console; screenshots; no particular save
Spec: #1434, [level-design.md](../../spec-notes/level-design.md) §3.2 and §13 question 8

The tutorial says each frame every multibound is tested against every occlusion plane, and
a multibound wholly behind one is skipped with all its contents [documented: Occlusion]. It
does not say which New Vegas references are multibounds. In the data, occlusion planes,
room markers and portals carry `XMBO`, and only two references carry `XMBP`, "MultiBound
Primitive Marker" [observed: level-design.md §3.2]. NVForever tests each placed object's
own bounds against the planes instead, indoors only. Outdoors a Primm plane, 0x00177B3D,
stands 88 units above its retaining wall and hid a roof sign's letters seen over it
[observed: NVForever screenshots, 2026-11-06]. So exterior planes stay off until the
original's rule is known.

1. At Primm, stand where the roof sign is seen over the retaining wall that plane
   0x00177B3D lies in (the plane runs x −57,480 to −53,480 at about z 6,000). Screenshot
   the sign. Then `TFC 2` (frozen culling, multibounds and planes shown coloured), and
   screenshot again from the same spot and from a step to the side.
2. In the same place, `MBC` (multibound and room culling off) and back, with a screenshot
   each: does anything appear or vanish?
3. `prid` and `GetBaseObject` (or the console's selection line) on each coloured box `TFC 2`
   shows, if it shows any.
4. If `TFC 2` shows nothing outdoors, the same three steps in an interior with occlusion
   planes (RocketLabMid, planes 0x000CD790 to 0x000CD794).

Report:
- whether the sign over the wall is ever hidden in the original;
- what `TFC 2` colours, and which references those boxes belong to;
- whether `MBC` changes what is drawn.

This settles §13 question 8: whether exterior planes can be used with per-mesh tests, or only
against multibounds, and which references count as multibounds.
