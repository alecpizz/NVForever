package org.nvforever.android;
import java.io.IOException;
/** Configuration editing and untrusted document names, independent of Android APIs. */
final class InstallConfig {
    // Keep in sync with fnv_install::REQUIRED_DATA_FILES (checked by tools/android/tests).
    static final String[] REQUIRED = { "FalloutNV.esm", "Fallout - Textures.bsa", "Fallout - Textures2.bsa",
        "Fallout - Meshes.bsa", "Fallout - Voices1.bsa", "Fallout - Sound.bsa", "Fallout - Misc.bsa", "Update.bsa" };
    static void checkName(String name) throws IOException {
        if (name == null || name.isEmpty() || name.equals(".") || name.equals("..")
                || name.contains("/") || name.contains("\\") || name.indexOf('\0') >= 0)
            throw new IOException("Unsafe document name");
    }
    static boolean ownedImport(java.io.File external, java.io.File root) throws IOException {
        return !java.nio.file.Files.isSymbolicLink(root.toPath())
            && root.getCanonicalFile().getParentFile().equals(external.getCanonicalFile())
            && root.getName().matches("game-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
            && new java.io.File(root, ".nvf-import").isFile();
    }
    static String updateGamePath(String text, String path) {
        StringBuilder out = new StringBuilder();
        boolean game = false, wrote = false, seen = false;
        for (String line : text.split("\\R")) {
            String trimmed = line.trim();
            if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
                if (game && !wrote) { out.append("Path=").append(path).append('\n'); wrote = true; }
                game = trimmed.equalsIgnoreCase("[Game]"); seen |= game;
            }
            if (game && trimmed.matches("(?i)Path\\s*=.*")) {
                if (!wrote) { out.append("Path=").append(path).append('\n'); wrote = true; }
            } else out.append(line).append('\n');
        }
        if (!seen) out.append("[Game]\n");
        if (!wrote) out.append("Path=").append(path).append('\n');
        return out.toString();
    }
}
