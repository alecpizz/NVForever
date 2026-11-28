package org.nvforever.android;

/** Epoch-scoped native overlay state; UI lifecycle events never acknowledge native rendering. */
final class LoadingState {
    final String request;
    private long epoch = 1, committed = -1, submitted = -1, completed = -1;
    private boolean resumed, focused, surface, failed;
    private long elapsed, sampled;
    LoadingState(String request, long now) {
        if (request == null || !request.matches("[0-9a-f]{32}")) throw new IllegalArgumentException("Invalid loading request");
        this.request = request; sampled = now;
    }
    private void advance(long now) { if (eligible() && now >= sampled) elapsed += Math.min(now - sampled, 300000); sampled = now; }
    private void invalidate() { epoch++; committed = submitted = -1; elapsed = 0; }
    synchronized void resumed(boolean value, long now) { advance(now); if (resumed != value) { resumed = value; invalidate(); } }
    synchronized void focused(boolean value, long now) { advance(now); if (focused != value) { focused = value; invalidate(); } }
    synchronized void surface(boolean value, long now) { advance(now); surface = value; invalidate(); }
    synchronized void beginEpisode(long now) { advance(now); invalidate(); }
    synchronized boolean coverReady() { return blocking() && eligible() && committed == epoch; }
    synchronized boolean eligible() { return resumed && focused && surface && !failed; }
    synchronized boolean blocking() { return failed || completed != epoch; }
    synchronized boolean failed() { return failed; }
    synchronized long epoch() { return epoch; }
    synchronized String state() { return blocking() ? request + ":" + epoch + ":" + (eligible() ? "1" : "0") : ""; }
    synchronized String ticket() { return request + ":" + epoch; }
    synchronized void overlayCommitted(long generation) { if (eligible() && epoch == generation) committed = epoch; }
    synchronized boolean nativeSubmitted(String ticket) {
        if (!eligible() || !ticket().equals(ticket) || !blocking()) return false;
        submitted = epoch; return true;
    }
    synchronized boolean readyToReveal(String ticket) {
        return blocking() && eligible() && ticket().equals(ticket) && committed == epoch && submitted == epoch;
    }
    synchronized boolean reveal(String ticket) {
        if (!readyToReveal(ticket)) return false;
        completed = epoch; return true;
    }
    synchronized void tick(long now) { advance(now); if (blocking() && eligible() && elapsed > 180000) failed = true; }
    synchronized void fail() { failed = true; }
}
