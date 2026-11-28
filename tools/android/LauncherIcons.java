/* Derive Android launcher resources from NVForever's own canonical icon.
 * Run from the repository root with Java 17:
 *   java -Djava.awt.headless=true tools/android/LauncherIcons.java
 * No fonts, game files, third-party art or imaging dependencies are used.
 */
import java.awt.AlphaComposite;
import java.awt.Color;
import java.awt.Graphics2D;
import java.awt.RenderingHints;
import java.awt.image.BufferedImage;
import java.nio.file.Files;
import java.nio.file.Path;
import javax.imageio.ImageIO;

class LauncherIcons {
    private static final String[] DENSITIES = {"mdpi", "hdpi", "xhdpi", "xxhdpi", "xxxhdpi"};
    private static final int[] LEGACY = {48, 72, 96, 144, 192};
    private static final int[] ADAPTIVE = {108, 162, 216, 324, 432};
    private static final Color BACKGROUND = new Color(0x111315);

    public static void main(String[] args) throws Exception {
        boolean check = args.length == 1 && args[0].equals("--check");
        if (args.length != 0 && !check) throw new IllegalArgumentException("Expected no arguments or --check");
        Path source = Path.of("engine/nvforever/assets/icon.png");
        BufferedImage brand = ImageIO.read(source.toFile());
        if (brand == null || brand.getWidth() < 1 || brand.getHeight() < 1)
            throw new IllegalArgumentException("Canonical icon is not a readable image");
        Path resources = Path.of("android/app/src/main/res");
        for (int i = 0; i < DENSITIES.length; i++) {
            Path directory = resources.resolve("mipmap-" + DENSITIES[i]);
            if (!check) Files.createDirectories(directory);
            BufferedImage legacy = render(brand, LEGACY[i], .82, true);
            for (int y = 0; y < legacy.getHeight(); y++) {
                for (int x = 0; x < legacy.getWidth(); x++) {
                    if ((legacy.getRGB(x, y) >>> 24) != 255)
                        throw new IllegalStateException("Legacy background must remain opaque");
                }
            }
            save(directory.resolve("ic_launcher.png"), legacy, check);
            BufferedImage foreground = render(brand, ADAPTIVE[i], 66.0 / 108, false);
            checkSafeZone(foreground);
            save(directory.resolve("ic_launcher_foreground.png"), foreground, check);
            // The canonical silhouette supplies themed artwork; retain its alpha.
            BufferedImage mono = new BufferedImage(foreground.getWidth(), foreground.getHeight(), BufferedImage.TYPE_INT_ARGB);
            for (int y = 0; y < mono.getHeight(); y++) {
                for (int x = 0; x < mono.getWidth(); x++)
                    mono.setRGB(x, y, (foreground.getRGB(x, y) & 0xff000000) | 0x00ffffff);
            }
            save(directory.resolve("ic_launcher_monochrome.png"), mono, check);
        }
    }

    private static BufferedImage render(BufferedImage source, int side, double fraction, boolean background) {
        BufferedImage output = new BufferedImage(side, side, BufferedImage.TYPE_INT_ARGB);
        Graphics2D graphics = output.createGraphics();
        try {
            graphics.setComposite(AlphaComposite.Src);
            if (background) {
                graphics.setColor(BACKGROUND);
                graphics.fillRect(0, 0, side, side);
                graphics.setComposite(AlphaComposite.SrcOver);
            }
            graphics.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_BICUBIC);
            graphics.setRenderingHint(RenderingHints.KEY_RENDERING, RenderingHints.VALUE_RENDER_QUALITY);
            double scale = side * fraction / Math.max(source.getWidth(), source.getHeight());
            if (!background) {
                // A square's corners exceed Android's circular 66 dp safe zone.
                // Fit actual alpha, including the lock, rather than clipping the brand.
                double radius = 0;
                for (int y = 0; y < source.getHeight(); y++) {
                    for (int x = 0; x < source.getWidth(); x++) {
                        if ((source.getRGB(x, y) >>> 24) != 0)
                            radius = Math.max(radius, Math.hypot(Math.abs(x + .5 - source.getWidth() / 2.0) + .5,
                                Math.abs(y + .5 - source.getHeight() / 2.0) + .5));
                    }
                }
                if (radius > 0) scale = Math.min(scale, (side * 33.0 / 108 - 2) / radius);
            }
            int width = Math.max(1, (int)Math.round(source.getWidth() * scale));
            int height = Math.max(1, (int)Math.round(source.getHeight() * scale));
            graphics.drawImage(source, (side - width) / 2, (side - height) / 2, width, height, null);
        } finally { graphics.dispose(); }
        return output;
    }

    private static void save(Path destination, BufferedImage image, boolean check) throws Exception {
        if (check) {
            BufferedImage actual = ImageIO.read(destination.toFile());
            if (actual == null || actual.getWidth() != image.getWidth() || actual.getHeight() != image.getHeight())
                throw new IllegalStateException("Wrong icon dimensions: " + destination);
            for (int y = 0; y < image.getHeight(); y++) {
                for (int x = 0; x < image.getWidth(); x++) {
                    if (actual.getRGB(x, y) != image.getRGB(x, y))
                        throw new IllegalStateException("Icon differs from canonical adaptation: " + destination);
                }
            }
            System.out.println("Checked " + destination);
            return;
        }
        if (!ImageIO.write(image, "png", destination.toFile()))
            throw new IllegalStateException("PNG encoder unavailable");
        System.out.println(destination);
    }

    private static void checkSafeZone(BufferedImage image) {
        double centre = image.getWidth() / 2.0;
        double safe = image.getWidth() * 33.0 / 108;
        int visible = 0;
        for (int y = 0; y < image.getHeight(); y++) {
            for (int x = 0; x < image.getWidth(); x++) {
                if ((image.getRGB(x, y) >>> 24) == 0) continue;
                visible++;
                if (Math.hypot(x + .5 - centre, y + .5 - centre) > safe)
                    throw new IllegalStateException("Adaptive brand exceeds the circular safe zone");
            }
        }
        if (visible == 0) throw new IllegalStateException("Empty launcher artwork");
    }
}
