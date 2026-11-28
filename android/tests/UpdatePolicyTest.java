package org.nvforever.android;

/** Pure-Java unit tests for UpdatePolicy (APK names, HTTPS/GitHub URLs, SHA-256, progress). */
public final class UpdatePolicyTest {
    private static int checks = 0;

    private static void check(boolean condition, String detail) {
        checks++;
        if (!condition) throw new AssertionError("FAILED: " + detail);
    }

    public static void main(String[] args) {
        check(UpdatePolicy.safeApkName("NVForever-v0.8.1-android-arm64.apk"), "valid apk name");
        check(!UpdatePolicy.safeApkName("../evil.apk"), "reject traversal");
        check(!UpdatePolicy.safeApkName("sub/file.apk"), "reject slash");
        check(!UpdatePolicy.safeApkName("archive.zip"), "reject non-apk");
        check(!UpdatePolicy.safeApkName(".apk"), "reject empty stem");

        String tag = "v0.8.1";
        String name = "NVForever-v0.8.1-android-arm64.apk";
        String official = UpdatePolicy.REPO_DOWNLOADS + tag + "/" + name;
        check(UpdatePolicy.isRepoReleaseAssetUrl(official, tag, name), "official release URL");
        check(!UpdatePolicy.isRepoReleaseAssetUrl("https://evil.example/" + name, tag, name), "reject third-party host");
        check(!UpdatePolicy.isRepoReleaseAssetUrl("http://github.com/LootJunkieAzza/NVForever/releases/download/v0.8.1/" + name, tag, name), "reject http");

        check(UpdatePolicy.isAllowedDownloadUrl(official), "allow github.com");
        check(UpdatePolicy.isAllowedDownloadUrl("https://objects.githubusercontent.com/github-production-release-asset/1234"), "allow objects.githubusercontent.com");
        check(UpdatePolicy.isAllowedDownloadUrl("https://release-assets.githubusercontent.com/asset/1234"), "allow release-assets.githubusercontent.com");
        check(UpdatePolicy.isAllowedDownloadUrl("https://github-releases.githubusercontent.com/asset/1234"), "allow github-releases.githubusercontent.com");
        check(!UpdatePolicy.isAllowedDownloadUrl("https://raw.githubusercontent.com/user/repo/main/app.apk"), "reject raw.githubusercontent.com");
        check(!UpdatePolicy.isAllowedDownloadUrl("https://gist.githubusercontent.com/user/123/raw/app.apk"), "reject gist.githubusercontent.com");
        check(!UpdatePolicy.isAllowedDownloadUrl("http://objects.githubusercontent.com/asset"), "reject http redirect");
        check(!UpdatePolicy.isAllowedDownloadUrl("https://untrusted.example.com/app.apk"), "reject arbitrary redirect host");

        // Metadata redirects (getHttps): nv-forever.com, api.github.com, and pinned GitHub download hosts.
        check(UpdatePolicy.isAllowedMetadataRedirectUrl("https://nv-forever.com/news.json"), "allow nv-forever.com");
        check(UpdatePolicy.isAllowedMetadataRedirectUrl("https://www.nv-forever.com/news.json"), "allow www.nv-forever.com");
        check(UpdatePolicy.isAllowedMetadataRedirectUrl("https://api.github.com/repos/LootJunkieAzza/NVForever/releases"), "allow api.github.com");
        check(UpdatePolicy.isAllowedMetadataRedirectUrl("https://objects.githubusercontent.com/release/SHA256SUMS.txt"), "allow objects.githubusercontent.com for metadata");
        check(!UpdatePolicy.isAllowedMetadataRedirectUrl("http://nv-forever.com/news.json"), "reject http nv-forever.com");
        check(!UpdatePolicy.isAllowedMetadataRedirectUrl("https://raw.githubusercontent.com/user/repo/main/x"), "reject raw.githubusercontent.com");
        check(!UpdatePolicy.isAllowedMetadataRedirectUrl("https://evil.example/news.json"), "reject unknown metadata redirect host");

        byte[] digest = new byte[32];
        for (int i = 0; i < 32; i++) digest[i] = (byte) i;
        String hex = UpdatePolicy.hexSha256(digest);
        check(hex.length() == 64, "64 hex chars");
        check(UpdatePolicy.isValidSha256Hex(hex), "valid lowercase sha256 hex");
        check(UpdatePolicy.isValidSha256Hex(hex.toUpperCase()), "valid uppercase sha256 hex");
        check(!UpdatePolicy.isValidSha256Hex(null), "reject null sha256");
        check(!UpdatePolicy.isValidSha256Hex(""), "reject empty sha256");
        check(!UpdatePolicy.isValidSha256Hex("short"), "reject short sha256");
        check(!UpdatePolicy.isValidSha256Hex("g".repeat(64)), "reject non-hex sha256");
        check(UpdatePolicy.checksumMatches(hex.toUpperCase(), digest), "case-insensitive checksum match");
        check(!UpdatePolicy.checksumMatches("0".repeat(64), digest), "mismatch detected");
        check(!UpdatePolicy.checksumMatches("short", digest), "short hex rejected");

        // APK authenticity verification (package name, signing certificate SHA-256, monotonic versionCode).
        java.util.List<String> signer = java.util.List.of(hex);
        java.util.List<String> signerUpper = java.util.List.of(hex.toUpperCase());
        java.util.List<String> otherSigner = java.util.List.of("a".repeat(64));
        check(UpdatePolicy.verifyApkMetadata("org.nvforever.android", "org.nvforever.android", 700010L, 700011L, signer, signerUpper) == null, "accept matching package, signer, and newer versionCode");
        check(UpdatePolicy.verifyApkMetadata("org.nvforever.android", "com.evil.fake", 700010L, 700011L, signer, signer) != null, "reject mismatched package name");
        check(UpdatePolicy.verifyApkMetadata("org.nvforever.android", null, 700010L, 700011L, signer, signer) != null, "reject null archive package");
        check(UpdatePolicy.verifyApkMetadata("org.nvforever.android", "org.nvforever.android", 700010L, 700010L, signer, signer) != null, "reject equal versionCode");
        check(UpdatePolicy.verifyApkMetadata("org.nvforever.android", "org.nvforever.android", 700010L, 700009L, signer, signer) != null, "reject downgrade versionCode");
        check(UpdatePolicy.verifyApkMetadata("org.nvforever.android", "org.nvforever.android", 700010L, 700011L, signer, otherSigner) != null, "reject mismatched signer");
        check(UpdatePolicy.verifyApkMetadata("org.nvforever.android", "org.nvforever.android", 700010L, 700011L, signer, java.util.List.of()) != null, "reject missing archive signers");
        check(UpdatePolicy.verifyApkMetadata("org.nvforever.android", "org.nvforever.android", 700010L, 700011L, signer, java.util.List.of(hex, "a".repeat(64))) != null, "reject extra untrusted signer");

        check(UpdatePolicy.progressText(5L * 1024 * 1024, 20L * 1024 * 1024).contains("25%"), "progress percent");
        check(UpdatePolicy.progressText(1024 * 1024, -1).contains("1.0 MB"), "unknown total progress");

        System.out.println("UpdatePolicyTest: PASS (" + checks + " checks)");
    }
}
