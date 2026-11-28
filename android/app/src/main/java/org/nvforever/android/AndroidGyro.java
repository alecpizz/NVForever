package org.nvforever.android;

import android.app.Activity;
import android.hardware.Sensor;
import android.hardware.SensorEvent;
import android.hardware.SensorEventListener;
import android.hardware.SensorManager;
import android.os.SystemClock;
import android.view.Display;

/** Optional game-rotation sensor. Lifecycle changes always discard pending motion. */
public final class AndroidGyro implements SensorEventListener {
    private final Activity activity;
    private final SensorManager manager;
    private final Sensor sensor;
    private final GyroMotion motion = new GyroMotion();
    private final float[] quaternion = new float[4];
    private long lastPoll;
    private boolean resumed, focused, requested, registered, closed, registrationFailed;

    public AndroidGyro(Activity activity) {
        this.activity = activity;
        SensorManager foundManager = null;
        Sensor foundSensor = null;
        try {
            foundManager = (SensorManager) activity.getSystemService(Activity.SENSOR_SERVICE);
            if (foundManager != null) foundSensor = foundManager.getDefaultSensor(Sensor.TYPE_GAME_ROTATION_VECTOR);
        } catch (RuntimeException unavailable) {
            foundManager = null;
        }
        manager = foundManager;
        sensor = foundSensor;
    }

    public synchronized void resumed(boolean value) {
        resumed = value;
        invalidate();
    }
    public synchronized void focused(boolean value) {
        focused = value;
        invalidate();
    }
    public synchronized void close() {
        closed = true;
        requested = false;
        invalidate();
    }
    private void invalidate() {
        registrationFailed = false;
        motion.reset();
        activity.runOnUiThread(this::registration);
    }
    private synchronized void registration() {
        boolean wanted = !closed && resumed && focused && requested && sensor != null;
        if (wanted == registered) return;
        motion.reset();
        try {
            if (registered) manager.unregisterListener(this);
            registered = wanted && manager.registerListener(this, sensor, 10_000, 0);
            registrationFailed = wanted && !registered;
        } catch (RuntimeException unavailable) {
            registered = false;
            registrationFailed = true;
            motion.reset();
        }
    }
    /** Called by JNI. Only these three commands are accepted; no external data. */
    public synchronized String poll(String command) {
        lastPoll = SystemClock.elapsedRealtimeNanos();
        boolean wanted = "on".equals(command);
        if ("reset".equals(command) || requested != wanted) {
            requested = wanted;
            invalidate();
        }
        return motion.drain(sensor != null && !closed && !registrationFailed, SystemClock.elapsedRealtimeNanos());
    }
    @Override public synchronized void onSensorChanged(SensorEvent event) {
        if (!registered || !resumed || !focused || !requested || closed || event.sensor != sensor) return;
        if (SystemClock.elapsedRealtimeNanos() - lastPoll > 250_000_000L) {
            requested = false;
            invalidate();
            return;
        }
        Display display = activity.getWindowManager().getDefaultDisplay();
        if (display == null || event.values.length < 3) { motion.reset(); return; }
        SensorManager.getQuaternionFromVector(quaternion, event.values);
        motion.sample(event.timestamp, quaternion[0], quaternion[1], quaternion[2], quaternion[3],
            display.getRotation(), SystemClock.elapsedRealtimeNanos());
    }
    @Override public void onAccuracyChanged(Sensor sensor, int accuracy) { }
}
