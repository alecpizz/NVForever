package org.nvforever.android;

import java.net.URI;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;

/** Pure-Java policy checks for in-app GitHub Release APK downloads. */
final class UpdatePolicy {
    static final String REPO_DOWNLOADS = "https://github.com/LootJunkieAzza/NVForever/releases/download/";
    static final long MAX_APK_BYTES = 512L * 1024L * 1024L;
    static final int MAX_REDIRECTS = 5;

    private UpdatePolicy() {}

    static boolean safeApkName(String name) {
        if (name == null || name.length() < 5 || name.contains("..")) return false;
        if (!name.toLowerCase(Locale.ROOT).endsWith(".apk")) return false;
        for (int i = 0; i < name.length(); i++) {
            char c = name.charAt(i);
            boolean ok = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
                || (c >= '0' && c <= '9') || c == '.' || c == '_' || c == '-';
            if (!ok) return false;
        }
        return true;
    }

    static boolean isRepoReleaseAssetUrl(String url, String tag, String name) {
        if (url == null || tag == null || tag.isEmpty() || !safeApkName(name)) return false;
        if (!isSafeComponent(tag)) return false;
        return url.equals(REPO_DOWNLOADS + tag + "/" + name);
    }

    static boolean isSafeComponent(String value) {
        if (value == null || value.isEmpty() || value.equals(".") || value.equals("..")) return false;
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            boolean ok = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
                || (c >= '0' && c <= '9') || c == '.' || c == '_' || c == '-';
            if (!ok) return false;
        }
        return true;
    }

    static boolean isAllowedDownloadUrl(String url) {
        if (!MarkdownParser.isSafeHttpsUrl(url)) return false;
        try {
            URI uri = URI.create(url);
            if (!"https".equalsIgnoreCase(uri.getScheme())) return false;
            String host = uri.getHost();
            if (host == null || host.isEmpty() || uri.getUserInfo() != null) return false;
            String lower = host.toLowerCase(Locale.ROOT);
            return lower.equals("github.com")
                || lower.equals("objects.githubusercontent.com")
                || lower.equals("release-assets.githubusercontent.com")
                || lower.equals("github-releases.githubusercontent.com");
        } catch (IllegalArgumentException e) {
            return false;
        }
    }

    /**
     * Allowed HTTPS redirect hosts for launcher metadata requests (`getHttps`):
     * nv-forever.com (project news/site config), api.github.com (releases API),
     * and the pinned GitHub Release download hosts (for SHA256SUMS.txt redirects).
     */
    static boolean isAllowedMetadataRedirectUrl(String url) {
        if (isAllowedDownloadUrl(url)) return true;
        if (!MarkdownParser.isSafeHttpsUrl(url)) return false;
        try {
            URI uri = URI.create(url);
            if (!"https".equalsIgnoreCase(uri.getScheme())) return false;
            String host = uri.getHost();
            if (host == null || host.isEmpty() || uri.getUserInfo() != null) return false;
            String lower = host.toLowerCase(Locale.ROOT);
            return lower.equals("nv-forever.com")
                || lower.equals("www.nv-forever.com")
                || lower.equals("api.github.com");
        } catch (IllegalArgumentException e) {
            return false;
        }
    }

    /**
     * Verify a downloaded APK's package identity, signing certificates, and monotonic
     * version code against the currently installed app before handing it to Android's
     * package installer. Returns {@code null} when valid, or an error description.
     */
    static String verifyApkMetadata(
            String expectedPackage,
            String archivePackage,
            long installedVersionCode,
            long archiveVersionCode,
            List<String> installedSignersSha256,
            List<String> archiveSignersSha256) {
        if (expectedPackage == null || expectedPackage.isEmpty()
                || archivePackage == null || !expectedPackage.equals(archivePackage)) {
            return "Downloaded APK package name does not match this app";
        }
        if (installedVersionCode <= 0 || archiveVersionCode <= installedVersionCode) {
            return "Downloaded APK version code (" + archiveVersionCode
                + ") is not newer than installed (" + installedVersionCode + ")";
        }
        if (installedSignersSha256 == null || installedSignersSha256.isEmpty()
                || archiveSignersSha256 == null || archiveSignersSha256.isEmpty()) {
            return "Downloaded APK has missing signing certificate information";
        }
        Set<String> installedSet = new HashSet<>();
        for (String s : installedSignersSha256) {
            if (!isValidSha256Hex(s)) return "Invalid installed signing certificate digest";
            installedSet.add(s.toLowerCase(Locale.ROOT));
        }
        Set<String> archiveSet = new HashSet<>();
        for (String s : archiveSignersSha256) {
            if (!isValidSha256Hex(s)) return "Downloaded APK has invalid signing certificate digest";
            archiveSet.add(s.toLowerCase(Locale.ROOT));
        }
        if (!installedSet.equals(archiveSet)) {
            return "Downloaded APK signing certificate does not match this app";
        }
        return null;
    }

    static boolean isValidSha256Hex(String value) {
        if (value == null || value.length() != 64) return false;
        for (int i = 0; i < 64; i++) {
            char c = value.charAt(i);
            boolean hex = (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
            if (!hex) return false;
        }
        return true;
    }

    static String hexSha256(byte[] digest) {
        if (digest == null || digest.length != 32) return "";
        StringBuilder out = new StringBuilder(64);
        for (byte b : digest) {
            out.append(Character.forDigit((b >> 4) & 0xF, 16));
            out.append(Character.forDigit(b & 0xF, 16));
        }
        return out.toString();
    }

    static boolean checksumMatches(String expectedHex, byte[] actualDigest) {
        if (expectedHex == null || expectedHex.length() != 64 || actualDigest == null || actualDigest.length != 32) {
            return false;
        }
        String actual = hexSha256(actualDigest);
        int diff = 0;
        for (int i = 0; i < 64; i++) {
            diff |= Character.toLowerCase(expectedHex.charAt(i)) ^ actual.charAt(i);
        }
        return diff == 0;
    }

    static String formatBytes(long bytes) {
        if (bytes < 0) return "0 B";
        if (bytes < 1024) return bytes + " B";
        if (bytes < 1024 * 1024) return String.format(Locale.ROOT, "%.1f KB", bytes / 1024.0);
        return String.format(Locale.ROOT, "%.1f MB", bytes / (1024.0 * 1024.0));
    }

    static String progressText(long downloaded, long total) {
        if (total > 0) {
            int pct = (int) Math.min(100, (downloaded * 100) / total);
            return "Downloading update… " + formatBytes(downloaded) + " / " + formatBytes(total) + " (" + pct + "%)";
        }
        return "Downloading update… " + formatBytes(downloaded);
    }
}
