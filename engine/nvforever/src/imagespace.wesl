// Image-space grade (imagespace.rs): saturation, tint, brightness and contrast on the
// display (sRGB) values of the tonemapped picture [hypothesis, see imagespace.rs], with the
// screen effects of image space modifiers: double vision and radial blur on the picture
// read, the fade colour over the graded result (docs/spec-notes/image-modifiers.md §5).

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

struct Grade {
    tint: vec4<f32>,
    fade: vec4<f32>,
    // Radial blur: strength, ramp up, start, ramp down.
    radial: vec4<f32>,
    // Radial blur down start, centre x, centre y; double vision strength.
    radial2: vec4<f32>,
    saturation: f32,
    contrast: f32,
    contrast_avg_lum: f32,
    brightness: f32,
    blur: f32,
    time: f32,
    sharp: f32,
    // VATS mode: swipe edge, band, wash, grade (imagespace.rs).
    vats: vec4<f32>,
    // VATS mode: the grade's colour, what a picture's lightness is multiplied by.
    vats_tint: vec4<f32>,
    // Depth of field: strength, distance, range, the target's distance (< 0: the screen
    // centre's); use target, flags, the camera's near plane (imagespace.rs).
    dof: vec4<f32>,
    dof_mode: vec4<f32>,
    // Motion blur: the exposure in frames (0: none), the sharp depth of the player's own
    // body, the longest smear (a part of the screen height); this frame's clip space to
    // the last one's (imagespace.rs).
    motion: vec4<f32>,
    reproject: mat4x4<f32>,
    // HDR output (hdr.rs): on (x = 1), the peak in paper whites, the knee, the highlights'
    // strength.
    hdr: vec4<f32>,
}

@group(0) @binding(0) var screen: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> grade: Grade;

// Radial blur: how far a point's samples reach towards the centre, as a part of its
// distance from the centre, per unit of strength [hypothesis: the wiki calls strength 2.5
// moderate and 5 strong].
const RADIAL_REACH: f32 = 0.02;
const RADIAL_TAPS: i32 = 12;

// VATS mode's look (docs/design/vats-mode.md §5) [design, after the owner's Dead Eye
// references]: luminance times this amber gives amber mid tones from mid grey; the creamy
// wash; the band's width behind the edge (vats_mode::BAND_WIDTH) and the edge's softness,
// as shares of the screen width; how much the corners darken.
const VATS_WASH: vec3<f32> = vec3<f32>(1.0, 0.95, 0.78);
const VATS_GRADE: f32 = 0.85;
const VATS_BAND: f32 = 0.22;
const VATS_SOFT: f32 = 0.02;
const VATS_VIGNETTE: f32 = 0.35;

// The alpha the Pip-Boy's screen writes (nvf_assets NifSurface::pipboy_screen): those pixels
// skip the scene's grade. The original's screen reads about the same indoors and at dusk
// [observed: playtest shots of the original, 2026-11-05, docs/spec-notes/pipboy.md §3], where
// this engine's grade would move it: clear weather at noon brightens by 1.3 [observed: this
// engine's log, `sky:`]. That it stays out at every hour is [hypothesis]. Screen effects (VATS,
// the fade) still cover it.
const SCREEN_MARK: f32 = 0.25;

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3(0.0031308));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

fn scene(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(screen, screen_sampler, clamp(uv, vec2(0.0), vec2(1.0)), 0.0).rgb;
}

// The picture at a point, doubled: two copies `half` apart to each side.
fn doubled(uv: vec2<f32>, half: f32) -> vec3<f32> {
    if half <= 0.0 {
        return scene(uv);
    }
    return 0.5 * (scene(uv + vec2(half, 0.0)) + scene(uv - vec2(half, 0.0)));
}

