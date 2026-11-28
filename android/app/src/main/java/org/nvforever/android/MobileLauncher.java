package org.nvforever.android;

import android.app.AlertDialog;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.content.pm.Signature;
import android.content.pm.SigningInfo;
import android.content.res.ColorStateList;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.net.Uri;
import android.text.TextUtils;
import android.util.Base64;
import android.view.Gravity;
import android.view.View;
import android.view.WindowInsets;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.EditText;
import android.widget.HorizontalScrollView;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.SeekBar;
import android.widget.TextView;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.atomic.AtomicBoolean;
import javax.net.ssl.HttpsURLConnection;
import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/** Native touch presentation of the shared Rust launcher; SAF and launches stay in the Activity. */
final class MobileLauncher {
    static { System.loadLibrary("nvf_android"); }
    private static native String request(String home, String payload) throws IOException;

    private enum Page {
        HOME("Home"), SETTINGS("Settings"), MODS("Mods"), GAME("Game folder");
        final String label;
        Page(String label) { this.label = label; }
    }

    private final LauncherActivity activity;
    private final String home;
    private final LinearLayout root, navigation, content, actions, countdownBadge, countdownDaysBox;
    private final ScrollView scroll;
    private final TextView status, countdownHeader, countdownDays, countdownHours, countdownMinutes, countdownSeconds, countdownDaysSep;
    private final JSONObject edits = new JSONObject();
    private JSONObject state, news, updateInfo;
    private Page page;
    private String tab = "Gameplay";
    private String message = "";
    private String updateProgress = "";
    private boolean busy, running, newsFetching, resumed;
    private boolean updateChecked, updateChecking, updatePrompted, updateDownloading;
    private final AtomicBoolean updateCancel = new AtomicBoolean(false);
    private volatile HttpsURLConnection activeDownload;
    private int newsEpoch;
    private final int bg, panel, card, button, gold, onGold, text, weak;
    private final Runnable tick = this::updateCountdown;
    private final Runnable checkGameExit = this::watchGameExit;

