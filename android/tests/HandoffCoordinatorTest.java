package org.nvforever.android;

/** Pure CPU admission tests. No lifecycle callback is accepted as native-frame readiness. */
public final class HandoffCoordinatorTest {
    private static int checks;
    private static final String ID = "0123456789abcdef0123456789abcdef";
    private static final String OTHER = "abcdef0123456789abcdef0123456789";
    private static final String ARGS = "[\"--play\",\"--load\",\"synthetic.nvfsave\"]";
    private static void check(boolean value, String message) { checks++; if (!value) throw new AssertionError(message); }
    private static final class Processes implements HandoffCoordinator.Processes {
        boolean oldAlive = true, newAlive, oldOwned = true, newOwned = true, unavailable;
        int stops, launches, lastStopped; String args, request;
        public boolean alive(int pid) { if (unavailable) throw new IllegalStateException("unavailable"); return pid == 42 ? oldAlive : pid == 84 && newAlive; }
        public boolean owned(int pid) { return pid == 42 ? oldAlive && oldOwned : pid == 84 && newAlive && newOwned; }
        public int successor(int old) { return newAlive && newOwned ? 84 : -1; }
        public void stop(int pid) { stops++; lastStopped = pid; if (pid == 84) newAlive = false; }
        public void launch(String request, String args) { launches++; this.request = request; this.args = args; newAlive = true; }
    }
    private static HandoffCoordinator begin(Processes p) {
        HandoffCoordinator c = new HandoffCoordinator(ID, ARGS, 42, 0);
        c.foreground(true, 0); c.coverCommitted(ID, c.coverEpoch()); c.tick(1, p); return c;
    }
    private static void coversBeforeStoppingAndLaunchesOnce() {
        Processes p = new Processes(); HandoffCoordinator c = new HandoffCoordinator(ID, ARGS, 42, 0);
        c.foreground(true, 0); c.tick(100, p);
        check(p.stops == 0 && p.launches == 0, "onCreate/focus/timers cannot stop old game before cover commit");
        c.coverCommitted(OTHER, c.coverEpoch()); c.tick(200, p); check(p.stops == 0, "stale request commit rejected");
        c.coverCommitted(ID, c.coverEpoch()); c.tick(201, p);
        check(p.stops == 1 && p.launches == 0, "actual cover commit admits one owned termination");
        for (int i = 0; i < 10; i++) { c.coverCommitted(ID, c.coverEpoch()); c.tick(202 + i, p); }
        check(p.stops == 1 && p.launches == 0, "duplicate updates never repeat stop or overlap native processes");
        p.oldAlive = false; c.tick(300, p); c.tick(301, p);
        check(p.launches == 1 && p.request.equals(ID) && p.args.equals(ARGS), "one successor with unchanged args");
        c.overlay(ID, 84, true);
        check(!c.submitted(OTHER, 84, true) && !c.submitted(ID, 42, true), "stale owner/request rejected");
        check(!c.submitted(ID, 84, false), "unowned successor cannot finish");
        check(c.submitted(ID, 84, true), "matching owned native submission finishes");
        c.tick(999999, p); check(p.launches == 1 && c.stage() == HandoffCoordinator.Stage.COMPLETE, "completion removes active loop");
    }
    private static void backgroundAndEpochs() {
        Processes p = new Processes(); HandoffCoordinator c = new HandoffCoordinator(ID, ARGS, 42, 0);
        c.foreground(true, 0); long oldEpoch = c.coverEpoch(); c.coverCommitted(ID, oldEpoch);
        c.foreground(false, 1); c.tick(1_000_000, p);
        check(!c.terminal() && p.stops == 0, "background/lock do not terminate, launch or consume timeout");
        c.foreground(true, 1_000_000); c.coverCommitted(ID, oldEpoch); c.tick(1_000_001, p);
        check(p.stops == 0, "resumed cover requires a fresh commit epoch");
        c.coverCommitted(ID, c.coverEpoch()); c.tick(1_000_002, p);
        check(p.stops == 1, "fresh foreground cover can proceed");
    }
    private static void failuresAndCancellation() {
        Processes p = new Processes(); p.unavailable = true;
        HandoffCoordinator unavailable = begin(p);
        check(unavailable.stage() == HandoffCoordinator.Stage.FAILED && p.stops == 0 && p.launches == 0, "unavailable process query fails closed, never means old process exited");
        p = new Processes(); p.oldOwned = false;
        HandoffCoordinator c = begin(p);
        check(c.stage() == HandoffCoordinator.Stage.FAILED && p.stops == 0 && p.launches == 0, "never terminate unowned PID");
        p = new Processes(); c = begin(p); p.oldOwned = false; c.tick(2, p);
        check(c.stage() == HandoffCoordinator.Stage.FAILED && p.launches == 0, "ownership loss while waiting for termination never admits a concurrent successor");
        p = new Processes(); c = begin(p); c.tick(10002, p);
        check(c.stage() == HandoffCoordinator.Stage.FAILED && p.launches == 0, "old process stop timeout is bounded");
        p = new Processes(); p.oldAlive = false; c = begin(p);
        check(p.stops == 0 && p.launches == 1, "already exited old owner is not an excuse to kill another PID");
        c.overlay(ID, 84, true); p.newAlive = false; c.tick(2, p);
        check(c.stage() == HandoffCoordinator.Stage.FAILED, "successor death is an intelligible terminal state");
        p = new Processes(); p.oldAlive = false; c = begin(p); c.tick(180002, p);
        check(c.stage() == HandoffCoordinator.Stage.FAILED, "missing readiness cannot leave an endless hidden loop");
        p = new Processes(); c = begin(p); p.oldAlive = false; c.tick(2, p); c.overlay(ID, 84, true);
        c.cancel(p); check(c.stage() == HandoffCoordinator.Stage.CANCELLED && p.lastStopped == 84 && !p.newAlive, "explicit cancel stops owned successor");
        int stops = p.stops; c.tick(999999, p); c.cancel(p); check(p.stops == stops, "cancel is idempotent");
    }
    private static void loadingRequiresActualSubmissionAndItsOwnFrame() {
        LoadingState s = new LoadingState(ID, 0);
        s.resumed(true, 0); s.focused(true, 0); s.surface(true, 0);
        String ticket = s.ticket();
        check(s.blocking(), "resume/focus/surface are not native readiness");
        s.overlayCommitted(s.epoch()); check(!s.reveal(ticket), "Java UI commit alone never reveals native content");
        check(!s.nativeSubmitted(OTHER + ":" + s.epoch()), "wrong request rejected");
        check(s.nativeSubmitted(ticket) && s.reveal(ticket), "both exact epoch witnesses allow reveal");
        check(!s.blocking() && !s.nativeSubmitted(ticket), "completion is once only");
        check(!s.reveal(ticket), "two queued UI evidence callbacks cannot reveal/acknowledge one epoch twice");
        s.resumed(false, 10); check(s.blocking() && !s.eligible(), "background re-arms without waking device");
        s.resumed(true, 1000000); s.focused(true, 1000000); s.surface(true, 1000000);
        check(!s.nativeSubmitted(ticket) && !s.reveal(ticket), "pre-background surface ticket cannot uncover new surface");
        String resumed = s.ticket(); check(s.nativeSubmitted(resumed), "current epoch native evidence accepted");
        check(!s.reveal(resumed), "native submission before own Java cover commit still waits");
        s.overlayCommitted(s.epoch()); check(s.reveal(resumed), "either evidence order is safe");
        s.surface(false, 1000001); check(s.blocking() && !s.eligible(), "destroyed surface cannot reveal");
    }
    private static void loadingTimeoutDoesNotPretendSuccess() {
        LoadingState s = new LoadingState(ID, 0); s.resumed(true, 0); s.focused(true, 0); s.surface(true, 0);
        s.tick(180001); check(s.failed() && s.blocking(), "native readiness deadline is a failure, never timer success");
        check(!s.nativeSubmitted(s.ticket()) && !s.reveal(s.ticket()), "late evidence cannot silently dismiss failure");
        LoadingState background = new LoadingState(ID, 0); background.tick(9999999);
        check(!background.failed() && background.blocking(), "background/locked loader has no fake completion or timeout");
    }
    private static void sameProcessEpisodeRequiresANewCoverAndNativeEpoch() {
        LoadingState s = new LoadingState(ID, 0); s.resumed(true, 0); s.focused(true, 0); s.surface(true, 0);
        String startup = s.ticket(); s.overlayCommitted(s.epoch()); s.nativeSubmitted(startup); s.reveal(startup);
        s.beginEpisode(5);
        check(s.blocking() && !s.coverReady() && !s.ticket().equals(startup), "same-process New Game re-arms without a process restart");
        check(!s.nativeSubmitted(startup), "old menu native evidence cannot reveal New Game");
        s.overlayCommitted(s.epoch()); check(s.coverReady(), "new cover commit can admit PlayIntro but cannot finish loading");
        check(s.blocking() && !s.reveal(s.ticket()), "cover commit is not decoded intro/native evidence");
        String intro = s.ticket(); s.nativeSubmitted(intro); check(s.reveal(intro), "matching intro/native evidence finishes the same-process episode");
    }
    public static void main(String[] args) {
        coversBeforeStoppingAndLaunchesOnce(); backgroundAndEpochs(); failuresAndCancellation();
        loadingRequiresActualSubmissionAndItsOwnFrame(); loadingTimeoutDoesNotPretendSuccess();
        sameProcessEpisodeRequiresANewCoverAndNativeEpoch();
        System.out.println("HandoffCoordinatorTest: PASS (" + checks + " checks; no rendering claim)");
    }
}
