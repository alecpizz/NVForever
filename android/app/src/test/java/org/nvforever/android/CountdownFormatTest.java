package org.nvforever.android;

/** Run directly with javac/java; verifies launcher release countdown badge formatting. */
public final class CountdownFormatTest {
    public static void main(String[] args) {
        CountdownFormat.Snapshot expired = CountdownFormat.of("0.8.0", 1000L, 1000L);
        require(expired == null, "expired target hides countdown");
        require(CountdownFormat.of("0.8.0", 900L, 1000L) == null, "past target hides countdown");

        CountdownFormat.Snapshot multiDay = CountdownFormat.of("0.8.0", 1_000_000L + 2 * 86_400L + 14 * 3_600L + 3 * 60L + 9L, 1_000_000L);
        require(multiDay != null, "future multi-day countdown exists");
        require("NEXT RELEASE v0.8.0".equals(multiDay.header), "adds v prefix to bare version");
        require(multiDay.showDays, "shows days when >= 1 day remains");
        require("02".equals(multiDay.days) && "14".equals(multiDay.hours) && "03".equals(multiDay.minutes) && "09".equals(multiDay.seconds),
            "zero-pads DD:HH:MM:SS segments");

        CountdownFormat.Snapshot subDay = CountdownFormat.of("v0.9.1", 5_000L + 5 * 3_600L + 42L, 5_000L);
        require(subDay != null, "future sub-day countdown exists");
        require("NEXT RELEASE v0.9.1".equals(subDay.header), "preserves existing v prefix");
        require(!subDay.showDays, "omits days box when < 1 day remains");
        require("05".equals(subDay.hours) && "00".equals(subDay.minutes) && "42".equals(subDay.seconds),
            "formats HH:MM:SS accurately");

        CountdownFormat.Snapshot blankVersion = CountdownFormat.of("  ", 200L, 100L);
        require(blankVersion != null && "NEXT RELEASE".equals(blankVersion.header), "falls back to NEXT RELEASE header");

        System.out.println("CountdownFormat checks passed");
    }

    private static void require(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }
}