// How strongly a point is blurred: nothing inside `start` (a distance from the centre, 1
// at the far corner of a centred screen), rising at a rate set by ramp up (0 never, 1 at
// once), and falling again from `down start` at a rate set by ramp down.
fn radial_amount(uv: vec2<f32>, centre: vec2<f32>) -> f32 {
    let d = length((uv - centre) * 2.0) / 1.41421356;
    let up_rate = grade.radial.y;
    var up = 0.0;
    if up_rate >= 1.0 {
        up = step(grade.radial.z, d);
    } else if up_rate > 0.0 {
        up = clamp((d - grade.radial.z) * up_rate / (1.0 - up_rate), 0.0, 1.0);
    }
    let down_rate = grade.radial.w;
    var down = 1.0;
    if down_rate >= 1.0 {
        down = 1.0 - step(grade.radial2.x, d);
    } else if down_rate > 0.0 {
        down = 1.0 - clamp((d - grade.radial2.x) * down_rate / (1.0 - down_rate), 0.0, 1.0);
    }
    return grade.radial.x * up * down;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let src = textureSample(screen, screen_sampler, in.uv);
    var lin = src.rgb;
    let double_vision = grade.radial2.w;
    if double_vision > 0.0 || grade.radial.x > 0.0 {
        // The two copies swing between 0.2 and 1 of the strength, as a part of the screen
        // width [hypothesis: the wiki says the effect animates by itself].
        let half = 0.5 * double_vision * (0.6 + 0.4 * sin(grade.time * 1.5));
        let centre = grade.radial2.yz;
        let amount = radial_amount(in.uv, centre) * RADIAL_REACH;
        if amount > 0.0 {
            var acc = vec3(0.0);
            for (var i = 0; i < RADIAL_TAPS; i++) {
                let t = f32(i) / f32(RADIAL_TAPS - 1);
                acc += doubled(centre + (in.uv - centre) * (1.0 - amount * t), half);
            }
            lin = acc / f32(RADIAL_TAPS);
        } else {
            lin = doubled(in.uv, half);
        }
    }
    var c = to_srgb(clamp(lin, vec3(0.0), vec3(1.0)));
    let marked = abs(src.a - SCREEN_MARK) < 0.02;
    if !marked {
        let lum = dot(c, vec3(0.2125, 0.7154, 0.0721));
        c = mix(vec3(lum), c, grade.saturation);
        c = mix(c, lum * grade.tint.rgb, grade.tint.a);
        c = c * grade.brightness;
        c = mix(vec3(grade.contrast_avg_lum), c, grade.contrast);
        c = clamp(c, vec3(0.0), vec3(1.0));
    }
    // VATS mode: the amber grade left of the swipe's edge, the band riding the edge, the
    // wash over the whole screen.
    let vats = grade.vats;
    if grade.vats_tint.a > 0.0 {
        // The classic style: no colour over the picture, only less light, down to the
        // tint's fourth figure at full strength (imagespace.rs).
        c = c * mix(1.0, grade.vats_tint.a, clamp(vats.w, 0.0, 1.0));
    } else if vats.y > 0.0 || vats.z > 0.0 || vats.w > 0.0 {
        let x = in.uv.x;
        let behind = 1.0 - smoothstep(vats.x - VATS_SOFT, vats.x + VATS_SOFT, x);
        let l = dot(c, vec3(0.2125, 0.7154, 0.0721));
        let corner = 1.0 - VATS_VIGNETTE * smoothstep(0.4, 1.35, length((in.uv - vec2(0.5)) * 2.0));
        let amber = clamp(vec3(l) * grade.vats_tint.rgb, vec3(0.0), vec3(1.0)) * corner;
        c = mix(c, amber, clamp(vats.w * VATS_GRADE * behind, 0.0, 1.0));
        let band = vats.y * smoothstep(vats.x - VATS_BAND, vats.x, x) * behind;
        c = mix(c, VATS_WASH, clamp(band + vats.z, 0.0, 1.0));
    }
    // The fade colour covers the graded picture [observed: the opening is white while a
    // modifier holds the brightness at zero, image-modifiers.md §5].
    c = mix(c, grade.fade.rgb, clamp(grade.fade.a, 0.0, 1.0));
    var out = to_linear(clamp(c, vec3(0.0), vec3(1.0)));
    // HDR output (hdr.rs): above the knee (linear luminance), the graded picture's
    // highlights rise with the square of the way from the knee to white, SDR white to the
    // peak; one gain for all three channels keeps the hue [design]. Not the Pip-Boy's
    // screen, and less as a fade to a colour covers the picture.
    if grade.hdr.x > 0.5 && !marked {
        let l = dot(out, vec3(0.2126, 0.7152, 0.0722));
        let t = clamp((l - grade.hdr.z) / max(1.0 - grade.hdr.z, 1e-3), 0.0, 1.0);
        let gain = 1.0 + (grade.hdr.y - 1.0) * t * t * grade.hdr.w;
        out = out * mix(gain, 1.0, clamp(grade.fade.a, 0.0, 1.0));
    }
    return vec4(out, select(src.a, 1.0, marked));
}
