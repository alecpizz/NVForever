package org.nvforever.android;

import java.util.Locale;

/** Pure countdown formatting for the Android launcher's top-right release badge. */
final class CountdownFormat {
    private CountdownFormat() {}

    static final class Snapshot {
        final String header;
        final boolean showDays;
        final String days;
        final String hours;
        final String minutes;
        final String seconds;

        Snapshot(String header, boolean showDays, String days, String hours, String minutes, String seconds) {
            this.header = header;
            this.showDays = showDays;
            this.days = days;
            this.hours = hours;
            this.minutes = minutes;
            this.seconds = seconds;
        }
    }

    /** Return structured countdown segments while target {@code atSeconds} is in the future, else null. */
    static Snapshot of(String version, long atSeconds, long nowSeconds) {
        long left = atSeconds - nowSeconds;
        if (left <= 0) return null;
        long d = left / 86_400L;
        long h = (left % 86_400L) / 3_600L;
        long m = (left % 3_600L) / 60L;
        long s = left % 60L;
        String clean = version == null ? "" : version.trim();
        String tag = clean.isEmpty() ? "" : (clean.startsWith("v") || clean.startsWith("V") ? clean : "v" + clean);
        String header = tag.isEmpty() ? "NEXT RELEASE" : ("NEXT RELEASE " + tag);
        return new Snapshot(
            header,
            d > 0,
            String.format(Locale.US, "%02d", d),
            String.format(Locale.US, "%02d", h),
            String.format(Locale.US, "%02d", m),
            String.format(Locale.US, "%02d", s)
        );
    }
}