    MobileLauncher(LauncherActivity activity) throws IOException, JSONException {
        this.activity = activity;
        home = new java.io.File(activity.getFilesDir(), "home").getAbsolutePath();
        state = object("state");
        JSONObject palette = state.getJSONObject("theme");
        bg = color(palette, "bg"); panel = color(palette, "panel"); card = color(palette, "card");
        button = color(palette, "button"); gold = color(palette, "gold"); onGold = color(palette, "onGold");
        text = color(palette, "text"); weak = color(palette, "weak");
        page = state.isNull("folder") ? Page.GAME : Page.HOME;

        root = column(); root.setBackgroundColor(bg);
        // API 29 keeps the platform's default fitting; API 30+ delivers the full insets here.
        if (android.os.Build.VERSION.SDK_INT >= 30) root.setOnApplyWindowInsetsListener((v, insets) -> {
            var safe = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout() | WindowInsets.Type.ime());
            v.setPadding(safe.left, safe.top, safe.right, safe.bottom);
            return WindowInsets.CONSUMED;
        });
        LinearLayout header = row(); header.setGravity(Gravity.CENTER_VERTICAL); space(header, 12, 4);
        header.setBackgroundColor(panel);
        ImageView icon = new ImageView(activity);
        icon.setImageResource(R.mipmap.ic_launcher);
        icon.setScaleType(ImageView.ScaleType.FIT_CENTER);
        icon.setContentDescription("NVForever");
        LinearLayout.LayoutParams iconLp = new LinearLayout.LayoutParams(dp(36), dp(36));
        iconLp.rightMargin = dp(10);
        header.addView(icon, iconLp);
        TextView title = label("NVForever", 22, gold); title.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        title.setSingleLine(true); title.setEllipsize(TextUtils.TruncateAt.END);
        header.addView(title, new LinearLayout.LayoutParams(0, dp(44), 1)); title.setGravity(Gravity.CENTER_VERTICAL);
        countdownBadge = column();
        countdownBadge.setGravity(Gravity.END);
        countdownBadge.setVisibility(View.GONE);
        space(countdownBadge, 8, 4);
        GradientDrawable badgeBg = new GradientDrawable();
        badgeBg.setColor(0xFF17130C);
        badgeBg.setStroke(Math.max(1, dp(1.5f)), (gold & 0x00FFFFFF) | 0xCC000000);
        badgeBg.setCornerRadius(dp(8));
        countdownBadge.setBackground(badgeBg);
        countdownHeader = label("NEXT RELEASE", 9, (gold & 0x00FFFFFF) | 0xD8000000);
        countdownHeader.setTypeface(Typeface.MONOSPACE, Typeface.BOLD);
        countdownHeader.setLetterSpacing(0.08f);
        countdownHeader.setSingleLine(true);
        countdownHeader.setGravity(Gravity.END);
        LinearLayout.LayoutParams headerLp = new LinearLayout.LayoutParams(-2, -2);
        headerLp.bottomMargin = dp(2);
        countdownBadge.addView(countdownHeader, headerLp);
        LinearLayout digitsRow = row();
        digitsRow.setGravity(Gravity.CENTER_VERTICAL);
        countdownDays = countdownDigitView();
        countdownDaysBox = countdownSegment(countdownDays, "D");
        countdownDaysSep = countdownColon();
        countdownHours = countdownDigitView();
        countdownMinutes = countdownDigitView();
        countdownSeconds = countdownDigitView();
        digitsRow.addView(countdownDaysBox);
        digitsRow.addView(countdownDaysSep);
        digitsRow.addView(countdownSegment(countdownHours, "H"));
        digitsRow.addView(countdownColon());
        digitsRow.addView(countdownSegment(countdownMinutes, "M"));
        digitsRow.addView(countdownColon());
        digitsRow.addView(countdownSegment(countdownSeconds, "S"));
        countdownBadge.addView(digitsRow);
        LinearLayout.LayoutParams badgeLp = new LinearLayout.LayoutParams(-2, -2);
        badgeLp.leftMargin = dp(8);
        header.addView(countdownBadge, badgeLp);
        root.addView(header);
        navigation = row();
        root.addView(horizontal(navigation));
        scroll = new ScrollView(activity); scroll.setFillViewport(true);
        content = column(); space(content, 12, 8); scroll.addView(content);
        root.addView(scroll, new LinearLayout.LayoutParams(-1, 0, 1));
        status = label("", 14, weak); space(status, 12, 4); root.addView(status);
        status.setMaxLines(3);
        actions = row(); actions.setBackgroundColor(panel); space(actions, 8, 2); root.addView(actions);
        activity.setContentView(root); root.requestApplyInsets();
        render();
    }

    private static int color(JSONObject colors, String key) throws JSONException { return (int) colors.getLong(key); }
    private int dp(float dp) { return Math.round(dp * activity.getResources().getDisplayMetrics().density); }
    private void space(View view, int x, int y) { view.setPadding(dp(x), dp(y), dp(x), dp(y)); }
    private LinearLayout column() { LinearLayout v = new LinearLayout(activity); v.setOrientation(LinearLayout.VERTICAL); return v; }
    private LinearLayout row() { return new LinearLayout(activity); }
    private TextView label(String value, int size, int color) {
        TextView v = new TextView(activity);
        v.setText(value); v.setTextSize(size); v.setTextColor(color); v.setLineSpacing(dp(2), 1f);
        return v;
    }
    private HorizontalScrollView horizontal(LinearLayout bar) {
        HorizontalScrollView view = new HorizontalScrollView(activity); view.setFillViewport(true);
        view.setHorizontalScrollBarEnabled(false); view.setBackgroundColor(panel); view.addView(bar);
        return view;
    }
    private Button tap(String value, boolean primary, boolean enabled, Runnable click) {
        Button view = new Button(activity);
        view.setAllCaps(false); view.setText(value); view.setTextSize(15);
        view.setMinimumHeight(dp(48)); view.setMinHeight(dp(48)); view.setMinimumWidth(dp(48)); space(view, 12, 4);
        view.setTextColor(primary && enabled ? onGold : enabled ? text : weak);
        GradientDrawable fill = new GradientDrawable(); fill.setColor(primary && enabled ? gold : button); fill.setCornerRadius(dp(8));
        view.setBackground(new RippleDrawable(ColorStateList.valueOf(0x40ffffff), fill, null));
        view.setEnabled(enabled); view.setOnClickListener(v -> click.run());
        return view;
    }
    private void add(LinearLayout parent, View view) {
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(-1, -2);
        lp.bottomMargin = dp(8); parent.addView(view, lp);
    }
    private void addBar(LinearLayout bar, View view, boolean evenly) {
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(evenly ? 0 : -2, -2, evenly ? 1 : 0);
        lp.setMargins(dp(4), dp(2), dp(4), dp(2)); bar.addView(view, lp);
    }
    private LinearLayout card(String title) {
        LinearLayout box = column(); space(box, 12, 10);
        GradientDrawable fill = new GradientDrawable(); fill.setColor(card); fill.setCornerRadius(dp(10));
        box.setBackground(fill); add(content, box);
        if (!title.isEmpty()) add(box, label(title, 19, gold));
        return box;
    }

    private Object send(String action, Object... arguments) throws IOException, JSONException {
        JSONObject payload = new JSONObject(); payload.put("action", action);
        for (int i = 0; i < arguments.length; i += 2) payload.put((String) arguments[i], arguments[i + 1]);
        JSONObject reply = new JSONObject(request(home, payload.toString()));
        if (reply.has("error")) throw new IOException(reply.getString("error"));
        return reply.get("data");
    }
    private JSONObject object(String action, Object... arguments) throws IOException, JSONException {
        return (JSONObject) send(action, arguments);
    }
    private boolean editable() { return !busy && !running; }
    void message(String value) { message = value; status.setText(value); status.setVisibility(value.isEmpty() ? View.GONE : View.VISIBLE); }
    void refresh(boolean busy, boolean running, String message) {
        this.busy = busy; this.running = running;
        if (message != null) this.message = message;
        try { state = object("state"); }
        catch (IOException | JSONException e) { this.message = e.getMessage(); }
        render();
        if (news == null && !newsFetching) fetchNews();
        if (!updateChecked && !updateChecking && state.optBoolean("newsEnabled", true)) checkForUpdates(false);
    }
    void pause() { resumed = false; root.removeCallbacks(tick); root.removeCallbacks(checkGameExit); }
    void resume() { resumed = true; updateCountdown(); watchGameExit(); }
    private void watchGameExit() {
        root.removeCallbacks(checkGameExit);
        if (!resumed || !running) return;
        if (activity.runningGame()) root.postDelayed(checkGameExit, 500);
        else refresh(busy, false, null);
    }
    void validateImport(String path) throws IOException, JSONException { send("validate_import", "path", path); }
    String startArgs(boolean resume) throws IOException, JSONException {
        JSONArray args = (JSONArray) send("start", "resume", resume);
        saveSettings();
        return args.toString();
    }
    void startRefused(String error) {
        refresh(false, false, error);
        if (state.isNull("folder")) showGameFolder(error);
    }
    private void saveSettings() throws IOException, JSONException {
        if (edits.length() == 0) return;
        send("save_settings", "edits", edits);
        var keys = edits.keys();
        while (keys.hasNext()) { keys.next(); keys.remove(); }
    }
    void showGameFolder(String error) { page = Page.GAME; message = error; render(); }

    private void render() {
        navigation.removeAllViews(); actions.removeAllViews(); content.removeAllViews();
        for (Page p : Page.values()) addBar(navigation, tap(p.label, p == page, true, () -> {
            if (page != p) { page = p; scroll.scrollTo(0, 0); render(); }
        }), false);
        message(message == null ? "Launcher could not complete that operation." : message);
        boolean ready = !state.isNull("folder"), canPlay = editable() && ready;
        JSONObject preview = state.optJSONObject("savePreview");
        boolean wide = activity.getResources().getDisplayMetrics().widthPixels >= dp(600);
        actions.setOrientation(wide ? LinearLayout.HORIZONTAL : LinearLayout.VERTICAL);
        actions.setGravity(Gravity.CENTER_VERTICAL);
        if (preview != null && !wide) {
            addSavePreview(actions, preview, canPlay, false);
        }
        LinearLayout buttonRow = wide ? actions : row();
        if (!wide) {
            actions.addView(buttonRow, new LinearLayout.LayoutParams(-1, -2));
        }
        Button play = tap("Play", true, canPlay, () -> activity.prepareAndStart(false));
        if (!ready) play.setContentDescription("Play unavailable · import a complete game folder first");
        addBar(buttonRow, play, true);
        if (!state.isNull("continue")) {
            Button resume = tap("Continue", true, canPlay, () -> activity.prepareAndStart(true));
            resume.setContentDescription("Continue · " + state.optString("continue")); addBar(buttonRow, resume, true);
        }
        if (preview != null && wide) {
            addSavePreview(actions, preview, canPlay, true);
        }
        try {
            switch (page) {
                case HOME -> homePage();
                case SETTINGS -> settingsPage();
                case MODS -> modsPage();
                case GAME -> gamePage();
            }
        } catch (IOException | JSONException e) { message(e.getMessage() == null ? "Could not load this page." : e.getMessage()); }
    }

    private void addSavePreview(LinearLayout parent, JSONObject preview, boolean canPlay, boolean sideBySide) {
        LinearLayout box = row();
        box.setGravity(Gravity.CENTER_VERTICAL);
        space(box, 8, 6);
        GradientDrawable fill = new GradientDrawable();
        fill.setColor(card);
        fill.setCornerRadius(dp(8));
        box.setBackground(new RippleDrawable(ColorStateList.valueOf(0x30ffffff), fill, null));
        String shotB64 = preview.isNull("shotBase64") ? "" : preview.optString("shotBase64", "");
        if (!shotB64.isEmpty()) {
            try {
                byte[] bytes = Base64.decode(shotB64, Base64.DEFAULT);
                Bitmap bmp = BitmapFactory.decodeByteArray(bytes, 0, bytes.length);
                if (bmp != null) {
                    ImageView thumb = new ImageView(activity);
                    thumb.setImageBitmap(bmp);
                    thumb.setScaleType(ImageView.ScaleType.CENTER_CROP);
                    LinearLayout.LayoutParams tlp = new LinearLayout.LayoutParams(dp(84), dp(48));
                    tlp.rightMargin = dp(8);
                    box.addView(thumb, tlp);
                }
            } catch (IllegalArgumentException ignored) {
                // Ignore malformed preview bytes and keep metadata visible.
            }
        }
        LinearLayout meta = column();
        String name = preview.optString("name", "Last save");
        TextView titleView = label(name, 14, gold);
        titleView.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        titleView.setSingleLine(true);
        titleView.setEllipsize(TextUtils.TruncateAt.END);
        meta.addView(titleView);
        String detail = preview.optString("detail", "");
        if (!detail.isEmpty()) {
            TextView detailView = label(detail, 12, text);
            detailView.setSingleLine(true);
            detailView.setEllipsize(TextUtils.TruncateAt.END);
            meta.addView(detailView);
        }
        box.addView(meta, new LinearLayout.LayoutParams(0, -2, 1));
        box.setEnabled(canPlay);
        box.setContentDescription("Continue last save · " + name + (detail.isEmpty() ? "" : " · " + detail));
        if (canPlay) {
            box.setOnClickListener(v -> activity.prepareAndStart(true));
        }
        LinearLayout.LayoutParams lp = sideBySide
            ? new LinearLayout.LayoutParams(0, -2, 1.4f)
            : new LinearLayout.LayoutParams(-1, -2);
        lp.setMargins(dp(4), dp(2), dp(4), dp(4));
        parent.addView(box, lp);
    }

    private void gamePage() {
        LinearLayout box = card("Game folder");
        add(box, label(state.isNull("folder") ? "No complete game folder is ready." : "Ready · imported game files", 16, text));
        add(box, label("Choose your Fallout: New Vegas folder or its Data folder. This makes a full copy in app storage and never changes your originals. Saves and settings are separate; uninstalling removes app files.", 15, weak));
        add(box, tap(state.isNull("folder") ? "Import game folder" : "Replace imported game folder", true, editable(), activity::pickGame));
    }

    private void homePage() throws JSONException {
        LinearLayout about = card("NVForever for Android");
        add(about, label("Experimental build (" + activity.installedVersion() + ") · play with touch, a controller, or a keyboard and mouse.", 15, weak));
        if (state.isNull("folder")) add(about, tap("Set up a game folder", true, true, () -> showGameFolder("Import a complete game folder to enable Play.")));
        add(about, tap(updateChecking ? "Checking for updates…" : "Check for updates", false, !updateChecking && !updateDownloading, () -> checkForUpdates(true)));
        if (updateInfo != null && updateInfo.optBoolean("available", false)) {
            String nextVersion = updateInfo.optString("version", "");
            LinearLayout updateCard = card("Update available: v" + nextVersion);
            String notes = updateInfo.optString("notes", "");
            if (!notes.isEmpty()) addMarkdown(updateCard, notes, 400);
            if (updateDownloading) {
                add(updateCard, label(updateProgress.isEmpty() ? "Downloading update…" : updateProgress, 14, gold));
                boolean cancelling = updateCancel.get();
                add(updateCard, tap(cancelling ? "Cancelling…" : "Cancel download", false, !cancelling, () -> {
                    updateCancel.set(true);
                    updateProgress = "Cancelling…";
                    message(updateProgress);
                    HttpsURLConnection conn = activeDownload;
                    if (conn != null) new Thread(conn::disconnect, "nvf-apk-cancel").start();
                    render();
                }));
            } else {
                if (!updateInfo.isNull("asset")) {
                    add(updateCard, tap("Update to v" + nextVersion, true, editable(), () -> confirmAndDownloadUpdate(updateInfo)));
                }
                String releaseUrl = updateInfo.optString("url", "");
                if (!releaseUrl.isEmpty()) add(updateCard, tap("Open release page", false, true, () -> openLink(releaseUrl)));
            }
        }
        LinearLayout feed = card("Project news");
        boolean enabled = state.optBoolean("newsEnabled", true);
        add(feed, tap(enabled ? "Turn news fetching off" : "Turn news fetching on", false, editable(), () -> act(() -> {
            send("set_news", "enabled", !enabled);
            state = object("state");
            news = null; newsEpoch++; newsFetching = false;
            render(); fetchNews();
        })));
        if (news == null || !enabled) {
            add(feed, label(enabled ? "Fetching project news…" : "News fetching is off", 14, weak));
            return;
        }
        add(feed, label(news.getString("source"), 14, weak));
        JSONArray posts = news.getJSONArray("posts");
        if (posts.length() == 0) add(feed, label("No news yet", 16, text));
        for (int i = 0; i < posts.length(); i++) {
            JSONObject post = posts.getJSONObject(i);
            LinearLayout box = card(post.getString("title"));
            add(box, label(post.getString("date") + " · " + post.getString("author"), 14, weak));
            addMarkdown(box, post.getString("body"), 700);
            if (!post.isNull("url")) add(box, tap("Open in browser", false, true, () -> openLink(post.optString("url"))));
        }
    }

    private void addMarkdown(LinearLayout box, String markdown, int previewLimit) {
        List<MarkdownParser.Block> blocks = MarkdownParser.parse(markdown);
        MarkdownParser.Preview preview = MarkdownParser.preview(blocks, previewLimit);
        TextView copy = label("", 16, text);
        MarkdownRenderer.apply(copy, preview.blocks, gold, weak, dp(16), this::openLink);
        add(box, copy);
        if (preview.truncated) {
            Button more = tap("Read more", false, true, () -> {});
            more.setOnClickListener(v -> {
                MarkdownRenderer.apply(copy, blocks, gold, weak, dp(16), this::openLink);
                more.setVisibility(View.GONE);
            });
            add(box, more);
        }
    }

    private void checkForUpdates(boolean userInitiated) {
        if (updateChecking || updateDownloading) return;
        updateChecking = true;
        if (userInitiated && page == Page.HOME) render();
        String current = activity.installedVersion();
        new Thread(() -> {
            try {
                JSONObject result = object("check_update", "current", current);
                activity.runOnUiThread(() -> {
                    updateChecking = false;
                    updateChecked = true;
                    if (activity.isDestroyed()) return;
                    updateInfo = result;
                    if (result.optBoolean("available", false)) {
                        if (page == Page.HOME) render();
                        if (userInitiated || !updatePrompted) {
                            updatePrompted = true;
                            confirmAndDownloadUpdate(result);
                        }
                    } else {
                        if (userInitiated) message("NVForever is up to date (" + current + ").");
                        if (page == Page.HOME) render();
                    }
                });
            } catch (IOException | JSONException e) {
                activity.runOnUiThread(() -> {
                    updateChecking = false;
                    if (activity.isDestroyed()) return;
                    if (userInitiated) message("Could not check for updates.");
                    if (page == Page.HOME) render();
                });
            }
        }, "nvf-update-check").start();
    }

    private void confirmAndDownloadUpdate(JSONObject info) {
        if (activity.isDestroyed() || activity.isFinishing() || updateDownloading) return;
        String nextVersion = info.optString("version", "");
        JSONObject asset = info.optJSONObject("asset");
        String releaseUrl = info.optString("url", "");
        AlertDialog.Builder dialog = new AlertDialog.Builder(activity)
            .setTitle("Update available: v" + nextVersion);
        if (asset != null) {
            long size = asset.optLong("size", 0);
            String sizeText = size > 0 ? " (" + UpdatePolicy.formatBytes(size) + ")" : "";
            dialog.setMessage("NVForever v" + nextVersion + " is available" + sizeText
                + ". Would you like to download and install the update now?")
                .setPositiveButton("Update", (d, which) -> downloadAndInstallUpdate(info))
                .setNegativeButton("Later", null);
        } else {
            dialog.setMessage("NVForever v" + nextVersion + " is available on GitHub, but no ARM64 APK asset is attached yet.")
                .setNegativeButton("Later", null);
            if (!releaseUrl.isEmpty()) dialog.setPositiveButton("Open release page", (d, which) -> openLink(releaseUrl));
        }
        dialog.show();
    }

    private void downloadAndInstallUpdate(JSONObject info) {
        if (updateDownloading) return;
        if (busy || running) {
            message("Close the game before updating.");
            return;
        }
        JSONObject asset = info.optJSONObject("asset");
        if (asset == null) return;
        String tag = info.optString("tag", "");
        String name = asset.optString("name", "");
        String url = asset.optString("url", "");
        long expectedSize = asset.optLong("size", -1);
        String expectedSha = asset.isNull("sha256") ? null : asset.optString("sha256", null);
        if (!UpdatePolicy.isRepoReleaseAssetUrl(url, tag, name)) {
            message("Refused untrusted update download URL.");
            return;
        }
        if (!UpdatePolicy.isValidSha256Hex(expectedSha)) {
            message("Refused update without a valid SHA-256 checksum.");
            return;
        }
        updateDownloading = true;
        updateCancel.set(false);
        updateProgress = "Starting download…";
        message(updateProgress);
        if (page == Page.HOME) render();
        new Thread(() -> {
            File dir = ApkUpdateProvider.updatesDir(activity);
            File part = new File(dir, name + ".part");
            File target = new File(dir, name);
            try {
                if (!dir.isDirectory() && !dir.mkdirs()) throw new IOException("Cannot create updates cache folder");
                byte[] digest = downloadHttpsApk(url, part, expectedSize);
                if (!UpdatePolicy.checksumMatches(expectedSha, digest)) {
                    throw new IOException("Downloaded APK failed SHA-256 verification");
                }
                if (target.exists() && !target.delete()) throw new IOException("Cannot replace previous cached APK");
                if (!part.renameTo(target)) throw new IOException("Cannot finalize downloaded APK");
                String authError = verifyDownloadedApk(activity, target);
                if (authError != null) {
                    target.delete();
                    throw new IOException(authError);
                }
                activity.runOnUiThread(() -> {
                    updateDownloading = false;
                    updateProgress = "";
                    if (activity.isDestroyed()) return;
                    message("Update downloaded. Opening Android installer…");
                    if (page == Page.HOME) render();
                    activity.installUpdateApk(target);
                });
            } catch (Exception e) {
                part.delete();
                boolean cancelled = updateCancel.get();
                activity.runOnUiThread(() -> {
                    updateDownloading = false;
                    updateProgress = "";
                    if (activity.isDestroyed()) return;
                    message(cancelled ? "Update download cancelled." : "Update failed: " + e.getMessage());
                    if (page == Page.HOME) render();
                });
            }
        }, "nvf-apk-download").start();
    }

    private byte[] downloadHttpsApk(String initialUrl, File dest, long expectedSize) throws IOException {
        String currentUrl = initialUrl;
        HttpsURLConnection connection = null;
        try {
            for (int hop = 0; hop <= UpdatePolicy.MAX_REDIRECTS; hop++) {
                if (!UpdatePolicy.isAllowedDownloadUrl(currentUrl)) {
                    throw new IOException("Refused redirect outside GitHub release hosts");
                }
                connection = (HttpsURLConnection) new URL(currentUrl).openConnection();
                activeDownload = connection;
                connection.setInstanceFollowRedirects(false);
                connection.setConnectTimeout(15000);
                connection.setReadTimeout(30000);
                connection.setRequestProperty("User-Agent", "NVForever-Android-Launcher");
                int code = connection.getResponseCode();
                if (code == 301 || code == 302 || code == 303 || code == 307 || code == 308) {
                    String location = connection.getHeaderField("Location");
                    connection.disconnect();
                    connection = null;
                    if (location == null || location.isEmpty()) throw new IOException("Empty redirect location");
                    currentUrl = new URL(new URL(currentUrl), location).toString();
                    continue;
                }
                if (code != 200) throw new IOException("Server returned HTTP " + code);
                long contentLength = connection.getContentLengthLong();
                long total = contentLength > 0 ? contentLength : expectedSize;
                if (total > UpdatePolicy.MAX_APK_BYTES) throw new IOException("Update APK exceeds maximum allowed size");
                MessageDigest sha = MessageDigest.getInstance("SHA-256");
                long downloaded = 0;
                long lastUi = 0;
                try (InputStream in = connection.getInputStream(); FileOutputStream out = new FileOutputStream(dest)) {
                    byte[] buffer = new byte[32768];
                    int n;
                    while ((n = in.read(buffer)) != -1) {
                        if (updateCancel.get()) throw new IOException("Cancelled");
                        downloaded += n;
                        if (downloaded > UpdatePolicy.MAX_APK_BYTES) throw new IOException("Download exceeded size limit");
                        sha.update(buffer, 0, n);
                        out.write(buffer, 0, n);
                        long now = android.os.SystemClock.elapsedRealtime();
                        if (now - lastUi >= 150) {
                            lastUi = now;
                            String text = UpdatePolicy.progressText(downloaded, total);
                            activity.runOnUiThread(() -> {
                                if (!activity.isDestroyed() && updateDownloading) {
                                    updateProgress = text;
                                    message(text);
                                }
                            });
                        }
                    }
                    out.getFD().sync();
                }
                if (downloaded == 0) throw new IOException("Downloaded APK is empty");
                return sha.digest();
            }
            throw new IOException("Too many redirects");
        } catch (NoSuchAlgorithmException e) {
            throw new IOException("SHA-256 unavailable", e);
        } finally {
            activeDownload = null;
            if (connection != null) connection.disconnect();
        }
    }

    private static List<String> signingCertDigests(SigningInfo signingInfo) throws IOException {
        if (signingInfo == null) return List.of();
        Signature[] signatures = signingInfo.hasMultipleSigners()
            ? signingInfo.getApkContentsSigners()
            : signingInfo.getSigningCertificateHistory();
        if (signatures == null || signatures.length == 0) return List.of();
        try {
            MessageDigest sha = MessageDigest.getInstance("SHA-256");
            List<String> out = new ArrayList<>(signatures.length);
            for (Signature sig : signatures) {
                if (sig == null) return List.of();
                sha.reset();
                out.add(UpdatePolicy.hexSha256(sha.digest(sig.toByteArray())));
            }
            return out;
        } catch (NoSuchAlgorithmException e) {
            throw new IOException("SHA-256 unavailable", e);
        }
    }

    private static String verifyDownloadedApk(LauncherActivity activity, File apk) throws IOException {
        PackageManager pm = activity.getPackageManager();
        String expectedPkg = activity.getPackageName();
        PackageInfo installed;
        try {
            installed = pm.getPackageInfo(expectedPkg, PackageManager.GET_SIGNING_CERTIFICATES);
        } catch (PackageManager.NameNotFoundException e) {
            return "Cannot inspect installed package metadata";
        }
        PackageInfo archive = pm.getPackageArchiveInfo(apk.getAbsolutePath(), PackageManager.GET_SIGNING_CERTIFICATES);
        if (archive == null) {
            return "Downloaded file is not a valid Android APK package";
        }
        List<String> installedSigners = signingCertDigests(installed.signingInfo);
        List<String> archiveSigners = signingCertDigests(archive.signingInfo);
        return UpdatePolicy.verifyApkMetadata(
            expectedPkg,
            archive.packageName,
            installed.getLongVersionCode(),
            archive.getLongVersionCode(),
            installedSigners,
            archiveSigners
        );
    }

    private void fetchNews() {
        if (!state.optBoolean("newsEnabled", true)) { updateCountdown(); return; }
        int epoch = ++newsEpoch; newsFetching = true;
        new Thread(() -> {
            try {
                JSONObject result = object("fetch_news");
                activity.runOnUiThread(() -> {
                    if (epoch != newsEpoch || activity.isDestroyed()) return;
                    news = result; newsFetching = false;
                    if (page == Page.HOME) render();
                    updateCountdown();
                });
            } catch (IOException | JSONException e) {
                activity.runOnUiThread(() -> {
                    if (epoch == newsEpoch && !activity.isDestroyed()) { newsFetching = false; message("Could not load project news."); }
                });
            }
        }, "nvf-launcher-news").start();
    }

    private TextView countdownDigitView() {
        TextView v = new TextView(activity);
        v.setText("00");
        v.setTextSize(12);
        v.setTextColor(gold);
        v.setTypeface(Typeface.MONOSPACE, Typeface.BOLD);
        v.setSingleLine(true);
        v.setShadowLayer(dp(3), 0f, 0f, (gold & 0x00FFFFFF) | 0x70000000);
        return v;
    }

    private LinearLayout countdownSegment(TextView value, String unit) {
        LinearLayout cell = row();
        cell.setGravity(Gravity.CENTER_VERTICAL);
        cell.setPadding(dp(4), dp(2), dp(4), dp(2));
        GradientDrawable box = new GradientDrawable();
        box.setColor(0xFF0C0906);
        box.setStroke(Math.max(1, dp(1)), (gold & 0x00FFFFFF) | 0x66000000);
        box.setCornerRadius(dp(4));
        cell.setBackground(box);
        cell.addView(value);
        TextView suffix = new TextView(activity);
        suffix.setText(unit);
        suffix.setTextSize(9);
        suffix.setTextColor(weak);
        suffix.setTypeface(Typeface.MONOSPACE, Typeface.BOLD);
        suffix.setSingleLine(true);
        LinearLayout.LayoutParams ulp = new LinearLayout.LayoutParams(-2, -2);
        ulp.leftMargin = dp(1.5f);
        cell.addView(suffix, ulp);
        return cell;
    }

    private TextView countdownColon() {
        TextView sep = new TextView(activity);
        sep.setText(":");
        sep.setTextSize(12);
        sep.setTextColor((gold & 0x00FFFFFF) | 0xAA000000);
        sep.setTypeface(Typeface.MONOSPACE, Typeface.BOLD);
        sep.setPadding(dp(2), 0, dp(2), 0);
        return sep;
    }

    private void updateCountdown() {
        root.removeCallbacks(tick);
        JSONObject next = news == null ? null : news.optJSONObject("next");
        if (next == null) { countdownBadge.setVisibility(View.GONE); return; }
        String version = next.optString("version", "");
        long at = next.optLong("at", 0L);
        long now = System.currentTimeMillis() / 1000L;
        CountdownFormat.Snapshot snap = CountdownFormat.of(version, at, now);
        if (snap == null) {
            countdownBadge.setVisibility(View.GONE);
            return;
        }
        countdownHeader.setText(snap.header);
        countdownDaysBox.setVisibility(snap.showDays ? View.VISIBLE : View.GONE);
        countdownDaysSep.setVisibility(snap.showDays ? View.VISIBLE : View.GONE);
        countdownDays.setText(snap.days);
        countdownHours.setText(snap.hours);
        countdownMinutes.setText(snap.minutes);
        countdownSeconds.setText(snap.seconds);
        countdownBadge.setVisibility(View.VISIBLE);
        if (resumed) root.postDelayed(tick, 1000);
    }

    // JNI calls this on the worker. Android supplies DNS, HTTPS and the device trust store.
    // Follows up to UpdatePolicy.MAX_REDIRECTS HTTPS redirects to pinned metadata/download hosts
    // (nv-forever.com, api.github.com, and GitHub Releases asset hosts for SHA256SUMS.txt 302s)
    // while preserving size/deadline caps.
    private static String getHttps(String address) {
        String currentUrl = address;
        long deadline = android.os.SystemClock.elapsedRealtime() + 15000;
        HttpsURLConnection connection = null;
        try {
            for (int hop = 0; hop <= UpdatePolicy.MAX_REDIRECTS; hop++) {
                if (android.os.SystemClock.elapsedRealtime() > deadline) return null;
                URL url = new URL(currentUrl);
                if (!"https".equals(url.getProtocol())) return null;
                connection = (HttpsURLConnection) url.openConnection();
                connection.setInstanceFollowRedirects(false);
                connection.setConnectTimeout(10000); connection.setReadTimeout(10000);
                connection.setRequestProperty("User-Agent", "NVForever-Android-Launcher");
                int code = connection.getResponseCode();
                if (code == 301 || code == 302 || code == 303 || code == 307 || code == 308) {
                    String location = connection.getHeaderField("Location");
                    connection.disconnect();
                    connection = null;
                    if (location == null || location.isEmpty()) return null;
                    String next = new URL(url, location).toString();
                    if (!UpdatePolicy.isAllowedMetadataRedirectUrl(next)) return null;
                    currentUrl = next;
                    continue;
                }
                if (code != 200) return null;
                try (InputStream in = connection.getInputStream(); ByteArrayOutputStream out = new ByteArrayOutputStream()) {
                    byte[] bytes = new byte[8192]; int n;
                    while ((n = in.read(bytes)) != -1) {
                        if (out.size() + n > 2 * 1024 * 1024 || android.os.SystemClock.elapsedRealtime() > deadline) return null;
                        out.write(bytes, 0, n);
                    }
                    return new String(out.toByteArray(), StandardCharsets.UTF_8);
                }
            }
            return null;
        } catch (IOException e) { return null; }
        finally { if (connection != null) connection.disconnect(); }
    }
    private void openLink(String url) {
        Uri uri = Uri.parse(url);
        if (!"https".equalsIgnoreCase(uri.getScheme()) || uri.getHost() == null || uri.getUserInfo() != null
                || url.chars().anyMatch(c -> c <= 32 || c == '\\')) return;
        try { activity.startActivity(new Intent(Intent.ACTION_VIEW, uri)); }
        catch (ActivityNotFoundException e) { message("No browser is available to open that link."); }
    }

    private void settingsPage() throws IOException, JSONException {
        JSONObject model = object("settings", "edits", edits);
        LinearLayout tabs = row();
        HorizontalScrollView tabBar = horizontal(tabs); add(content, tabBar);
        JSONArray available = model.getJSONArray("tabs");
        for (int i = 0; i < available.length(); i++) {
            JSONArray t = available.getJSONArray(i); String key = t.getString(0);
            boolean selected = key.equals(tab);
            Button choice = tap(t.getString(1), selected, true, () -> {
                if (!selected) { tab = key; scroll.scrollTo(0, 0); render(); }
            });
            addBar(tabs, choice, false);
            if (selected) tabBar.post(() -> tabBar.scrollTo(choice.getLeft(), 0));
        }
        LinearLayout changes = row(); add(content, changes);
        addBar(changes, tap("Save settings", true, editable() && edits.length() != 0, () -> act(() -> {
            saveSettings(); message("Settings saved for the next launch."); renderHere();
        })), true);
        addBar(changes, tap("Undo edits", false, edits.length() != 0, () -> {
            var keys = edits.keys(); while (keys.hasNext()) { keys.next(); keys.remove(); }
            renderHere();
        }), true);
        JSONArray rows = model.getJSONArray("rows");
        String heading = null; boolean any = false;
        for (int i = 0; i < rows.length(); i++) {
            JSONObject r = rows.getJSONObject(i);
            if (!tab.equals(r.getString("tab"))) continue;
            any = true;
            String group = r.isNull("heading") ? "" : r.getString("heading");
            if (!group.equals(heading) && !group.isEmpty()) add(content, label(group, 17, gold));
            heading = group;
            LinearLayout box = card(r.getString("label"));
            String note = r.getString("note"); if (!note.isEmpty()) add(box, label(note, 14, weak));
            String value = r.getString("value"), key = r.getString("key");
            boolean canEdit = editable() && r.isNull("reason") && !r.getBoolean("readOnly");
            if (!r.isNull("reason")) add(box, label(r.getString("reason"), 14, weak));
            JSONObject control = r.getJSONObject("control");
            String shown = value.isEmpty() ? "Game default" : value;
            switch (control.getString("type")) {
                case "switch" -> add(box, tap(value.isEmpty() ? shown : "1".equals(value) ? "On" : "Off", false, canEdit,
                        () -> edit(key, "1".equals(value) ? "0" : "1")));
                case "list" -> {
                    JSONArray list = control.getJSONArray("choices");
                    String[] values = new String[list.length()], labels = new String[list.length()]; int selected = -1;
                    for (int j = 0; j < list.length(); j++) {
                        JSONObject choice = list.getJSONObject(j); values[j] = choice.getString("name"); labels[j] = choice.getString("label");
                        if (values[j].equalsIgnoreCase(value)) { shown = labels[j]; selected = j; }
                    }
                    int at = selected;
                    add(box, tap(shown, false, canEdit && list.length() != 0, () -> new AlertDialog.Builder(activity)
                        .setTitle(r.optString("label")).setSingleChoiceItems(labels, at, (dialog, which) -> {
                            dialog.dismiss(); if (which != at) edit(key, values[which]);
                        }).setNegativeButton("Cancel", null).show()));
                }
                case "slider" -> {
                    double part = parse(value);
                    add(box, tap(value.isEmpty() ? shown : stepLabel(control, part), false, canEdit,
                        () -> slider(r.optString("label"), key, part, control)));
                }
                default -> add(box, tap(shown, false, canEdit, () -> {
                    EditText input = field(value, r.optString("label"));
                    new AlertDialog.Builder(activity).setTitle(r.optString("label")).setView(input).setNegativeButton("Cancel", null)
                        .setPositiveButton("Apply", (d, which) -> edit(key, input.getText().toString())).show();
                }));
            }
        }
        if (!any && "Mobile".equals(tab)) add(content, label("The first normal Play or Continue publishes this build’s Mobile settings. Touch practice and layout editing open only in the game.", 16, weak));
    }

    private void slider(String title, String key, double value, JSONObject control) {
        double step = Math.max(0.001, control.optDouble("step", 0.01));
        int first = (int) Math.ceil(control.optDouble("min", 0) / step - 0.0001);
        int last = (int) Math.round(1 / step);
        LinearLayout dialog = column(); space(dialog, 16, 12);
        TextView show = label("", 17, text); add(dialog, show);
        SeekBar slider = new SeekBar(activity); slider.setMinimumHeight(dp(48)); slider.setMax(Math.max(0, last - first));
        slider.setProgress((int) Math.round(value / step) - first); add(dialog, slider);
        show.setText(stepLabel(control, Math.min(1, (first + slider.getProgress()) * step)));
        slider.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener() {
            public void onProgressChanged(SeekBar bar, int index, boolean user) { show.setText(stepLabel(control, Math.min(1, (first + index) * step))); }
            public void onStartTrackingTouch(SeekBar bar) {}
            public void onStopTrackingTouch(SeekBar bar) {}
        });
        new AlertDialog.Builder(activity).setTitle(title).setView(dialog).setNegativeButton("Cancel", null)
            .setPositiveButton("Apply", (d, which) -> edit(key, String.format(Locale.ROOT, "%.3f", Math.min(1, (first + slider.getProgress()) * step)))).show();
    }
    private static double parse(String value) {
        try { double part = Double.parseDouble(value); return Double.isFinite(part) ? part : 0; }
        catch (NumberFormatException e) { return 0; }
    }
    private static String stepLabel(JSONObject control, double part) {
        JSONArray steps = control.optJSONArray("steps");
        String label = Math.round(part * 100) + "%"; double distance = Double.POSITIVE_INFINITY;
        if (steps != null) for (int i = 0; i < steps.length(); i++) {
            JSONObject step = steps.optJSONObject(i);
            double delta = Math.abs(part - step.optDouble("value"));
            if (delta < distance) { label = step.optString("label"); distance = delta; }
        }
        return label;
    }
    private void edit(String key, String value) { act(() -> { edits.put(key, value); renderHere(); }); }
    private void renderHere() { int y = scroll.getScrollY(); render(); scroll.post(() -> scroll.scrollTo(0, y)); }
    private EditText field(String value, String hint) {
        EditText v = new EditText(activity); v.setSingleLine(true); v.setText(value); v.setHint(hint); v.setMinimumHeight(dp(48)); return v;
    }

    private void modsPage() throws IOException, JSONException {
        if (!editable()) { add(content, label("Close the game or finish importing before changing mods.", 16, weak)); return; }
        JSONObject model = object("mods");
        JSONObject fields = model.getJSONObject("fields");
        LinearLayout start = card("New game start");
        EditText quest = field(fields.getString("quest"), "Start quest (game default)"); add(start, quest);
        EditText film = field(fields.getString("film"), "Intro film (game default)"); add(start, film);
        CheckBox noFilm = new CheckBox(activity); noFilm.setText("No intro film"); noFilm.setMinimumHeight(dp(48));
        noFilm.setChecked(fields.getBoolean("no_film")); film.setEnabled(!noFilm.isChecked()); add(start, noFilm);
        noFilm.setOnCheckedChangeListener((view, checked) -> film.setEnabled(!checked));
        if (!model.isNull("ttw")) {
            if (!model.getBoolean("ttw")) add(start, label("Tale of Two Wastelands is off in this load order.", 15, weak));
            add(start, tap("Use Tale of Two Wastelands start", false, true, () -> {
                JSONObject defaults = model.optJSONObject("ttwDefaults"); quest.setText(defaults.optString("quest"));
                film.setText(defaults.optString("film")); noFilm.setChecked(defaults.optBoolean("no_film"));
            }));
        }
        add(start, tap("Save start", true, true, () -> act(() -> {
            JSONObject next = new JSONObject(); next.put("quest", quest.getText().toString()); next.put("film", film.getText().toString());
            next.put("no_film", noFilm.isChecked()); send("save_start", "fields", next); message("New game start saved.");
        })));
        LinearLayout plugins = card("Plugin load order");
        JSONArray items = model.getJSONArray("entries");
        boolean haveGame = !state.isNull("folder");
        if (!haveGame) add(plugins, label("Import your game folder to manage its plugins.", 15, weak));
        for (int i = 0; i < items.length(); i++) {
            JSONObject item = items.getJSONObject(i); String name = item.getString("name");
            boolean base = "FalloutNV.esm".equalsIgnoreCase(name);
            CheckBox check = new CheckBox(activity); check.setText(name); check.setMinimumHeight(dp(48));
            check.setChecked(base || item.getBoolean("active")); check.setEnabled(haveGame && !base); add(plugins, check);
            check.setOnClickListener(v -> changePlugin(name, "toggle"));
            if (base) continue;
            LinearLayout move = row(); add(plugins, move);
            boolean up = i > 0 && !"FalloutNV.esm".equalsIgnoreCase(items.getJSONObject(i - 1).getString("name"));
            boolean down = i + 1 < items.length() && !"FalloutNV.esm".equalsIgnoreCase(items.getJSONObject(i + 1).getString("name"));
            addBar(move, tap("Move up", false, haveGame && up, () -> changePlugin(name, "up")), true);
            addBar(move, tap("Move down", false, haveGame && down, () -> changePlugin(name, "down")), true);
        }
    }
    private void changePlugin(String name, String change) { act(() -> { send("plugin", "name", name, "change", change); renderHere(); }); }
    private interface Action { void run() throws IOException, JSONException; }
    private void act(Action action) {
        try { action.run(); }
        catch (IOException | JSONException e) { message(e.getMessage() == null ? "Launcher operation failed." : e.getMessage()); }
    }
}
