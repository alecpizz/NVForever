package org.nvforever.android;
import android.app.NativeActivity;
import android.app.AlertDialog;
import android.content.Intent;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.ResultReceiver;
import android.os.SystemClock;
import android.view.SurfaceHolder;
import android.view.WindowInsets;
import android.view.ViewTreeObserver;
import java.util.concurrent.atomic.AtomicBoolean;
/** Only the game lives in :game; native state is fresh for every save load. */
public final class GameActivity extends NativeActivity {
    /** APK debug flag, independent of the native library's Rust compilation profile. */
    public String renderDiagnosticsBuildType() {
        return (getApplicationInfo().flags & android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE) != 0 ? "debug" : "release";
    }
    /** Local opt-in through Android's per-tag logging level; no upload capability. */
    public String renderAdmissionLog() {
        return android.util.Log.isLoggable("NVFRenderAdmission", android.util.Log.DEBUG) ? "enabled" : "disabled";
    }
    private ControllerDevices controllers;
    private AndroidGyro gyro;
    private AndroidPerformance performance;
    private volatile float touchBottomDp = 24.0f;
    private volatile String touchInsetsSnapshot = "1,0,0,0,0,0,0";
    private final ViewTreeObserver.OnGlobalLayoutListener touchInsetsObserver = this::refreshTouchInsets;
    private final Handler loadingHandler = new Handler(Looper.getMainLooper());
    private final AtomicBoolean restartQueued = new AtomicBoolean();
    private LoadingState loading;
    private LoadingUi.CoverLease loadingCover;
    private ResultReceiver receiver;
    private AlertDialog loadingDialog;
    private boolean destroyed, resumed;
    private String loadRefusal;
    private long committedEpoch = -1, committedWindow = -1;
    private final Runnable watchdog = new Runnable() { public void run() {
        if (destroyed || loading == null) return;
        loading.tick(SystemClock.uptimeMillis());
        if (loadRefusal != null) showLoadRefusal();
        else if (loading.failed()) showLoadingBack(true);
        if (loading.blocking()) loadingHandler.postDelayed(this, 250);
    }};
    @Override public void onCreate(Bundle state) {
        String id = getIntent().getStringExtra(RestartActivity.REQUEST);
        if (id == null || !id.matches("[0-9a-f]{32}")) id = java.util.UUID.randomUUID().toString().replace("-", "");
        loading = new LoadingState(id, SystemClock.uptimeMillis());
        receiver = getIntent().getParcelableExtra(RestartActivity.RECEIVER);
        // Metadata and request state exist before NativeActivity starts its native thread.
        controllers = new ControllerDevices((android.hardware.input.InputManager)getSystemService(INPUT_SERVICE));
        gyro = new AndroidGyro(this);
        performance = new AndroidPerformance(this);
        super.onCreate(state);
        LoadingUi.immersive(this);
        // NativeActivity owns the application surface: Java cover frames need a
        // separate, noninteractive Window, never children of the taken surface.
        try { loadingCover = LoadingUi.cover(this); }
        catch (RuntimeException unavailable) { loading.fail(); }
        getWindow().getDecorView().getViewTreeObserver().addOnGlobalLayoutListener(touchInsetsObserver);
        refreshTouchInsets(); refreshCover();
    }
    @Override protected void onPause() {
        if (gyro != null) gyro.resumed(false);
        if (performance != null) performance.stop();
        resumed = false;
        if (loading != null) loading.resumed(false, SystemClock.uptimeMillis());
        if (loadingCover != null) loadingCover.hide();
        loadingHandler.removeCallbacks(watchdog);
        if (controllers != null) controllers.cancelHeldInput();
        super.onPause();
    }
    @Override protected void onResume() {
        resumed = true;
        if (loading != null) loading.resumed(true, SystemClock.uptimeMillis());
        if (controllers != null) controllers.cancelHeldInput();
        super.onResume();
        if (gyro != null) gyro.resumed(true);
        if (performance != null) performance.start();
        LoadingUi.immersive(this); refreshCover();
        refreshTouchInsets();
    }
    @Override public void onWindowFocusChanged(boolean focused) {
        if (gyro != null) gyro.focused(focused);
        if (loading != null) loading.focused(focused, SystemClock.uptimeMillis());
        if (controllers != null) controllers.cancelHeldInput();
        super.onWindowFocusChanged(focused);
        if (focused) LoadingUi.immersive(this);
        refreshCover();
    }
    @Override public void surfaceCreated(SurfaceHolder holder) {
        if (loading != null) loading.surface(true, SystemClock.uptimeMillis());
        super.surfaceCreated(holder); refreshCover();
    }
    @Override public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        if (loading != null) loading.surface(width > 0 && height > 0, SystemClock.uptimeMillis());
        super.surfaceChanged(holder, format, width, height); refreshCover();
    }
    @Override public void surfaceDestroyed(SurfaceHolder holder) {
        if (loading != null) loading.surface(false, SystemClock.uptimeMillis());
        super.surfaceDestroyed(holder); refreshCover();
    }
    @Override protected void onDestroy() {
        if (gyro != null) gyro.close();
        if (performance != null) performance.close();
        destroyed = true; loadingHandler.removeCallbacks(watchdog);
        if (loadingCover != null) loadingCover.close();
        if (loadingDialog != null) loadingDialog.dismiss();
        getWindow().getDecorView().getViewTreeObserver().removeOnGlobalLayoutListener(touchInsetsObserver);
        if (controllers != null) controllers.close();
        super.onDestroy();
    }
    private void refreshCover() {
        if (destroyed || loading == null) return;
        loadingHandler.removeCallbacks(watchdog);
        if (!resumed || !hasWindowFocus() || (!loading.blocking() && !restartQueued.get())) {
            if (loadingCover != null) loadingCover.hide();
            return;
        }
        if (loadingCover == null || !loadingCover.show()) { coverUnavailable(); return; }
        loadingHandler.post(watchdog);
        final long generation = loadingCover.generation();
        if (!loading.eligible() || (committedEpoch == loading.epoch() && committedWindow == generation)) return;
        final long epoch = loading.epoch(); committedEpoch = epoch; committedWindow = generation;
        loadingCover.committed(() -> {
            if (destroyed || loading == null || epoch != loading.epoch()) return;
            loading.overlayCommitted(epoch);
            if (loading.eligible()) { sendLoading(RestartActivity.OVERLAY); revealIfReady(); }
        }, this::coverUnavailable);
    }
    private void coverUnavailable() {
        if (destroyed || loading == null) return;
        loading.fail();
        showLoadingBack(true);
    }
    private void sendLoading(int code) {
        if (receiver == null || loading == null) return;
        Bundle result = new Bundle(); result.putString(RestartActivity.REQUEST, loading.request);
        result.putInt("pid", android.os.Process.myPid()); result.putLong("epoch", loading.epoch());
        receiver.send(code, result);
    }
    /** Called from JNI. A lifecycle event never marks a native frame ready. */
    public String loadingState() {
        if (loading == null) return "";
        if (restartQueued.get()) return loading.request + ":" + loading.epoch() + ":0";
        return loading.state();
    }
    public String beginNewGameCover() {
        final String before = loading == null ? "" : loading.ticket();
        runOnUiThread(() -> {
            if (destroyed || restartQueued.get() || loading == null || loading.failed()) return;
            loading.beginEpisode(SystemClock.uptimeMillis()); refreshCover();
        });
        return before;
    }
    public String contentCoverReady() { return loading != null && loading.coverReady() ? loading.ticket() : ""; }
    public String nativeContentSubmitted(String ticket) {
        runOnUiThread(() -> { if (!destroyed && !restartQueued.get() && loading.nativeSubmitted(ticket)) revealIfReady(); });
        return "";
    }
    private void revealIfReady() {
        LoadingReveal.schedule(new LoadingReveal.Host() {
            public LoadingState state() { return loading; }
            public LoadingUi.CoverLease cover() { return loadingCover; }
            public boolean active() { return !destroyed && !restartQueued.get(); }
            public void completed() {
                loadingHandler.removeCallbacks(watchdog);
                sendLoading(RestartActivity.SUBMITTED);
            }
            public void failed() { coverUnavailable(); }
        });
    }
    public String loadRefused(String reason) {
        runOnUiThread(() -> {
            if (destroyed) return;
            loadRefusal = reason == null ? "The save could not be loaded." : reason.substring(0, Math.min(reason.length(), 4096));
            showLoadRefusal();
        });
        return "";
    }
    private void showLoadRefusal() {
        if (destroyed || !resumed || !hasWindowFocus() || loadRefusal == null || (loadingDialog != null && loadingDialog.isShowing())) return;
        loadingDialog = new AlertDialog.Builder(this).setTitle("The save could not be loaded")
            .setMessage(loadRefusal + "\n\nYour saves are kept. You can choose another save from the main menu.")
            .setCancelable(false).setNegativeButton("Return to launcher", (d, which) -> returnToLauncher())
            .setPositiveButton("Main menu", (d, which) -> { loadRefusal = null; }).create();
        loadingDialog.show();
    }
    public String loadingBack() { runOnUiThread(() -> showLoadingBack(false)); return ""; }
    @Override public void onBackPressed() { if (loading != null && (loading.blocking() || restartQueued.get())) showLoadingBack(false); else super.onBackPressed(); }
    private void showLoadingBack(boolean failed) {
        if (destroyed || !resumed || !hasWindowFocus() || loading == null || (!loading.blocking() && !restartQueued.get()) || (loadingDialog != null && loadingDialog.isShowing())) return;
        // A dialog has its own Java input window; NativeActivity owns the game's input queue.
        AlertDialog.Builder dialog = new AlertDialog.Builder(this).setTitle(failed ? "Loading stopped" : "Return to launcher?")
            .setMessage(failed ? "The game could not finish preparing its view. Saves are kept." : "This stops loading. Saves are kept.")
            .setPositiveButton("Return to launcher", (d, which) -> returnToLauncher());
        if (failed) dialog.setCancelable(false); else dialog.setNegativeButton("Keep loading", null);
        loadingDialog = dialog.create(); loadingDialog.show();
    }
    private void returnToLauncher() {
        sendLoading(RestartActivity.FAILURE);
        startActivity(new Intent(this, LauncherActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP));
        finish(); android.os.Process.killProcess(android.os.Process.myPid());
    }
    private void refreshTouchInsets() {
        WindowInsets insets = getWindow().getDecorView().getRootWindowInsets();
        if (insets != null) updateTouchInsets(insets);
    }
    private void updateTouchInsets(WindowInsets insets) {
        int bottom = Math.max(insets.getSystemWindowInsetBottom(), Math.max(insets.getSystemGestureInsets().bottom, insets.getMandatorySystemGestureInsets().bottom));
        if (insets.getDisplayCutout() != null) bottom = Math.max(bottom, insets.getDisplayCutout().getSafeInsetBottom());
        int left = Math.max(insets.getSystemWindowInsetLeft(), Math.max(insets.getSystemGestureInsets().left, insets.getMandatorySystemGestureInsets().left));
        int top = Math.max(insets.getSystemWindowInsetTop(), Math.max(insets.getSystemGestureInsets().top, insets.getMandatorySystemGestureInsets().top));
        int right = Math.max(insets.getSystemWindowInsetRight(), Math.max(insets.getSystemGestureInsets().right, insets.getMandatorySystemGestureInsets().right));
        if (insets.getDisplayCutout() != null) {
            left = Math.max(left, insets.getDisplayCutout().getSafeInsetLeft());
            top = Math.max(top, insets.getDisplayCutout().getSafeInsetTop());
            right = Math.max(right, insets.getDisplayCutout().getSafeInsetRight());
        }
        android.view.View decor = getWindow().getDecorView();
        // One immutable physical-pixel snapshot; native validates size and converts scale.
        touchInsetsSnapshot = "1," + decor.getWidth() + "," + decor.getHeight() + "," + left + "," + top + "," + right + "," + bottom;
        touchBottomDp = bottom / Math.max(0.1f, getResources().getDisplayMetrics().density);
    }
    public String controllerDevices() { return controllers == null ? "" : controllers.snapshot(); }
    public String performanceState() { return performance == null ? "" : performance.snapshot(); }
    public String gyroSample(String command) { return gyro == null ? "" : gyro.poll(command); }
    public String touchSafeBottom() { return Float.toString(touchBottomDp); }
    public String touchSafeInsets() { return touchInsetsSnapshot; }
    public String startupArgs() { String args = getIntent().getStringExtra("args"); return args == null ? "[]" : args; }
    public String restartEngine(String args) {
        try {
            if (args == null || args.length() > 262144) return "Loading arguments exceed bounds";
            org.json.JSONArray parsed = new org.json.JSONArray(args);
            if (parsed.length() > 128) return "Too many loading arguments";
            for (int i = 0; i < parsed.length(); i++) if (!(parsed.get(i) instanceof String)) return "Invalid loading argument";
        } catch (org.json.JSONException invalid) { return "Invalid loading arguments"; }
        if (!restartQueued.compareAndSet(false, true)) return "";
        runOnUiThread(() -> {
            if (loadingCover == null || !loadingCover.show()) { coverUnavailable(); return; }
            LoadingUi.immersive(this);
            String request = java.util.UUID.randomUUID().toString().replace("-", "");
            try {
                startActivity(new Intent(this, RestartActivity.class)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP)
                    .putExtra("args", args).putExtra("pid", android.os.Process.myPid()).putExtra(RestartActivity.REQUEST, request),
                    android.app.ActivityOptions.makeCustomAnimation(this, 0, 0).toBundle());
            } catch (Exception failure) { loading.fail(); showLoadingBack(true); }
        });
        return "";
    }
    public String engineStopped(String reason) {
        sendLoading(RestartActivity.FAILURE);
        runOnUiThread(() -> {
            if (!reason.isEmpty()) startActivity(new Intent(this, LauncherActivity.class)
                .putExtra("error", reason).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP));
            finish(); android.os.Process.killProcess(android.os.Process.myPid());
        });
        return "";
    }
}
