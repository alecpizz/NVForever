package org.nvforever.android;

/** Pure Java tests execute the production lifecycle/sampling state, with authored feedback. */
public final class AndroidPerformanceTest {
    private static final class Clock implements PerformanceThermals.Clock {
        long now;
        public long millis() { return now; }
    }
    private static final class Source implements PerformanceThermals.Source {
        PerformanceThermals.StatusSink sink;
        int status, reads, headroomReads, subscriptions, removals;
        float headroom = Float.NaN;
        boolean failedSubscribe, failedStatus, failedHeadroom;
        public void subscribe(PerformanceThermals.StatusSink next) {
            subscriptions++;
            sink = next;
            if (failedSubscribe) throw new UnsupportedOperationException();
        }
        public void unsubscribe() { removals++; }
        public int status() {
            reads++;
            if (failedStatus) throw new UnsupportedOperationException();
            return status;
        }
        public float headroom() {
            headroomReads++;
            if (failedHeadroom) throw new UnsupportedOperationException();
            return headroom;
        }
    }
    private static void check(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }
    public static void main(String[] args) {
        Clock clock = new Clock();
        Source source = new Source();
        PerformanceThermals state = new PerformanceThermals(source, clock);
        state.start();
        check(state.snapshot().equals("1,0,-1,0"), "API29 status works without headroom");
        state.start();
        check(source.subscriptions == 1, "idempotent resume");
        PerformanceThermals.StatusSink first = source.sink;
        clock.now = 9999;
        state.sample();
        check(source.reads == 1, "headroom is not polled faster than ten seconds");
        clock.now = 10000;
        source.status = 2;
        source.headroom = .82f;
        state.sample();
        check(state.snapshot().equals("1,2,820,0"), "real numeric feedback retained");
        state.stop();
        first.changed(6);
        check(state.snapshot().startsWith("0,-1,-1,"), "background invalidates callbacks");
        state.start();
        first.changed(6);
        check(state.snapshot().startsWith("1,2,-1,"), "prior generation cannot update resumed state");
        check(source.headroomReads == 2, "quick resume cannot bypass headroom rate limit");
        source.sink.changed(3);
        check(state.snapshot().startsWith("1,3,-1,"), "current listener feedback admitted");
        clock.now += 10000;
        state.sample();
        check(state.snapshot().equals("1,2,820,0"), "new poll obtains fresh headroom");
        clock.now += 10000;
        source.sink.changed(1);
        clock.now += 10000;
        source.sink.changed(1);
        clock.now += 5001;
        source.sink.changed(1);
        check(state.snapshot().equals("1,1,-1,0"), "frequent status callbacks cannot refresh stale headroom");
        clock.now += 25001;
        check(state.snapshot().equals("1,-1,-1,25001"), "stale feedback is unavailable");
        source.sink.changed(0);
        check(state.snapshot().equals("1,0,-1,0"), "new callback cannot resurrect stale headroom");
        source.failedStatus = true;
        state.sample();
        check(state.snapshot().equals("1,-1,-1,25001"), "platform failure does not invent a reading");
        source.failedStatus = false;
        source.failedHeadroom = true;
        clock.now += 10000;
        state.sample();
        check(state.snapshot().equals("1,2,-1,0"), "unsupported headroom keeps supported status");
        source.status = 99;
        clock.now += 10000;
        state.sample();
        check(state.snapshot().equals("1,-1,-1,0"), "unknown status omitted");
        state.stop();
        source.failedSubscribe = true;
        state.start();
        check(state.snapshot().startsWith("0,-1,-1,"), "subscription failure falls back to manual settings");
        check(source.removals == 3, "each owned listener retired");
        System.out.println("AndroidPerformanceTest: lifecycle, sampling rate, unsupported, freshness PASS");
    }
}
