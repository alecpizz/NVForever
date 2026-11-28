package org.nvforever.android;

/** JDK-only synthetic orientation cases; no Android device or camera required. */
public final class GyroMotionTest {
    private static int checks;
    private static final long T = 10_000_000_000L, STEP = 10_000_000L;
    private static double field(String json, String key) {
        String prefix = "\"" + key + "\":";
        int start = json.indexOf(prefix) + prefix.length(), end = start;
        while (end < json.length() && json.charAt(end) != ',' && json.charAt(end) != '}') end++;
        return Double.parseDouble(json.substring(start, end));
    }
    private static void close(double actual, double expected) {
        checks++;
        if (!Double.isFinite(actual) || Math.abs(actual - expected) > 1e-6)
            throw new AssertionError(actual + " != " + expected);
    }
    private static void identity(GyroMotion m, long time, int rotation) {
        m.sample(time, 1, 0, 0, 0, rotation, time);
    }
    private static void x(GyroMotion m, long time, double angle, int rotation) {
        m.sample(time, Math.cos(angle/2), Math.sin(angle/2), 0, 0, rotation, time);
    }
    public static void main(String[] args) {
        // All actual Surface rotations, including a landscape-native ROTATION_0.
        double[][] expected = {{0,-.02},{.02,0},{0,.02},{-.02,0}};
        for (int rotation = 0; rotation < 4; rotation++) {
            GyroMotion m = new GyroMotion();
            identity(m, T, rotation);
            x(m, T+STEP, .02, rotation);
            String s = m.drain(true, T+STEP);
            close(field(s,"x"), expected[rotation][0]);
            close(field(s,"y"), expected[rotation][1]);
            close(field(m.drain(true,T+STEP),"y"), 0);
        }
        GyroMotion m = new GyroMotion();
        identity(m,T,0);
        m.sample(T+STEP, Math.cos(.01),0,Math.sin(.01),0,0,T+STEP);
        close(field(m.drain(true,T+STEP),"x"),-.02);
        // q and -q describe the same orientation; a hemisphere flip is not motion.
        m.sample(T+2*STEP,-Math.cos(.01),0,-Math.sin(.01),0,0,T+2*STEP);
        close(field(m.drain(true,T+2*STEP),"x"),0);
        // Rotation changes, long gaps, old clocks and recenter all require a new reference.
        for (int problem = 0; problem < 6; problem++) {
            m = new GyroMotion();
            identity(m,T,0);
            x(m,T+STEP,.02,0);
            switch (problem) {
                case 0: x(m,T+2*STEP,.04,1); break;
                case 1: x(m,T+200_000_000L,.04,0); break;
                case 2: x(m,T-STEP,.04,0); break;
                case 3: m.sample(T+2*STEP,Double.NaN,0,0,0,0,T+2*STEP); break;
                case 4: x(m,T+2*STEP,2,0); break;
                default: m.reset();
            }
            String s = m.drain(true,problem == 1 ? T+200_000_000L : T+2*STEP);
            close(field(s,"x"),0);
            close(field(s,"y"),0);
        }
        // A valid reference may age out while no frame polls it; accumulated input vanishes.
        m = new GyroMotion();
        identity(m,T,0); x(m,T+STEP,.02,0);
        close(field(m.drain(true,T+300_000_000L),"y"),0);
        // Roll cannot turn yaw or pitch. No sensor data is sent as an absolute heading.
        m = new GyroMotion(); identity(m,T,0);
        m.sample(T+STEP,Math.cos(.01),0,0,Math.sin(.01),0,T+STEP);
        String s = m.drain(true,T+STEP);
        close(field(s,"x"),0); close(field(s,"y"),0);
        // A bounded stationary-noise floor does not accumulate slow fusion drift.
        m = new GyroMotion(); identity(m,T,0);
        for (int i = 1; i <= 1000; i++) x(m,T+i*STEP,i*0.000001,0);
        s = m.drain(true,T+1000*STEP);
        close(field(s,"x"),0); close(field(s,"y"),0);
        System.out.println("GyroMotionTest: " + checks + " checks PASS");
    }
}
