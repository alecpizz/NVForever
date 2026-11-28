# Android water counter readback admission

[documented: engine/nvforever/src/reflection.rs, engine/vendor/bevy_render/src/gpu_readback.rs]
The water reflection observer installs a persistent GPU readback once its counter
exists. Bevy prepares another staging buffer each frame and keeps each requested
buffer until its asynchronous callback completes. There is no outstanding-request
bound for that entity. If callbacks stall, more requests and buffers remain alive.

[design] Android instead admits one `ReadbackOnce` request at a time. Only the
completion for that exact pending request updates the existing cumulative counter
and retires its entity. The following normal Update may admit the next request.
No completion means no further requests; the existing initial two-second fallback
remains unchanged. This limits the caller to one outstanding readback, including
across a suspend or delayed callback, without changing generic Bevy readback behavior.
Desktop keeps its existing persistent readback path.

[design] Counter change, equality and wrap semantics stay unchanged. Sampling now
follows callback progress on Android, so a slow callback can delay reflection
visibility decisions. This observer already measures asynchronous counter changes;
it does not establish that any particular frame's pixels reached the screen.
The change does not alter materials, shaders, capabilities, water geometry or the
renderer error policy. Failed-map callback hardening is a separate concern.

[design] A replaced counter resets the observed baseline but does not bypass an
outstanding old request. That request's late completion retires it without applying
its old bytes; only then may the new counter be requested. Malformed short results
also retire their request without changing the counter baseline.

[implementation evidence: private bounded R7 diagnostic archive, independent source investigation]
A fresh R7 device-loss event follows a successful world load without a retained
intervening resume. The archive has no retained in-flight readback counts or uniquely
attributable new callback stack. This admission bound addresses a demonstrated
source-level resource-growth path; it is not established as the cause or cure of
that device loss or the earlier temporary flat actor appearance.

[design] Headless App tests use the real `GpuReadbackPlugin` one-shot cleanup and
real completion observers, with a synthetic counter handle and clock. They check
that repeated updates without completion do not create new requests/entities,
that the existing fallback still activates, that one completion allows exactly one
successor, that stale/duplicate completions cannot release a newer request, and
that malformed bytes cannot update the cumulative counter. No GPU is used or
claimed by these tests. Existing counter equality/change/wrap tests remain intact.
