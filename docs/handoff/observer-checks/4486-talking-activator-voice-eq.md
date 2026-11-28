# What does the original do to a dry file said through a talking activator?

Priority: P3
Needs: a person at the keyboard (choosing a dialogue topic), or a save made after Chris
Haversam's intercom GREETING 0x000CD87D has been said; the game's output recorded on the
WASAPI loopback (the method of observer batch o28, observer-checks.md "Added 2026-11-06")
Spec: #4486, [audio.md](../../spec-notes/audio.md) §15.5

Ours: a talking activator's lines play as their files, and the voice filter is used only
for armour that modulates the voice [observed for band-limited files: o28_intercom, #992].
Two of Chris Haversam's REPCONN intercom lines, ChrisHavLabIntercom 0x000CECF7 responses 1
and 2, have dry full-band files, so in NVForever they sound like a man in the room
(player report on #4486).

## Answered so far (2026-11-27, capture jobs o35_chris_intercom9, 10 and 11, save nvftest) [observed]

**Method.** Chris's GREETING 0x000CD87D played twice through ObservationIntercom
0x000CE86D (`Activate player 1`; the player about 20 units from the intercom, Chris moved
430 units off). As the control it played once face to face in the same cell
(`StartConversation player GREETING` from a script, the player 150 units from Chris). The
line has no activator condition, so the same files play both ways.

**Results.** Each response's file is found in the recordings at a normalised correlation
of 0.83–0.99. The cross-spectrum transfer from the file to the recording is the same in
both channels (coherence 0.8–1.0 from 250 Hz to 6 kHz):

| band | face to face | through the intercom |
|---|---|---|
| 250–500 Hz | −4.1 dB | −4.7 dB |
| 500–1,000 Hz | −4.1 | −6.6 |
| 1–2 kHz | −4.1 | −10.2 |
| 2–3 kHz | −4.1 | −12.1 |
| 3–4.5 kHz | −4.2 | −11.8 |
| 4.5–6 kHz | −4.4 | −7.5 |
| 6 kHz | −4.5 | −13.7 |
| 7–8 kHz | −5 | −19 (coherence 0.2) |

- Face to face is flat within 0.6 dB.
- The intercom curve repeats to 0.1 dB across two runs and shows no comb.
- Against its 600–1,500 Hz level, the intercom curve is the tilt o28_intercom measured at
  Benny's Tops intercom: +4 dB at 300–400 Hz, −4 dB at 2–3.5 kHz, a small bump at
  4.5–5 kHz.
- So the tilt comes with the talking activator and not with the room. It is a mild EQ,
  not the 350–3,200 Hz band of `voice_filter.rs`.
- [hypothesis] The cause is untested. The player stood nearer the intercom (20 units)
  than the face-to-face Chris (150), and a near-field effect is not ruled out.
- 0x000CD87D's files hold almost nothing below 200 Hz, so the original's low end is
  unmeasured.

## Left to do

Record 0x000CECF7 (a topic under GREETING 0x000CE9F0 "Hello? Who's down there?", which
needs VMS01.NightkinQuest 1 or 2 and ObservationIntercom's `currentintercom` 1). Fit both
responses to their files as above, and say:
- whether the curve matches the one above;
- what happens below 200 Hz.

Things that did not work unattended:
- the dialogue menu stayed on 0x000CD87D's second response and never showed its topics,
  and `ClickMenuButton DialogMenu/NOGLOW_BRANCH/DM_ClickRect` did not advance it;
- `GetUIString` read every `DM_TopicList/DM_Topic…` path as empty;
- after `CloseAllMenus`, a second `Activate player 1` opened no conversation;
- `StartConversation player ChrisHavLabIntercom` and `Say ChrisHavLabIntercom 1` on the
  intercom played nothing;
- a scripted `Activate player` without the `1` skips the intercom's OnActivate block, so
  `SetTalkingActivatorActor` never runs.
