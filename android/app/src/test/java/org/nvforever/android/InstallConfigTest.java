package org.nvforever.android;
/** Run directly with javac/java; these checks need neither Android nor game data. */
public final class InstallConfigTest {
    public static void main(String[] args) throws Exception {
        String updated = InstallConfig.updateGamePath("[Game]\nPath=/old\n[Graphics]\nMSAA=4\n", "/new game");
        require(updated.contains("Path=/new game\n[Graphics]\nMSAA=4"), "preserves unrelated settings");
        updated = InstallConfig.updateGamePath("[Graphics]\nMSAA=4\n", "/new");
        require(updated.contains("[Game]\nPath=/new\n"), "adds missing game section");
        updated = InstallConfig.updateGamePath("[gAmE]\r\n pAtH = /old\r\nPath=/duplicate\r\n", "/new");
        require(updated.contains("Path=/new\n") && !updated.contains("/old") && !updated.contains("/duplicate"), "case and duplicate path handling");
        updated = InstallConfig.updateGamePath("[Game]\n; comment\n[Other]\nPath=/keep\n", "/new");
        require(updated.contains("Path=/new\n[Other]\nPath=/keep"), "does not edit other sections");
        InstallConfig.checkName("Fallout - Meshes.bsa");
        for (String bad : new String[]{"", ".", "..", "../escape", "a/b", "a\\b", "nul\0tail"}) {
            boolean rejected = false;
            try { InstallConfig.checkName(bad); } catch (java.io.IOException e) { rejected = true; }
            require(rejected, "rejects unsafe document name");
        }
        boolean rejectedNull = false;
        try { InstallConfig.checkName(null); } catch (java.io.IOException e) { rejectedNull = true; }
        require(rejectedNull, "rejects null document name");
        java.nio.file.Path temp = java.nio.file.Files.createTempDirectory("nvf-import-test-");
        try {
            java.io.File imported = new java.io.File(temp.toFile(), "game-" + java.util.UUID.randomUUID());
            require(imported.mkdir(), "creates synthetic import");
            require(!InstallConfig.ownedImport(temp.toFile(), imported), "unmarked directories are protected");
            require(new java.io.File(imported, ".nvf-import").createNewFile(), "creates ownership marker");
            require(InstallConfig.ownedImport(temp.toFile(), imported), "recognizes an owned direct child");
            require(!InstallConfig.ownedImport(temp.getParent().toFile(), imported), "refuses a nested path with a different parent");
            java.io.File arbitrary = new java.io.File(temp.toFile(), "Data");
            require(arbitrary.mkdir() && new java.io.File(arbitrary, ".nvf-import").createNewFile(), "creates synthetic foreign directory");
            require(!InstallConfig.ownedImport(temp.toFile(), arbitrary), "protects other marked directories");
        } finally {
            try (var paths = java.nio.file.Files.walk(temp)) {
                for (var path : paths.sorted(java.util.Comparator.reverseOrder()).toList()) java.nio.file.Files.delete(path);
            }
        }
        System.out.println("InstallConfig checks passed");
    }
    private static void require(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }
}
