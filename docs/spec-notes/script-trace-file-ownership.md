# Script trace ownership and sequential reuse

The documented `NVF_PLAY_SCRIPT_TRACE=<path>` contract writes trace events to that
path (script.rs and docs/design/layer5-play.md). Maintainer review of6507c5f,
PR2383 comment6875026800, identifies that treating every existing file as a live
writer violates this contract and leaves readers looking at a finished older run.

[documented] Rust1.89 introduced [File::try_lock](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)
and [TryLockError](https://doc.rust-lang.org/std/fs/enum.TryLockError.html).
`WouldBlock` means another handle/process holds the lock. The lock remains owned
by the file handle until it is explicitly unlocked or all duplicated/inherited
handles close. Platform locking behavior differs; this code does not promise live
unlocked readers on Windows.

[documented] Linux [flock(2)](https://man7.org/linux/man-pages/man2/flock.2.html)
associates the lock with an open file description, including descriptors duplicated
by `fork` or `dup`. Closing the original descriptor does not release its lock while
a duplicate remains open. Rust's [File::try_clone](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_clone)
creates a handle referring to the same underlying file.
[documented] Windows [LockFileEx](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex)
also recommends explicit unlocking rather than depending on OS cleanup timing.

[design] Open the requested file without truncating, acquire its exclusive OS lock,
then truncate and write the header. Keep an owning file-lock guard inside BufWriter
through the writer's entire lifetime. After BufWriter's final flush attempt, the
guard explicitly unlocks before closing its handle, including on initialization
errors. This prevents a descriptor temporarily inherited by another process from
extending ownership beyond the trace writer's lifetime. Only WouldBlock selects a PID/attempt sibling; other
open, lock or truncate errors remain visible. Finished requested files are reused
at the exact path rather than accumulating suffixes. Do not delete an old pathname
as a cleanup trick: it could be another writer's identity. Retain the64-name bound,
format/processId header, immediate header flush and actual-path log.

The authored controls cover overlapping same-process handles, a real independently
opened child-process writer while the requested file is locked, and sequential
reuse of a stale requested path. All content/JSONL/no-NUL/separation assertions
remain; completed files are inspected after close because Windows locks may exclude
other read handles. The child test is isolated through its own exact test entrypoint.
At initial source preparation these controls were authored; subsequent source-bound
execution results are recorded in the PR.

[design] A deterministic descriptor-lifetime regression retains a duplicate handle
after the trace writer drops, with a live-writer separation control followed by
immediate reuse and truncation of the exact requested path. No sleep, lock retry or
test-thread serialization substitutes for ownership release. This isolates a
documented retention mechanism shared by duplicate and fork-inherited descriptors;
it does not establish the cause of an earlier CI failure without an
incident-specific process trace. The existing real child-process test retains all
its ownership, process-ID, content and exact-path reuse assertions.
A second duplicated-handle control closes the old duplicate while a replacement
writer is live and checks that the replacement still owns the exact path.

Earlier private trace-corruption evidence and the original overlapping-writer test
are historical regression context, not original-game behavior observations or proof
of this revision. No real game/save-load handoff, native parity, full-mod, FPS or
crash repair is claimed. A creation/header failure may leave a partial file, and
uncooperative writers that ignore OS locking remain outside the ownership protocol.
