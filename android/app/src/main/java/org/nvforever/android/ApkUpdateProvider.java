package org.nvforever.android;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import java.io.File;
import java.io.FileNotFoundException;
import java.io.IOException;

/** Read-only ContentProvider exposing downloaded update APKs to the system package installer. */
public final class ApkUpdateProvider extends ContentProvider {
    private static final String APK_MIME = "application/vnd.android.package-archive";

    static File updatesDir(Context context) {
        return new File(context.getCacheDir(), "updates");
    }

    static Uri uriFor(Context context, String fileName) {
        if (!UpdatePolicy.safeApkName(fileName)) throw new IllegalArgumentException("Invalid APK name");
        return new Uri.Builder()
            .scheme("content")
            .authority(context.getPackageName() + ".updates")
            .appendPath(fileName)
            .build();
    }

    private File resolveApk(Uri uri) throws FileNotFoundException {
        Context context = getContext();
        if (context == null || uri == null) throw new FileNotFoundException("Provider unavailable");
        String name = uri.getLastPathSegment();
        if (!UpdatePolicy.safeApkName(name) || uri.getPathSegments().size() != 1) {
            throw new SecurityException("Invalid update URI");
        }
        try {
            File dir = updatesDir(context).getCanonicalFile();
            File target = new File(dir, name).getCanonicalFile();
            if (!dir.equals(target.getParentFile()) || !target.isFile()) {
                throw new FileNotFoundException("Update APK not found");
            }
            return target;
        } catch (IOException e) {
            throw new FileNotFoundException(e.getMessage());
        }
    }

    @Override public boolean onCreate() { return true; }

    @Override public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        if (!"r".equals(mode)) throw new SecurityException("Update provider is read-only");
        return ParcelFileDescriptor.open(resolveApk(uri), ParcelFileDescriptor.MODE_READ_ONLY);
    }

    @Override public String getType(Uri uri) {
        return APK_MIME;
    }

    @Override public Cursor query(Uri uri, String[] projection, String selection, String[] selectionArgs, String sortOrder) {
        try {
            File file = resolveApk(uri);
            String[] cols = projection == null ? new String[]{OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE} : projection;
            MatrixCursor cursor = new MatrixCursor(cols, 1);
            Object[] row = new Object[cols.length];
            for (int i = 0; i < cols.length; i++) {
                if (OpenableColumns.DISPLAY_NAME.equals(cols[i])) row[i] = file.getName();
                else if (OpenableColumns.SIZE.equals(cols[i])) row[i] = file.length();
            }
            cursor.addRow(row);
            return cursor;
        } catch (FileNotFoundException e) {
            return new MatrixCursor(new String[0], 0);
        }
    }

    @Override public Uri insert(Uri uri, ContentValues values) { throw new UnsupportedOperationException(); }
    @Override public int delete(Uri uri, String selection, String[] selectionArgs) { throw new UnsupportedOperationException(); }
    @Override public int update(Uri uri, ContentValues values, String selection, String[] selectionArgs) { throw new UnsupportedOperationException(); }
}
