// The tonemapper's input (tone_input.rs; nvforever lights.rs `ToneCurve`): the one place the
// sky, the volumetric clouds and the hot light sources turn a display value (a colour the
// game grades directly, as the weather's sky colours are) into what the main view's
// tonemapper must be given to show it. A choice of NVForever's own, not the original game's.
//
// The curve's code, as the uniforms carry it (lights.rs `ToneCurve::code`): 1 when the view
// shows display values clipped (lights.rs GAME_CLIP / GAME_HDR: the value is the input), 0
// under Tony McMapface (the display-value look's NVF_TONEMAP=tony, and Physical lighting).
// Light that is already linear (the physical sky dome, Physical lighting's surfaces) is not
// passed through here. A choice of curves (#4690) adds codes here and in `ToneCurve`.
//
// TONY_IN / TONY_OUT sample Tony McMapface's response to grey (input linear, output linear),
// measured from renders of a flat grey sky without the grade [observed: Bevy 0.19.1 Tony
// McMapface]; the inverse is piecewise linear between the samples (Tony McMapface also
// shifts saturated colours, which this ignores). tone_input.rs holds the same numbers.

const TONE_CLIPPED: f32 = 1.0;

const TONY_N: u32 = 24u;
var<private> TONY_IN: array<f32, 24> = array<f32, 24>(0.0, 0.005, 0.01, 0.02, 0.04, 0.07, 0.1, 0.15, 0.2, 0.3, 0.4, 0.5, 0.6, 0.8, 1.0, 1.3, 1.7, 2.2, 3.0, 5.0, 8.0, 16.0, 40.0, 100.0);
var<private> TONY_OUT: array<f32, 24> = array<f32, 24>(0.0, 0.0056, 0.0113, 0.0222, 0.0437, 0.0723, 0.0999, 0.1470, 0.1812, 0.2542, 0.3140, 0.3663, 0.4125, 0.4910, 0.5457, 0.6172, 0.6867, 0.7454, 0.8070, 0.8796, 0.9216, 0.9473, 0.9771, 0.9875);

// The linear input Tony McMapface maps to the linear output `y`; the last input above the
// table.
fn tony_inverse(y: f32) -> f32 {
    for (var i = 1u; i < TONY_N; i++) {
        if y <= TONY_OUT[i] {
            let t = (y - TONY_OUT[i - 1u]) / max(TONY_OUT[i] - TONY_OUT[i - 1u], 1e-6);
            return mix(TONY_IN[i - 1u], TONY_IN[i], t);
        }
    }
    return TONY_IN[TONY_N - 1u];
}

// Per channel.
fn tony_inverse_each(rgb: vec3<f32>) -> vec3<f32> {
    return vec3(tony_inverse(rgb.r), tony_inverse(rgb.g), tony_inverse(rgb.b));
}

// Chromaticity-preserving: all channels scaled by the brightest one's inverse ratio, so a
// saturated bright colour (sun glare, sunset clouds) never has one channel reach the
// table's top alone and shift towards red.
fn tony_inverse_rgb(rgb: vec3<f32>) -> vec3<f32> {
    let m = max(max(rgb.r, rgb.g), rgb.b);
    if m <= 1e-6 {
        return vec3(0.0);
    }
    return rgb * (tony_inverse(m) / m);
}

// A display value as the input of the view's tonemapper `curve` (the code above),
// chromaticity-preserving.
fn to_tonemap_input(rgb: vec3<f32>, curve: f32) -> vec3<f32> {
    if curve > TONE_CLIPPED - 0.5 {
        return rgb;
    }
    return tony_inverse_rgb(rgb);
}

// The same, per channel (the volumetric clouds' composite, as it was written).
fn to_tonemap_input_each(rgb: vec3<f32>, curve: f32) -> vec3<f32> {
    if curve > TONE_CLIPPED - 0.5 {
        return rgb;
    }
    return tony_inverse_each(rgb);
}
