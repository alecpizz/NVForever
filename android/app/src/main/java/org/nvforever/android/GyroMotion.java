package org.nvforever.android;

/** Relative orientation only; no Android dependencies, camera state or absolute yaw. */
public final class GyroMotion {
    private static final long MAX_GAP = 50_000_000L, MAX_AGE = 100_000_000L;
    private final double[] previous = new double[4];
    private long timestamp, epoch = 1;
    private int rotation = -1;
    private double horizontal, vertical;
    private boolean reference;

    public synchronized void reset() {
        reference = false;
        timestamp = 0;
        horizontal = vertical = 0;
        epoch++;
    }

    public synchronized void sample(long time, double w, double x, double y, double z,
                                    int displayRotation, long now) {
        double norm = Math.sqrt(w*w + x*x + y*y + z*z);
        if (time <= 0 || now < time || now - time > MAX_AGE || displayRotation < 0
                || displayRotation > 3 || !Double.isFinite(norm) || norm < 0.9 || norm > 1.1) {
            reset();
            return;
        }
        w /= norm; x /= norm; y /= norm; z /= norm;
        if (rotation != displayRotation) {
            reset();
            rotation = displayRotation;
        }
        long dt = time - timestamp;
        if (!reference || dt <= 0 || dt > MAX_GAP) {
            reset();
            remember(time, w, x, y, z);
            return;
        }
        // inverse(previous) * current: rotation expressed in the previous device frame.
        double pw = previous[0], px = previous[1], py = previous[2], pz = previous[3];
        double rw = pw*w + px*x + py*y + pz*z;
        double rx = pw*x - px*w - py*z + pz*y;
        double ry = pw*y + px*z - py*w - pz*x;
        double rz = pw*z - px*y + py*x - pz*w;
        if (rw < 0) { rw = -rw; rx = -rx; ry = -ry; rz = -rz; }
        double sine = Math.sqrt(rx*rx + ry*ry + rz*rz);
        double angle = 2 * Math.atan2(sine, Math.max(0, rw));
        remember(time, w, x, y, z);
        if (!Double.isFinite(angle) || angle > 12.0 * dt / 1_000_000_000.0) {
            reset();
            return;
        }
        // Small stationary fusion drift is not an aiming request (design noise floor).
        if (sine < 1e-10 || angle < 0.001 * dt / 1_000_000_000.0) return;
        rx *= angle / sine;
        ry *= angle / sine;
        // Display axes: Android remap AXIS_Y, AXIS_MINUS_X for ROTATION_90.
        double screenX, screenY;
        switch (displayRotation) {
            case 1: screenX = ry; screenY = -rx; break;
            case 2: screenX = -rx; screenY = -ry; break;
            case 3: screenX = -ry; screenY = rx; break;
            default: screenX = rx; screenY = ry;
        }
        // View counts use +x right and +y down; forward points out the device's back.
        horizontal -= screenY;
        vertical -= screenX;
        if (Math.abs(horizontal) > 0.5 || Math.abs(vertical) > 0.5) reset();
    }

    private void remember(long time, double w, double x, double y, double z) {
        previous[0] = w; previous[1] = x; previous[2] = y; previous[3] = z;
        timestamp = time;
        reference = true;
    }

    public synchronized String drain(boolean available, long now) {
        if (reference && (now < timestamp || now - timestamp > MAX_AGE)) reset();
        String result = "{\"version\":1,\"available\":" + available + ",\"epoch\":" + epoch
            + ",\"timestamp\":" + timestamp + ",\"rotation\":" + Math.max(0, rotation)
            + ",\"x\":" + horizontal + ",\"y\":" + vertical + "}";
        horizontal = vertical = 0;
        return result;
    }
}
