package org.nvforever.android;

import android.app.Activity;
import android.app.ActivityManager;
import android.app.AlertDialog;
import android.app.KeyguardManager;
import android.content.Context;
import android.content.Intent;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.ResultReceiver;
import android.os.SystemClock;
import android.view.View;
import java.lang.ref.WeakReference;

/** Main-task cover outlives clearing the separate native game task. */
public final class RestartActivity extends Activity {
    static final String REQUEST = "loading_request", RECEIVER = "loading_receiver";
    static final int OVERLAY = 1, SUBMITTED = 2, FAILURE = 3;
    private static HandoffCoordinator session;
    private static WeakReference<RestartActivity> current = new WeakReference<>(null);
    private final Handler handler = new Handler(Looper.getMainLooper());
    private View cover;
    private boolean resumed, destroyed, errorShown;
    private String creationError;
    private long committedEpoch = -1;
    private final Runnable poll = () -> update();
    private final HandoffCoordinator.Processes processes = new HandoffCoordinator.Processes() {
        public boolean alive(int pid) { return process(RestartActivity.this, pid) != null; }
        public boolean owned(int pid) { return ownedGame(RestartActivity.this, pid); }
        public int successor(int oldPid) {
            var list = ((ActivityManager)getSystemService(ACTIVITY_SERVICE)).getRunningAppProcesses();
            if (list == null) throw new IllegalStateException("Process ownership unavailable");
            for (var p : list) if (p.pid != oldPid && ownedGame(RestartActivity.this, p.pid)) return p.pid;
            return -1;
        }
        public void stop(int pid) { if (owned(pid)) android.os.Process.killProcess(pid); }
        public void launch(String request, String args) {
            Intent next = new Intent(RestartActivity.this, GameActivity.class)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TASK)
                .putExtra("args", args).putExtra(REQUEST, request)
                .putExtra(RECEIVER, new ReadyReceiver(getApplicationContext(), request));
            startActivity(next, android.app.ActivityOptions.makeCustomAnimation(RestartActivity.this, 0, 0).toBundle());
        }
    };
    @Override public void onCreate(Bundle state) {
        super.onCreate(state); current = new WeakReference<>(this);
        LoadingUi.immersive(this); cover = LoadingUi.create(this); setContentView(cover);
        String request = getIntent().getStringExtra(REQUEST), args = getIntent().getStringExtra("args");
        try {
            if (session == null || (session.terminal() && !session.request.equals(request))) {
                session = new HandoffCoordinator(request, args, getIntent().getIntExtra("pid", -1), SystemClock.uptimeMillis());
                // Lost main-process ownership must not replay a partially completed launch.
                if (state != null) session.fail("The loading connection was interrupted. Return to the launcher.");
            }
        } catch (IllegalArgumentException invalid) {
            creationError = "The loading request could not be identified."; return;
        }
    }
    @Override protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        // The active request owns this activity; duplicate or competing intents do not launch again.
        if (session == null || (session.terminal() && !session.request.equals(intent.getStringExtra(REQUEST)))) {
            setIntent(intent); committedEpoch = -1; errorShown = false;
            try {
                session = new HandoffCoordinator(intent.getStringExtra(REQUEST), intent.getStringExtra("args"), intent.getIntExtra("pid", -1), SystemClock.uptimeMillis());
                schedule();
            } catch (IllegalArgumentException invalid) { creationError = "The loading request could not be identified."; showError(creationError); }
        }
    }
    @Override protected void onSaveInstanceState(Bundle state) { state.putBoolean("loading_session", true); super.onSaveInstanceState(state); }
    @Override protected void onResume() { super.onResume(); resumed = true; LoadingUi.immersive(this); if (creationError != null) showError(creationError); else schedule(); }
    @Override protected void onPause() {
        resumed = false;
        if (session != null && session.stage() == HandoffCoordinator.Stage.COVERING) session.foreground(false, SystemClock.uptimeMillis());
        super.onPause();
    }
    @Override public void onWindowFocusChanged(boolean focused) { super.onWindowFocusChanged(focused); if (focused) { LoadingUi.immersive(this); schedule(); } }
    @Override protected void onDestroy() { destroyed = true; handler.removeCallbacks(poll); if (current.get() == this) current.clear(); super.onDestroy(); }
    private void schedule() { handler.removeCallbacks(poll); if (!destroyed) handler.post(poll); }
    private boolean appVisible() {
        KeyguardManager keyguard = (KeyguardManager)getSystemService(KEYGUARD_SERVICE);
        if (keyguard != null && keyguard.isKeyguardLocked()) return false;
        var list = ((ActivityManager)getSystemService(ACTIVITY_SERVICE)).getRunningAppProcesses();
        if (list == null) return resumed || (session != null && session.stage() == HandoffCoordinator.Stage.WAITING);
        for (var p : list) if (p.uid == android.os.Process.myUid()
                && p.importance <= ActivityManager.RunningAppProcessInfo.IMPORTANCE_VISIBLE) return true;
        return false;
    }
    private void update() {
        if (destroyed || session == null) return;
        boolean canAct = session.stage() == HandoffCoordinator.Stage.WAITING ? appVisible() : resumed && hasWindowFocus() && appVisible();
        session.foreground(canAct, SystemClock.uptimeMillis());
        if (session.stage() == HandoffCoordinator.Stage.COVERING && canAct && committedEpoch != session.coverEpoch()) {
            long epoch = session.coverEpoch(); String id = session.request; committedEpoch = epoch;
            LoadingUi.committed(cover, () -> runOnUiThread(() -> {
                if (!destroyed && session != null) { session.coverCommitted(id, epoch); schedule(); }
            }));
        }
        session.tick(SystemClock.uptimeMillis(), processes);
        if (session.stage() == HandoffCoordinator.Stage.COMPLETE) { finish(); return; }
        if (session.stage() == HandoffCoordinator.Stage.FAILED) { showError(session.error()); return; }
        if (session.stage() == HandoffCoordinator.Stage.CANCELLED) return;
        handler.postDelayed(poll, 100);
    }
    @Override public void onBackPressed() {
        new AlertDialog.Builder(this).setTitle("Return to launcher?")
            .setMessage("This stops the current loading request. Saves are kept.")
            .setNegativeButton("Keep loading", null).setPositiveButton("Return", (dialog, which) -> leave()).show();
    }
    private void showError(String message) {
        if (errorShown || destroyed || !resumed) return; errorShown = true;
        new AlertDialog.Builder(this).setTitle("Loading stopped").setMessage(message)
            .setCancelable(false).setPositiveButton("Return to launcher", (dialog, which) -> leave()).show();
    }
    private void leave() {
        if (session != null) session.cancel(processes);
        handler.removeCallbacks(poll);
        startActivity(new Intent(this, LauncherActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP));
        finish();
    }
    private static ActivityManager.RunningAppProcessInfo process(Context context, int pid) {
        var list = ((ActivityManager)context.getSystemService(ACTIVITY_SERVICE)).getRunningAppProcesses();
        if (list == null) throw new IllegalStateException("Process ownership unavailable");
        for (var p : list) if (p.pid == pid) return p;
        return null;
    }
    static boolean ownedGame(Context context, int pid) {
        try {
            var p = process(context, pid);
            return p != null && p.uid == android.os.Process.myUid() && (context.getPackageName() + ":game").equals(p.processName);
        } catch (IllegalStateException unavailable) { return false; }
    }
    private static final class ReadyReceiver extends ResultReceiver {
        private final Context context;
        private final String request;
        ReadyReceiver(Context context, String request) { super(new Handler(Looper.getMainLooper())); this.context = context; this.request = request; }
        @Override protected void onReceiveResult(int code, Bundle data) {
            HandoffCoordinator active = session;
            if (active == null || data == null || !request.equals(data.getString(REQUEST)) || !active.request.equals(request)) return;
            int pid = data.getInt("pid", -1); boolean owned = ownedGame(context, pid);
            if (code == OVERLAY) active.overlay(request, pid, owned);
            else if (code == SUBMITTED) active.submitted(request, pid, owned);
            else if (code == FAILURE && owned && active.successorPid() == pid) active.fail("The game stopped while loading.");
            RestartActivity activity = current.get(); if (activity != null) activity.schedule();
        }
    }
}
