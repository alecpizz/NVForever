//! What an extraction makes of the request: decided here, from plain values, and carried out in
//! gpu.rs [design].

use super::{HookState, Place};
use bevy::render::render_resource::CachedPipelineState;

/// Which texture a view has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Key {
    pub(super) place: Place,
    pub(super) revision: u64,
    /// Not the empty one.
    pub(super) solved: bool,
}

/// What the device says to it (admit.rs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Admission {
    /// It cannot be asked yet: asked again in the next frame.
    Deferred,
    Refused,
    Granted,
}

/// What [`plan`] goes by beside the request and what is bound.
#[derive(Clone, Copy, Debug)]
pub(super) struct Now {
    pub(super) here: Option<Place>,
    pub(super) hook: HookState,
    pub(super) admission: Admission,
    /// A view may get the bindings now ([`Latch`]).
    pub(super) open: bool,
    /// A world camera's view is drawn in this frame.
    pub(super) views: bool,
    /// The visit that has the stock light to its end.
    pub(super) ended: Option<(Place, Why)>,
}

/// Why the light is the stock one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Why {
    /// Nothing is asked for.
    Nothing,
    /// Asked for another cell, travel or game than the player is in, or outdoors.
    Elsewhere,
    /// The hook or the device cannot say yet, or no view is drawn.
    Waiting,
    Hook,
    Failed,
    Refused,
    /// Asked for when no view can be given the bindings: the next visit's screen is waited for.
    Late,
}

impl Why {
    /// Whether it is so to the end of the visit, whatever is asked for later in it.
    pub(super) fn ends_visit(self) -> bool {
        matches!(self, Why::Hook | Why::Failed | Why::Refused | Why::Late)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Plan {
    /// No view has the bindings and nothing is held.
    Stock(Why),
    /// An older result than what is bound: what is bound stays.
    Keep,
    /// Every world camera's view has the request's; `new`: another texture than the one held.
    Bind { new: bool },
}

/// Whether a request is an older result of the place than what is bound: that one stays.
pub(super) fn older(want: Key, held: Key) -> bool {
    want.place == held.place && want.revision < held.revision
}

/// The first binding of a visit only while its screen is up and has not settled; another texture of
/// the same visit at any time, as it changes no key.
pub(super) fn plan(request: Option<Key>, now: &Now, held: Option<Key>) -> Plan {
    let Some(want) = request else { return Plan::Stock(Why::Nothing) };
    if now.here != Some(want.place) {
        return Plan::Stock(Why::Elsewhere);
    }
    if let Some((_, why)) = now.ended.filter(|e| e.0 == want.place) {
        return Plan::Stock(why);
    }
    match now.hook {
        HookState::Pending => return Plan::Stock(Why::Waiting),
        HookState::Failed => return Plan::Stock(Why::Hook),
        HookState::Installed => {}
    }
    match now.admission {
        Admission::Deferred => return Plan::Stock(Why::Waiting),
        Admission::Refused => return Plan::Stock(Why::Refused),
        Admission::Granted => {}
    }
    match held.filter(|h| h.place == want.place) {
        Some(h) if older(want, h) => Plan::Keep,
        Some(h) => Plan::Bind { new: h != want },
        None if !now.views => Plan::Stock(Why::Waiting),
        None if now.open => Plan::Bind { new: true },
        None => Plan::Stock(Why::Late),
    }
}

/// Whether a view may get the bindings: while a start's or a travel's screen is up, until it first
/// settles (as `pipeline_prewarm`'s `released`), so that their pipelines are waited for.
#[derive(Debug, Default)]
pub(super) struct Latch {
    released: bool,
}

impl Latch {
    pub(super) fn step(&mut self, screen: bool, settling: bool) -> bool {
        self.released = screen && (self.released || settling);
        screen && !self.released
    }
}

/// The pipelines whose fragment reads the volume.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Pipelines {
    pub(super) there: usize,
    pub(super) waiting: usize,
    /// With an error that is not tried again.
    pub(super) failed: usize,
}

impl Pipelines {
    pub(super) fn count(&mut self, state: &CachedPipelineState) {
        match state {
            CachedPipelineState::Ok(_) => self.there += 1,
            CachedPipelineState::Err(e) if !crate::pipeline_gate::retried(e) => self.failed += 1,
            CachedPipelineState::Err(_) | CachedPipelineState::Queued | CachedPipelineState::Creating(_) => self.waiting += 1,
        }
    }
}
