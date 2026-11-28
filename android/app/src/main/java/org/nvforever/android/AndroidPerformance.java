package org.nvforever.android;

import android.content.Context;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.os.PowerManager;
import android.os.SystemClock;

/** Activity-owned API29 feedback; no sensor, window or input ownership. */
public final class AndroidPerformance implements AutoCloseable {
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final PerformanceThermals thermals;
    private boolean running;
    private boolean closed;
    private final Runnable tick = new Runnable() {
        @Override public void run() {
            if (!running || closed) return;
            thermals.sample();
            handler.postDelayed(this, PerformanceThermals.SAMPLE_MS);
        }
    };

    public AndroidPerformance(Context context) {
        PowerManager manager = manager(context);
        thermals = new PerformanceThermals(new PerformanceThermals.Source() {
            private PowerManager.OnThermalStatusChangedListener listener;
            @Override public void subscribe(PerformanceThermals.StatusSink sink) {
                if (manager == null || Build.VERSION.SDK_INT < 29) {
                    throw new UnsupportedOperationException("Thermal feedback unavailable");
                }
                listener = sink::changed;
                manager.addThermalStatusListener(context.getMainExecutor(), listener);
            }
            @Override public void unsubscribe() {
                if (manager != null && listener != null && Build.VERSION.SDK_INT >= 29) {
                    manager.removeThermalStatusListener(listener);
                }
                listener = null;
            }
            @Override public int status() {
                return manager != null && Build.VERSION.SDK_INT >= 29
                        ? manager.getCurrentThermalStatus() : -1;
            }
            @Override public float headroom() {
                return manager != null && Build.VERSION.SDK_INT >= 30
                        ? manager.getThermalHeadroom(0) : Float.NaN;
            }
        }, SystemClock::elapsedRealtime);
    }

    private static PowerManager manager(Context context) {
        try { return context.getSystemService(PowerManager.class); }
        catch (RuntimeException unavailable) { return null; }
    }

    public void start() {
        if (closed || running) return;
        running = true;
        thermals.start();
        handler.postDelayed(tick, PerformanceThermals.SAMPLE_MS);
    }

    public void stop() {
        running = false;
        handler.removeCallbacks(tick);
        thermals.stop();
    }

    public String snapshot() { return thermals.snapshot(); }

    @Override public void close() {
        closed = true;
        stop();
    }
}
