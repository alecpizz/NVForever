# Player script sequence bridge and lifecycle — preparation contract

[design] This is the initial documentation-only draft for the player-sequence
slice of [issue #2133](https://github.com/LootJunkieAzza/NVForever/issues/2133),
tracking repair items NVF-018 and NVF-020. An isolated current-main source port
and authored executable fixtures are prepared. Compilation, paired regression
execution and runtime validation of this port are pending. This note does not
announce a validated bridge or accept the wider issue.

## 1. Current portable boundary

[observed: portable source at 19c0f08a]
[nvse_anim.rs](../../crates/fnv-game/src/nvse_anim.rs) records script-played paths
in `AnimPaths::played`, keyed by actor. ActivateAnim and PlayAnimationPath share
that bookkeeping path; adding a name can affect script getters without drawing
that sequence. Its module note explicitly leaves script-sequence playback to a
later engine slice.

[observed: portable source at 19c0f08a]
[nvse_knvse.rs](../../crates/fnv-game/src/nvse_knvse.rs) maintains clocks, weights,
offsets and sequence metadata. Its transition helpers write the legacy maps.
The current [player body](../../engine/nvforever/src/player_body.rs) and
[animation blending](../../engine/nvf-assets/src/anim_blend.rs) provide rendering
and layering machinery; this draft does not add their missing script request
consumer. Bookkeeping success is not proof of a rendered pose.

[documented: repository catalogue] The
[function rows](../../schemas/script/functions.json) name the explicit view,
priority, restart, weight and easing inputs for ActivateAnim; the path/view inputs
for PlayAnimationPath; and the weight/time inputs for transition helpers and
readbacks. The catalogue identifies API names and argument slots, not proof that
the engine consumes them or that every optional/default behavior is qualified.
Existing [kNVSE design](../design/knvse-anim.md) and
[second-slice notes](ext-knvse.md) retain their separately tagged observations
and hypotheses; this note does not upgrade them.

## 2. Bounded player bridge candidate

[design] Keep a request identity that distinguishes actor, selected player view
and normalized path. Repeated requests, explicit restart, deactivation, clock
changes and seeks must update the same identity without leaking state to another
actor, path, view or Game. Preserve the existing path-comparison policy; no new
path-spelling parity is established here.

[design] Start with explicit caller priority, weight and easing values for the
player's script sequence layer. Loading, metadata publication, playback and
readbacks must agree on the selected request. A failed load must not remain
reported indefinitely as a playing rendered request. Its diagnostics and API
outcome need a stated contract and owned negative controls.

[design] Feed the layer beside the existing player's locomotion/IDLE pose and
remove it through its explicit fade. Do not acquire a new PlayerSeat position
hold simply because a script sequence exists. Do not change the existing
Camera1st-to-ViewDriven rotational policy or the separate Movement-IDLE position
hold repair to make this bridge's fixture pass.

[design] Author clock/seek/pause/restart controls against real request state and
actual blend output. Existing deferred SetAnimCurrentTime behavior remains
separate from immediate transition-helper writes. Do not infer native default
pause/resume or easing rules from a convenient portable clock implementation.

## 3. Mandatory lifecycle correction

[hypothesis: not yet probed in the original game with kNVSE] PlayAnimationPath
is not inherently once-only: a request using looping or reversing clip metadata
survives several complete periods until explicitly deactivated, and end-time
retirement applies only to the clip's non-looping/clamped cycle. [design]
NVForever's render playback selects its mode from actual clip cycle metadata,
rather than treating PlayAnimationPath itself as always once.

[design] Owned cycle controls must cover the portable metadata values 0 and 1
(loop/reverse), with 2 as the clamped control, nonzero begin/end/frequency and
multiple elapsed periods. The portable [cycle-time sampler](../../crates/fnv-nif/src/anim.rs) and
[layer time selection](../../engine/nvf-assets/src/anim_blend.rs) supply that
metadata model. These are authored test vectors, not a new native timing/default
observation. Restart, load failure and explicit stop need
separate assertions; keeping a name in a map is insufficient.

[design] CrossFadeAnims, BlendToAnim and BlendToAnimFromPose already write legacy
weights or offsets. When a matching player/view request supplies readbacks and
rendering, immediate helper writes must also reach that exact request; otherwise
GetAnimationAttributes.seqWeight or GetAnimCurrentTime may show stale values.
Keep unrelated views and paths unchanged. Check source and destination weights,
destination offsets and the resulting sampled layer state together.

[design] The matching-view consistency correction is deliberately limited to
that state agreement. It does not complete static-pose composition, explicit
view forwarding of every transition argument or the remaining optional inputs.
A fixture must select an eligible view before evaluating a transition; failure
at an unrelated view/admission gate is not a lifecycle reproduction.

## 4. Owned reproduction and validation plan

[design] Use newly authored sequence metadata and tiny transform tracks, without
installed KFs, meshes, mod text or saves. The candidate includes four small-library
controls and one blend control from the reviewed source packets. They have not
been compiled or executed on this base. Validation must list and execute actual
tests; the following plan is not a pass:

| Control | Required observation | Excluded shortcut |
|---|---|---|
| Actor/view/path isolation | Only the selected request clock, weight and pose change | One shared path-only clock |
| Loop/reverse lifecycle | More than one full period remains active until explicit stop | Retaining only the script name |
| Clamped lifecycle | The explicit one-shot reaches its end and retires consistently | Hard-coding all APIs to once |
| Transition weight/time | Nondefault helper writes read back immediately on the eligible request and agree with its layer | Updating only legacy maps |
| Deferred seek/restart | Existing deferred seek and explicit restart remain independently observable | Reusing transition-time semantics for every setter |
| Deactivation/fade | Layer fades/releases without creating a player position hold | Suppressing an existing movement guard |
| Failed/unsupported input | Clear bounded outcome with no indefinitely claimed playback | A fake successful status or blanket optional-argument acceptance |

[design] Keep the final fixture identical during a production-only baseline
reversal, with nonzero listing/execution counts and restored source hashes.
Separate helper/core controls from renderer pose checks. After implementation,
run the applicable root/engine suites, strict checks and planner-selected smoke;
those checks are not executions of this documentation commit. A future owned
smoke must observe pose/layer state and release, rather than emit an unconditional
passing line. Installed-consumer replay is additional evidence, not a substitute
for the owned baseline.

## 5. Open contracts and ownership

[hypothesis: not qualified] Native omitted-argument defaults, controller default
weights/eases, morph/time synchronization, additive/reference-pose composition,
full static-pose transitions, NPC direct sequence rendering and complete view
forwarding remain outside this initial slice. Do not silently claim them through
an accepted request or a getter result.

[design] This player bridge can support the API needs reported for B42 Quickthrow
and Immersive Pip-Boy, but accepts neither mod in full. It does not implement a
local-map texture backend, all quest/HUD recipients, throwing/cancel/damage
behavior, IDLE clone admission or unknown Loop255 lifetime. Those retain their
own issue/validation boundaries.

[observed: public PR scope]
[PR #2344](https://github.com/LootJunkieAzza/NVForever/pull/2344) changes when a
script-created actor is drawn and its mirror starts watching. Its placed-actor
read-ahead/spawn-budget work remains with that owner. This draft adds no competing
actor-spawn implementation and claims no resolution of all issue #2133 rows.

## Source port preparation

[design] An isolated implementation port is being prepared from the reviewed player-sequence request/layer model and its mandatory loop/transition correction. The current port has not been compiled or executed. Historical private runtime results do not qualify this source revision. Native defaults, transition/view forwarding, morph/additive/reference-pose composition and NPC rendering remain unqualified. Existing player-model view identity and independent IDLE/camera policies must remain separate.

[design] The candidate reserves the script-layer tag tier `0x40000000` and rejects request serial overflow into it. Portable fallback values remain priority 1, weight 1 and a 0.2-second fade; they are engineering choices, not qualified native defaults. Weights are clamped to 0 through 1, and non-finite explicit inputs or a requested morph synchronization path are refused. The path admission guard accepts a relative path under meshes, optionally already prefixed with meshes, and rejects rooted, drive-qualified and parent-component paths. This lexical guard does not establish physical reparse-point containment.
