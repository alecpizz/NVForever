package org.nvforever.android;

/** Shared production completion protocol. Host owns the actual Activity effects. */
final class LoadingReveal {
    interface Host {
        LoadingState state();
        LoadingUi.CoverLease cover();
        boolean active();
        void completed();
        void failed();
    }
    static void schedule(Host host) {
        final String ticket = host.state().ticket();
        final LoadingUi.CoverLease queued = host.cover();
        if (queued == null) return;
        final long generation = queued.generation();
        queued.nextFrame(() -> {
            if (!host.active() || host.cover() != queued) return;
            final LoadingState state = host.state();
            if (!state.readyToReveal(ticket)) return;
            final LoadingUi.CoverLease.HideResult hidden = queued.hideForReveal(generation);
            // Window callbacks can synchronously retire this episode/Activity.
            if (!host.active() || host.cover() != queued || host.state() != state
                    || !state.readyToReveal(ticket)) return;
            if (hidden == LoadingUi.CoverLease.HideResult.STALE) return;
            if (hidden == LoadingUi.CoverLease.HideResult.FAILED) {
                state.fail();
                host.failed();
                return;
            }
            if (state.reveal(ticket)) host.completed();
        });
    }
    private LoadingReveal() { }
}
