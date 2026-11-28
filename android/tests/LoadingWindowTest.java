package org.nvforever.android;

import java.util.ArrayList;
import java.util.List;

/** Real CoverLease + LoadingState; fake window submissions are not physical rendering evidence. */
public final class LoadingWindowTest {
    private static final String ID = "0123456789abcdef0123456789abcdef";
    private static int checks;
    private static void check(boolean value, String reason) {
        checks++;
        if (!value) throw new AssertionError(reason);
    }
    private static final class InputOwner {
        final Object nativeWindow = new Object();
        Object focused = nativeWindow, touch = nativeWindow;
        int focusChanges;
    }
    private static final class Window implements LoadingUi.WindowBackend {
        final InputOwner owner;
        final Object identity;
        final List<Runnable> commits = new ArrayList<>(), animations = new ArrayList<>();
        final List<String> order = new ArrayList<>();
        boolean installed, showing, accelerated = true, failShow;
        int flags, closed;
        Window(InputOwner owner, Object identity) { this.owner = owner; this.identity = identity; }
        public Object identity() { return identity; }
        public boolean contentInstalled() { return installed; }
        public void configure(int value) {
            check(!installed, "hardware/focus/input policy must be set before content");
            flags = value; order.add("flags");
        }
        public void installContent() {
            check(flags == LoadingUi.COVER_FLAGS, "content cannot precede the complete required window policy");
            installed = true; order.add("content");
        }
        public void show() {
            if (failShow) throw new IllegalStateException("authored window failure");
            check(installed, "window needs its own content");
            order.add("show"); showing = true;
            if ((flags & 8) == 0) { owner.focused = identity; owner.focusChanges++; }
            owner.touch = (flags & 16) == 0 ? identity : owner.nativeWindow;
        }
        public boolean showing() { return showing; }
        public void hide() { showing = false; }
        public void close() { showing = false; closed++; }
        public void frameCommit(Runnable action, Runnable unavailable) {
            if (!accelerated || !showing) unavailable.run(); else commits.add(action);
        }
        public void nextFrame(Runnable action) { animations.add(action); }
        void submitFrame() { List<Runnable> batch = new ArrayList<>(commits); commits.clear(); for (Runnable r : batch) r.run(); }
        void animate() { List<Runnable> batch = new ArrayList<>(animations); animations.clear(); for (Runnable r : batch) r.run(); }
    }
    private static LoadingState eligible() {
        LoadingState state = new LoadingState(ID, 0);
        state.resumed(true, 0); state.focused(true, 0); state.surface(true, 0);
        return state;
    }
    private static void aNativeOwnedSurfaceIsRejectedEvenIfItClaimsHardware() {
        InputOwner owner = new InputOwner();
        Window nativeChild = new Window(owner, owner.nativeWindow);
        boolean rejected = false;
        try { new LoadingUi.CoverLease(owner.nativeWindow, nativeChild); }
        catch (IllegalArgumentException expected) { rejected = true; }
        check(rejected, "a Java child of NativeActivity's owned surface cannot be a cover window");
        check(!nativeChild.installed && nativeChild.order.isEmpty(), "do not mutate native window policy or install content there");
        Window late = new Window(owner, new Object()); late.installed = true;
        rejected = false;
        try { new LoadingUi.CoverLease(owner.nativeWindow, late); }
        catch (IllegalArgumentException expected) { rejected = true; }
        check(rejected && late.order.isEmpty(), "hardware acceleration configured after content is not accepted");
    }
    private static void independentWindowPreservesNativeInputAndNeedsBothRealWitnesses() {
        InputOwner owner = new InputOwner(); Window window = new Window(owner, new Object());
        LoadingUi.CoverLease cover = new LoadingUi.CoverLease(owner.nativeWindow, window);
        LoadingState state = eligible(); final long epoch = state.epoch(); final String ticket = state.ticket();
        check(cover.show(), "independent window can show");
        check(window.order.equals(java.util.Arrays.asList("flags", "content", "show")), "configure before content and show");
        check((window.flags & 0x01000018) == 0x01000018, "HWUI + nonfocusable + nontouchable policy");
        check(owner.focused == owner.nativeWindow && owner.touch == owner.nativeWindow && owner.focusChanges == 0,
            "cover leaves keys and touch with the existing native window/input queue");
        check(state.nativeSubmitted(ticket), "independent matching native presentation evidence");
        check(!state.reveal(ticket), "native frame is not a Java cover-frame acknowledgement");
        cover.committed(() -> state.overlayCommitted(epoch), state::fail);
        check(!state.coverReady() && state.blocking(), "registering a callback is not a frame submission");
        window.submitFrame();
        check(state.coverReady(), "backend frame submission reaches the actual coordinator");
        check(state.reveal(ticket) && !state.blocking(), "both exact epoch witnesses release loading gate");
        cover.hide(); check(!window.showing(), "completion hides the separate window");
        check(owner.focused == owner.nativeWindow && owner.touch == owner.nativeWindow, "hide preserves native ownership");
    }
    private static void hideReattachAndDestroyInvalidateStaleCallbacks() {
        InputOwner owner = new InputOwner(); Window window = new Window(owner, new Object());
        LoadingUi.CoverLease cover = new LoadingUi.CoverLease(owner.nativeWindow, window);
        int[] committed = {0}, unavailable = {0}, frames = {0};
        check(cover.show(), "initial show"); long first = cover.generation();
        cover.committed(() -> committed[0]++, () -> unavailable[0]++);
        cover.nextFrame(() -> frames[0]++);
        cover.hide(); window.submitFrame(); window.animate();
        check(committed[0] == 0 && unavailable[0] == 0 && frames[0] == 0, "hidden callbacks cannot acknowledge or reveal");
        check(cover.show() && cover.generation() != first, "reattach creates a new cover generation even without a LoadingState epoch change");
        cover.committed(() -> committed[0]++, () -> unavailable[0]++);
        window.showing = false; // External attachment loss before its registered callback.
        check(cover.show(), "replacement attachment can show");
        cover.committed(() -> committed[0]++, () -> unavailable[0]++);
        window.submitFrame(); check(committed[0] == 1, "only current attachment frame can acknowledge");
        cover.nextFrame(() -> frames[0]++); cover.committed(() -> committed[0]++, () -> unavailable[0]++);
        cover.close(); cover.close(); window.submitFrame(); window.animate();
        check(window.closed == 1 && !cover.show(), "destruction is final and idempotent");
        check(committed[0] == 1 && frames[0] == 0, "no callback escapes destruction");
    }
    private static void backgroundAndNewGameNeedNewEpochsAndSeparateCoverFrames() {
        InputOwner owner = new InputOwner(); Window window = new Window(owner, new Object());
        LoadingUi.CoverLease cover = new LoadingUi.CoverLease(owner.nativeWindow, window);
        LoadingState state = eligible(); final long old = state.epoch(); String oldTicket = state.ticket();
        cover.show(); cover.committed(() -> state.overlayCommitted(old), state::fail);
        state.resumed(false, 5); cover.hide(); window.submitFrame();
        check(!state.coverReady() && state.blocking(), "background pending frame cannot complete loading");
        state.resumed(true, 10); cover.show(); final long resumed = state.epoch();
        cover.committed(() -> state.overlayCommitted(resumed), state::fail); window.submitFrame();
        check(!state.nativeSubmitted(oldTicket), "pre-background native ticket is stale");
        check(state.nativeSubmitted(state.ticket()) && state.reveal(state.ticket()), "fresh epoch can finish after resume"); cover.hide();
        String menuTicket = state.ticket(); state.beginEpisode(20); cover.show(); final long intro = state.epoch();
        check(!state.nativeSubmitted(menuTicket) && !state.reveal(state.ticket()), "New Game cannot reuse old menu readiness");
        cover.committed(() -> state.overlayCommitted(intro), state::fail);
        check(!state.coverReady(), "new content waits for its separate actual frame"); window.submitFrame();
        check(state.coverReady() && !state.reveal(state.ticket()), "cover frame alone cannot finish New Game");
        check(state.nativeSubmitted(state.ticket()) && state.reveal(state.ticket()), "matching new native content may complete episode");
    }
    private static void failedWindowOrHardwareNeverPretendsReadiness() {
        InputOwner owner = new InputOwner(); Window software = new Window(owner, new Object());
        software.accelerated = false;
        LoadingUi.CoverLease cover = new LoadingUi.CoverLease(owner.nativeWindow, software);
        LoadingState state = eligible(); final long epoch = state.epoch(); cover.show();
        state.nativeSubmitted(state.ticket()); cover.committed(() -> state.overlayCommitted(epoch), state::fail);
        check(state.failed() && state.blocking() && !state.reveal(state.ticket()), "unavailable HWUI fails closed into existing recovery");
        Window broken = new Window(owner, new Object()); broken.failShow = true;
        LoadingUi.CoverLease failed = new LoadingUi.CoverLease(owner.nativeWindow, broken);
        check(!failed.show() && broken.closed == 1, "partially attached failed window is cleaned up");
        check(!failed.show(), "failure cannot loop hidden window retries");
    }
    public static void main(String[] args) {
        aNativeOwnedSurfaceIsRejectedEvenIfItClaimsHardware();
        independentWindowPreservesNativeInputAndNeedsBothRealWitnesses();
        hideReattachAndDestroyInvalidateStaleCallbacks();
        backgroundAndNewGameNeedNewEpochsAndSeparateCoverFrames();
        failedWindowOrHardwareNeverPretendsReadiness();
        System.out.println("LoadingWindowTest: PASS (" + checks + " checks; no Android rendering claim)");
    }
}
