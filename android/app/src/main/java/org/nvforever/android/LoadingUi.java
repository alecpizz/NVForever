package org.nvforever.android;

import android.app.Activity;
import android.app.Dialog;
import android.graphics.drawable.ColorDrawable;
import android.os.Build;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.Window;
import android.view.WindowManager;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;

/** Original, dependency-free loading appearance. Does not touch the native surface/inset listener. */
final class LoadingUi {
    static final int COVER_FLAGS = WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
        | WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE
        | WindowManager.LayoutParams.FLAG_HARDWARE_ACCELERATED;

    /** Pure policy/lifetime boundary, shared by the real window and host tests. */
    interface WindowBackend {
        Object identity();
        boolean contentInstalled();
        void configure(int flags);
        void installContent();
        void show();
        boolean showing();
        void hide();
        void close();
        void frameCommit(Runnable committed, Runnable unavailable);
        void nextFrame(Runnable action);
    }

    static final class CoverLease {
        private final WindowBackend window;
        private boolean visible, closed;
        private long generation;
        CoverLease(Object nativeWindow, WindowBackend window) {
            if (nativeWindow == null || window.identity() == null || nativeWindow == window.identity())
                throw new IllegalArgumentException("Loading cover needs an independent window");
            if (window.contentInstalled())
                throw new IllegalArgumentException("Loading window policy must precede content");
            this.window = window;
            window.configure(COVER_FLAGS);
            window.installContent();
        }
        boolean show() {
            if (closed) return false;
            if (!visible || !window.showing()) {
                generation++;
                visible = false;
                try { window.show(); visible = window.showing(); }
                catch (RuntimeException unavailable) { close(); return false; }
            }
            return visible;
        }
        long generation() { return generation; }
        private boolean current(long expected) { return !closed && visible && generation == expected && window.showing(); }
        void committed(Runnable action, Runnable unavailable) {
            final long expected = generation;
            if (!current(expected)) return;
            window.frameCommit(() -> { if (current(expected)) action.run(); },
                () -> { if (current(expected)) unavailable.run(); });
        }
        void nextFrame(Runnable action) {
            final long expected = generation;
            if (current(expected)) window.nextFrame(() -> { if (current(expected)) action.run(); });
        }
        enum HideResult { HIDDEN, STALE, FAILED }
        /** Logical visibility only: this is not compositor, pixels or input evidence. */
        HideResult hideForReveal(long expected) {
            if (closed || !visible || generation != expected) return HideResult.STALE;
            try { window.hide(); } catch (RuntimeException possiblyHidden) { /* Inspect actual postcondition below. */ }
            if (closed || !visible || generation != expected) return HideResult.STALE;
            final boolean stillShowing;
            try { stillShowing = window.showing(); }
            catch (RuntimeException unavailable) { return HideResult.FAILED; }
            if (stillShowing) return HideResult.FAILED;
            visible = false; generation++;
            return HideResult.HIDDEN;
        }
        void hide() {
            if (!closed) {
                visible = false; generation++;
                try { window.hide(); } catch (RuntimeException alreadyDetached) { /* Window is leaving. */ }
            }
        }
        void close() {
            if (!closed) {
                closed = true; visible = false; generation++;
                try { window.close(); } catch (RuntimeException alreadyDetached) { /* Activity teardown owns removal. */ }
            }
        }
    }

    /** A normal app Dialog has its own HWUI ViewRoot; NativeActivity keeps its surface. */
    static CoverLease cover(Activity activity) {
        return new CoverLease(activity.getWindow(), new WindowBackend() {
            private final Dialog dialog = new Dialog(activity, android.R.style.Theme_Material_NoActionBar_Fullscreen);
            private View content;
            public Object identity() { return dialog.getWindow(); }
            public boolean contentInstalled() { return content != null; }
            public void configure(int flags) {
                Window window = dialog.getWindow();
                if (window == null) throw new IllegalStateException("Loading window unavailable");
                dialog.setCancelable(false); dialog.setCanceledOnTouchOutside(false);
                window.addFlags(flags | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN);
                window.clearFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND);
                window.setDimAmount(0.0f);
                window.setBackgroundDrawable(new ColorDrawable(0xff031017));
                window.setGravity(android.view.Gravity.FILL);
            }
            public void installContent() {
                content = create(activity);
                dialog.setContentView(content, new android.view.ViewGroup.LayoutParams(
                    android.view.ViewGroup.LayoutParams.MATCH_PARENT, android.view.ViewGroup.LayoutParams.MATCH_PARENT));
            }
            public void show() {
                dialog.show();
                dialog.getWindow().setLayout(android.view.ViewGroup.LayoutParams.MATCH_PARENT,
                    android.view.ViewGroup.LayoutParams.MATCH_PARENT);
                immersive(dialog.getWindow());
            }
            public boolean showing() { return dialog.isShowing(); }
            public void hide() { dialog.hide(); }
            public void close() { dialog.dismiss(); }
            public void frameCommit(Runnable action, Runnable unavailable) {
                boolean queued = content.post(() -> {
                    try {
                        if (!content.isAttachedToWindow() || !content.isHardwareAccelerated()) { unavailable.run(); return; }
                        content.getViewTreeObserver().registerFrameCommitCallback(() -> activity.runOnUiThread(action));
                        content.invalidate();
                    } catch (RuntimeException detached) { unavailable.run(); }
                });
                if (!queued) unavailable.run();
            }
            public void nextFrame(Runnable action) { content.postOnAnimation(action); }
        });
    }
    static View create(Activity activity) {
        LinearLayout root = new LinearLayout(activity);
        root.setOrientation(LinearLayout.VERTICAL); root.setGravity(android.view.Gravity.CENTER);
        root.setBackgroundColor(0xff031017); root.setPadding(32, 32, 32, 32);
        TextView label = new TextView(activity); label.setText("Loading NVForever…");
        label.setTextColor(0xffdcf3eb); label.setTextSize(24); root.addView(label);
        ProgressBar progress = new ProgressBar(activity); root.addView(progress);
        TextView hint = new TextView(activity); hint.setText("Back returns to the launcher.");
        hint.setTextColor(0xffb4c9c4); hint.setTextSize(16); root.addView(hint);
        return root;
    }
    static void immersive(Activity activity) {
        immersive(activity.getWindow());
    }
    private static void immersive(Window window) {
        View decor = window.getDecorView();
        if (Build.VERSION.SDK_INT >= 30) {
            window.setDecorFitsSystemWindows(false);
            WindowInsetsController controller = decor.getWindowInsetsController();
            if (controller != null) {
                controller.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
                controller.hide(WindowInsets.Type.statusBars() | WindowInsets.Type.navigationBars());
            }
        } else decor.setSystemUiVisibility(View.SYSTEM_UI_FLAG_FULLSCREEN | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
            | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
            | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION | View.SYSTEM_UI_FLAG_LAYOUT_STABLE);
    }
    static void committed(View view, Runnable callback) {
        // API29 minimum: actual Java/HWUI frame submission, never a native-frame claim.
        view.post(() -> {
            if (!view.isAttachedToWindow() || !view.isHardwareAccelerated()) return;
            view.getViewTreeObserver().registerFrameCommitCallback(callback);
            view.invalidate();
        });
    }
    private LoadingUi() { }
}
