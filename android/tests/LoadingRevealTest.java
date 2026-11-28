package org.nvforever.android;

import java.util.ArrayList;
import java.util.List;

/** Actual shared production coordinator; authored window state is not Android pixels/input. */
public final class LoadingRevealTest {
    private static final String ID = "0123456789abcdef0123456789abcdef";
    private static int checks;
    private static void check(boolean value, String reason) {
        checks++;
        if (!value) throw new AssertionError(reason);
    }
    enum Fault { NONE, NO_OP, BEFORE, AFTER }
    static final class Owner {
        final Object nativeWindow = new Object();
        Object focused = nativeWindow, inputQueue = nativeWindow;
        boolean physicalKeyHeld = true;
    }
    static final class Window implements LoadingUi.WindowBackend {
        final Owner owner;
        final Object identity = new Object();
        final List<Runnable> commits = new ArrayList<>(), animations = new ArrayList<>();
        boolean installed, visible, queryFailsAfterHide;
        int flags, hideCalls;
        Fault fault = Fault.NONE;
        Runnable duringHide;
        Window(Owner owner) { this.owner = owner; }
        public Object identity() { return identity; }
        public boolean contentInstalled() { return installed; }
        public void configure(int value) { check(!installed, "policy before content"); flags = value; }
        public void installContent() { installed = true; }
        public void show() { check(installed && flags == LoadingUi.COVER_FLAGS, "real cover policy"); visible = true; }
        public boolean showing() {
            if (queryFailsAfterHide && hideCalls > 0) throw new IllegalStateException("authored query unavailable");
            return visible;
        }
        public void hide() {
            hideCalls++;
            if (fault == Fault.BEFORE) throw new IllegalStateException("authored before hide");
            if (fault != Fault.NO_OP) visible = false;
            Runnable action = duringHide; duringHide = null;
            if (action != null) action.run();
            if (fault == Fault.AFTER) throw new IllegalStateException("authored after hide");
        }
        public void close() { visible = false; }
        public void frameCommit(Runnable action, Runnable unavailable) { commits.add(action); }
        public void nextFrame(Runnable action) { animations.add(action); }
        void commit() { List<Runnable> batch = new ArrayList<>(commits); commits.clear(); for (Runnable action : batch) action.run(); }
        void animate() { List<Runnable> batch = new ArrayList<>(animations); animations.clear(); for (Runnable action : batch) action.run(); }
    }
    static final class Host implements LoadingReveal.Host {
        final Owner owner = new Owner();
        final LoadingState state = new LoadingState(ID, 0);
        Window window = new Window(owner);
        LoadingUi.CoverLease cover = new LoadingUi.CoverLease(owner.nativeWindow, window);
        boolean active = true, watchdog = true;
        int completed, failed;
        public LoadingState state() { return state; }
        public LoadingUi.CoverLease cover() { return cover; }
        public boolean active() { return active; }
        public void completed() { watchdog = false; completed++; }
        public void failed() { check(state.failed(), "existing failure state precedes recovery callback"); failed++; }
        Host() {
            state.resumed(true, 0); state.focused(true, 0); state.surface(true, 0);
            check(cover.show(), "separate cover visible");
        }
        void ready() {
            final long epoch = state.epoch();
            cover.committed(() -> state.overlayCommitted(epoch), state::fail);
            window.commit();
            check(state.nativeSubmitted(state.ticket()), "matching actual state native witness admitted");
        }
        void queue() { LoadingReveal.schedule(this); }
        void nativeOwnerUnchanged() {
            check(owner.focused == owner.nativeWindow && owner.inputQueue == owner.nativeWindow,
                "coordinator cannot transfer native window/input ownership");
            check(owner.physicalKeyHeld, "coordinator never resets physical input owner");
        }
    }
    private static void successAndAfterThrow() {
        for (Fault fault : new Fault[] {Fault.NONE, Fault.AFTER}) {
            Host h = new Host(); h.ready(); h.window.fault = fault;
            h.queue(); check(h.state.blocking() && h.completed == 0, "queue is not completion");
            h.window.animate();
            check(!h.window.visible && !h.state.blocking() && h.state.state().isEmpty(), "observed logical hide then completed");
            check(h.completed == 1 && h.failed == 0 && !h.watchdog, "one successful completion effect");
            h.queue(); h.window.animate();
            check(h.completed == 1 && h.window.hideCalls == 1, "completed duplicate cannot hide/ack twice");
            h.nativeOwnerUnchanged();
        }
    }
    private static void refusedHide(Fault fault) {
        Host h = new Host(); h.ready(); h.window.fault = fault; h.queue(); h.window.animate();
        check(h.state.blocking(), "failed hide must keep loading blocked");
        check(h.window.visible && h.completed == 0 && h.watchdog, "visible failed cover keeps watchdog/no success");
        check(h.state.failed() && h.failed == 1, "same-epoch failed hide enters existing recovery once");
        check(!h.state.state().isEmpty() && !h.state.nativeSubmitted(h.state.ticket()), "failed epoch cannot unlock or accept late success");
        h.queue(); h.window.animate();
        check(h.completed == 0 && h.failed == 1 && h.window.hideCalls == 1, "no unbounded hide retry");
        h.nativeOwnerUnchanged();
    }
    private static void queryUnavailable() {
        Host h = new Host(); h.ready(); h.window.queryFailsAfterHide = true; h.queue(); h.window.animate();
        check(h.state.blocking() && h.state.failed() && h.completed == 0 && h.failed == 1 && h.watchdog,
            "unavailable hide postcondition fails closed without false success");
        h.nativeOwnerUnchanged();
    }
    private static void notReadyAndInactive() {
        for (int missing = 0; missing < 3; missing++) {
            Host h = new Host();
            if (missing != 0) { long epoch = h.state.epoch(); h.cover.committed(() -> h.state.overlayCommitted(epoch), h.state::fail); h.window.commit(); }
            if (missing != 1) check(h.state.nativeSubmitted(h.state.ticket()), "native evidence setup");
            if (missing == 2) h.active = false;
            h.queue(); h.window.animate();
            check(h.state.blocking() && !h.state.failed() && h.completed == 0 && h.failed == 0,
                "notready/inactive is not success or hide failure");
            check(h.window.hideCalls == 0 && h.watchdog, "no hide before both witnesses/active host");
        }
    }
    private static void staleEpochAndLifecycle() {
        for (int change = 0; change < 4; change++) {
            Host h = new Host(); h.ready(); String old = h.state.ticket(); h.queue();
            if (change == 0) h.state.beginEpisode(1);
            if (change == 1) { h.state.focused(false, 1); h.state.focused(true, 2); }
            if (change == 2) { h.state.resumed(false, 1); h.cover.hide(); h.state.resumed(true, 2); h.cover.show(); }
            if (change == 3) { h.state.surface(false, 1); h.state.surface(true, 2); }
            int before = h.window.hideCalls; h.window.animate();
            check(!h.state.ticket().equals(old) && h.state.blocking() && !h.state.failed(), "fresh lifecycle ticket remains untouched by stale callback");
            check(h.completed == 0 && h.failed == 0 && h.window.hideCalls == before && h.watchdog, "stale callback does not hide/fail/complete new epoch");
            h.ready(); h.queue(); h.window.animate();
            check(h.completed == 1 && !h.state.blocking(), "fresh real frame/native witnesses recover");
            h.nativeOwnerUnchanged();
        }
    }
    private static void closedAndAlreadyHidden() {
        Host closed = new Host(); closed.ready(); closed.queue(); closed.cover.close(); closed.active = false; closed.window.animate();
        check(closed.completed == 0 && closed.failed == 0 && closed.state.blocking(), "closed destroyed callback cannot complete/fail");
        Host hidden = new Host(); hidden.ready(); hidden.queue(); hidden.window.visible = false; hidden.window.animate();
        check(hidden.state.blocking() && hidden.completed == 0 && hidden.failed == 0 && hidden.watchdog,
            "already-hidden before callback does not fabricate an animation witness");
        check(hidden.cover.show(), "actual fresh lease attachment"); hidden.ready(); hidden.queue(); hidden.window.animate();
        check(hidden.completed == 1 && !hidden.state.blocking(), "fresh attachment/commit can finish normally");
        hidden.nativeOwnerUnchanged();
    }
    private static void changedEpisodeDuringHide() {
        for (Fault fault : new Fault[] {Fault.NONE, Fault.NO_OP}) {
            Host h = new Host(); h.ready(); h.window.fault = fault;
            h.window.duringHide = () -> h.state.beginEpisode(1); h.queue(); h.window.animate();
            check(h.state.blocking() && !h.state.failed() && h.completed == 0 && h.failed == 0 && h.watchdog,
                "hide callback cannot acknowledge/fail a new episode");
            h.window.fault = Fault.NONE; h.cover.show(); h.ready(); h.queue(); h.window.animate();
            check(h.completed == 1 && !h.state.blocking(), "fresh episode succeeds without stale completion replay");
        }
    }
    private static void changedLeaseDuringHide() {
        Host h = new Host(); h.ready();
        h.window.duringHide = () -> { h.cover.hide(); check(h.cover.show(), "same lease fresh attachment"); };
        h.queue(); h.window.animate();
        check(h.window.visible && h.state.blocking() && !h.state.failed() && h.completed == 0 && h.failed == 0,
            "old generation cannot complete or fail reattached same lease");
        h.ready(); h.queue(); h.window.animate();
        check(h.completed == 1 && !h.state.blocking(), "fresh same-lease generation completes once");
    }
    private static void changedWindowDuringHide() {
        Host h = new Host(); h.ready(); Window old = h.window;
        old.duringHide = () -> { h.cover.close(); h.window = new Window(h.owner); h.cover = new LoadingUi.CoverLease(h.owner.nativeWindow, h.window); h.cover.show(); };
        h.queue(); old.animate();
        check(h.window != old && h.window.visible && h.state.blocking() && !h.state.failed(), "replacement cover is not hidden or completed by old hide");
        check(h.completed == 0 && h.failed == 0 && h.watchdog, "replacement cover retains recovery/commit ownership");
        h.ready(); h.queue(); h.window.animate();
        check(h.completed == 1 && !h.state.blocking() && !h.window.visible, "fresh replacement commit completes once");
        h.nativeOwnerUnchanged();
    }
    private static void duplicateQueuedCallbacks() {
        Host h = new Host(); h.ready(); h.queue(); h.queue(); h.window.animate();
        check(h.completed == 1 && h.failed == 0 && h.window.hideCalls == 1 && !h.state.blocking(), "two queued callbacks produce one hide/completion");
        h.nativeOwnerUnchanged();
    }
    public static void main(String[] args) {
        if (args.length == 0) {
            for (String name : new String[] {"success-and-after-throw", "not-ready-inactive", "stale-epoch-lifecycle",
                    "closed-already-hidden", "duplicate-queued", "no-op", "before-throw", "query-unavailable",
                    "episode-during-hide", "lease-during-hide", "window-during-hide"}) main(new String[] {name});
            return;
        }
        if (args.length != 1) throw new IllegalArgumentException("one exact source-pinned case required");
        switch (args[0]) {
            case "success-and-after-throw": successAndAfterThrow(); break;
            case "no-op": refusedHide(Fault.NO_OP); break;
            case "before-throw": refusedHide(Fault.BEFORE); break;
            case "query-unavailable": queryUnavailable(); break;
            case "not-ready-inactive": notReadyAndInactive(); break;
            case "stale-epoch-lifecycle": staleEpochAndLifecycle(); break;
            case "closed-already-hidden": closedAndAlreadyHidden(); break;
            case "episode-during-hide": changedEpisodeDuringHide(); break;
            case "lease-during-hide": changedLeaseDuringHide(); break;
            case "window-during-hide": changedWindowDuringHide(); break;
            case "duplicate-queued": duplicateQueuedCallbacks(); break;
            default: throw new IllegalArgumentException("unknown exact case");
        }
        System.out.println("LoadingRevealTest: PASS " + args[0] + " (" + checks + " checks; logical window only)");
    }
}
