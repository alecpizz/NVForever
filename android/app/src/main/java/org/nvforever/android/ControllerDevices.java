package org.nvforever.android;

import android.hardware.input.InputManager;
import android.os.Handler;
import android.os.Looper;
import android.view.InputDevice;
import android.view.MotionEvent;
import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/** Metadata/removal only. Native buttons and axes stay with winit's one input reader. */
final class ControllerDevices implements InputManager.InputDeviceListener {
    private final InputManager manager;
    private boolean changed = true;
    private boolean closed;
    private long resetGeneration;
    private static final int[] AXES = {
        MotionEvent.AXIS_X, MotionEvent.AXIS_Y, MotionEvent.AXIS_Z, MotionEvent.AXIS_RX,
        MotionEvent.AXIS_RY, MotionEvent.AXIS_RZ, MotionEvent.AXIS_HAT_X, MotionEvent.AXIS_HAT_Y,
        MotionEvent.AXIS_LTRIGGER, MotionEvent.AXIS_RTRIGGER, MotionEvent.AXIS_GAS, MotionEvent.AXIS_BRAKE
    };
    ControllerDevices(InputManager manager) {
        this.manager = manager;
        manager.registerInputDeviceListener(this, new Handler(Looper.getMainLooper()));
    }
    @Override public synchronized void onInputDeviceAdded(int id) { changed = true; }
    @Override public synchronized void onInputDeviceChanged(int id) { changed = true; }
    @Override public synchronized void onInputDeviceRemoved(int id) { changed = true; }
    synchronized void cancelHeldInput() { resetGeneration++; changed = true; }
    synchronized void close() { closed = true; cancelHeldInput(); manager.unregisterInputDeviceListener(this); }
    synchronized String snapshot() {
        if (!changed) return "";
        changed = false;
        JSONArray devices = new JSONArray();
        try {
            for (int id : closed ? new int[0] : manager.getInputDeviceIds()) {
                InputDevice device = manager.getInputDevice(id);
                if (device == null || !(device.supportsSource(InputDevice.SOURCE_GAMEPAD)
                        || device.supportsSource(InputDevice.SOURCE_JOYSTICK))) continue;
                JSONObject item = new JSONObject();
                item.put("id", id); item.put("name", device.getName());
                item.put("vendor", device.getVendorId()); item.put("product", device.getProductId());
                item.put("sources", device.getSources());
                JSONArray axes = new JSONArray();
                for (int axis : AXES) {
                    InputDevice.MotionRange range = device.getMotionRange(axis, InputDevice.SOURCE_JOYSTICK);
                    if (range == null) continue;
                    JSONObject data = new JSONObject();
                    data.put("axis", axis); data.put("min", range.getMin());
                    data.put("max", range.getMax()); data.put("flat", range.getFlat()); axes.put(data);
                }
                item.put("axes", axes); devices.put(item);
            }
        } catch (JSONException e) {
            // Retry metadata next frame; never turn a serialization error into a disconnect.
            changed = true;
            return "";
        }
        try {
            return new JSONObject().put("reset", resetGeneration).put("devices", devices).toString();
        } catch (JSONException e) { changed = true; return ""; }
    }
}
