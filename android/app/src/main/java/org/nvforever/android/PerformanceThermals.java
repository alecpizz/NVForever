package org.nvforever.android;

/** Bounded, session-only thermal feedback. No identifiers or user preferences. */
final class PerformanceThermals {
    interface StatusSink { void changed(int status); }
    interface Source {
        void subscribe(StatusSink sink);
        void unsubscribe();
        int status();
        float headroom();
    }
    interface Clock { long millis(); }
    static final long SAMPLE_MS = 10_000;
    static final long STALE_MS = 25_000;
    private final Source source;
    private final Clock clock;
    private long generation;
    private boolean active;
    private int status = -1;
    private int headroom = -1;
    private long sampledAt = -1;
    private long headroomAt = -1;
    private long polledAt = -1;

    PerformanceThermals(Source source, Clock clock) {
        this.source = source;
        this.clock = clock;
    }

    synchronized void start() {
        if (active) return;
        long session = ++generation;
        active = true;
        sampledAt = -1;
        status = headroom = -1;
        try {
            source.subscribe(value -> accept(session, value));
            sample(true);
        } catch (RuntimeException unavailable) {
            stop();
        }
    }

    private synchronized void accept(long session, int value) {
        if (!active || generation != session) return;
        status = validStatus(value);
        // A status callback must not make an old headroom sample look fresh.
        if (headroomAt < 0 || clock.millis() - headroomAt > STALE_MS) headroom = -1;
        sampledAt = clock.millis();
    }

    synchronized void sample() {
        sample(false);
    }

    private void sample(boolean resumed) {
        if (!active) return;
        long now = clock.millis();
        boolean due = polledAt < 0 || now - polledAt >= SAMPLE_MS;
        if (!due && !resumed) return;
        try {
            status = validStatus(source.status());
            if (due) {
                // This rate bound spans quick pause/resume cycles too.
                polledAt = now;
                float value;
                try { value = source.headroom(); }
                catch (RuntimeException unsupported) { value = Float.NaN; }
                headroom = Float.isFinite(value) && value >= 0 && value <= 10
                        ? Math.round(value * 1000) : -1;
                headroomAt = now;
            }
            sampledAt = now;
        } catch (RuntimeException unavailable) {
            status = headroom = -1;
            sampledAt = -1;
        }
    }

    synchronized void stop() {
        ++generation;
        boolean subscribed = active;
        active = false;
        status = headroom = -1;
        sampledAt = -1;
        if (subscribed) {
            try { source.unsubscribe(); }
            catch (RuntimeException ignored) { /* Feedback remains inactive. */ }
        }
    }

    synchronized String snapshot() {
        long age = sampledAt < 0 ? STALE_MS + 1 : Math.max(0, clock.millis() - sampledAt);
        boolean fresh = age <= STALE_MS;
        boolean headroomFresh = headroomAt >= 0 && clock.millis() - headroomAt <= STALE_MS;
        return (active ? "1" : "0") + "," + (fresh ? status : -1) + ","
                + (fresh && headroomFresh ? headroom : -1) + "," + Math.min(age, STALE_MS + 1);
    }

    private static int validStatus(int value) { return value >= 0 && value <= 6 ? value : -1; }
}
