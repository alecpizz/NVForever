package org.nvforever.android;

/** Pure request admission/state machine. Android adapters own actual UI/process operations. */
final class HandoffCoordinator {
    enum Stage { COVERING, STOPPING, WAITING, COMPLETE, FAILED, CANCELLED }
    interface Processes {
        boolean alive(int pid);
        boolean owned(int pid);
        int successor(int oldPid);
        void stop(int pid);
        void launch(String request, String args) throws Exception;
    }
    final String request, args;
    final int oldPid;
    private Stage stage = Stage.COVERING;
    private int successor = -1;
    private boolean foreground, committed, overlay;
    private long coverEpoch, elapsed, sampled;
    private String error = "";
    HandoffCoordinator(String request, String args, int oldPid, long now) {
        if (request == null || !request.matches("[0-9a-f]{32}") || args == null || args.length() > 262144 || oldPid <= 0)
            throw new IllegalArgumentException("Invalid private handoff request");
        this.request = request; this.args = args; this.oldPid = oldPid; sampled = now;
    }
    synchronized Stage stage() { return stage; }
    synchronized String error() { return error; }
    synchronized int successorPid() { return successor; }
    synchronized long coverEpoch() { return coverEpoch; }
    synchronized boolean terminal() { return stage == Stage.COMPLETE || stage == Stage.FAILED || stage == Stage.CANCELLED; }
    private void advance(long now) {
        if (foreground && now >= sampled) elapsed += Math.min(now - sampled, 300000);
        sampled = now;
    }
    synchronized void foreground(boolean active, long now) {
        advance(now);
        if (active != foreground && stage == Stage.COVERING) { coverEpoch++; committed = false; }
        foreground = active;
    }
    synchronized void coverCommitted(String id, long epoch) {
        if (stage == Stage.COVERING && foreground && request.equals(id) && coverEpoch == epoch) committed = true;
    }
    synchronized void tick(long now, Processes processes) {
        advance(now);
        if (terminal() || !foreground) return;
        try {
            if (stage == Stage.COVERING) {
                if (elapsed > 15000) { fail("The loading screen could not be prepared."); return; }
                if (!committed) return;
                if (processes.alive(oldPid)) {
                    if (!processes.owned(oldPid)) { fail("The previous game could not be identified."); return; }
                    processes.stop(oldPid);
                }
                stage = Stage.STOPPING; elapsed = 0;
            }
            if (stage == Stage.STOPPING) {
                if (processes.alive(oldPid)) {
                    if (!processes.owned(oldPid)) fail("The previous game could not be identified.");
                    else if (elapsed > 10000) fail("The previous game did not stop.");
                    return;
                }
                // Commit admission before invoking an adapter that can call back immediately.
                stage = Stage.WAITING; elapsed = 0;
                processes.launch(request, args);
            }
            if (stage == Stage.WAITING) {
                int current = processes.successor(oldPid);
                if (successor < 0 && current > 0) successor = current;
                if (successor > 0 && (!processes.alive(successor) || !processes.owned(successor)))
                    fail("The game stopped while loading.");
                else if (elapsed > 180000) fail("Loading took too long. You can return to the launcher.");
            }
        } catch (Exception failure) { fail("The game could not be started. Return to the launcher."); }
    }
    synchronized void overlay(String id, int pid, boolean owned) {
        if (stage != Stage.WAITING || !request.equals(id) || !owned || pid <= 0 || pid == oldPid) return;
        if (successor > 0 && successor != pid) return;
        successor = pid; overlay = true;
    }
    synchronized boolean submitted(String id, int pid, boolean owned) {
        if (stage != Stage.WAITING || !overlay || !request.equals(id) || !owned || successor != pid) return false;
        stage = Stage.COMPLETE; return true;
    }
    synchronized void fail(String message) {
        if (stage == Stage.COMPLETE || stage == Stage.CANCELLED) return;
        stage = Stage.FAILED; error = message;
    }
    synchronized void cancel(Processes processes) {
        if (stage == Stage.COMPLETE || stage == Stage.CANCELLED) return;
        stage = Stage.CANCELLED;
        if (processes.owned(oldPid)) processes.stop(oldPid);
        if (successor > 0 && successor != oldPid && processes.owned(successor)) processes.stop(successor);
    }
}
