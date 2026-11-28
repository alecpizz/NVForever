//! Production PBR source preparation shared by LightsPlugin and CPU shader checks.

/// Bevy's attenuation: windowed inverse square [observed: bevy_pbr 0.20.1].
pub(crate) const BEVY_ATTENUATION: &str = "return getRangeFalloff(distanceSquare, inverseRangeSquared) * 1.0 / max(distanceSquare, 0.0001);";

/// The game's attenuation: 1 − (d / radius)², clamped [hypothesis: shape to calibrate
/// against in-game captures; the LIGH falloff exponent, 1 for most lights, is not used].
pub(crate) const GAME_ATTENUATION: &str = "return saturate(1.0 - distanceSquare * inverseRangeSquared);";

/// Physical windowed inverse-square attenuation (the Physical lighting row): Frostbite/Karis
/// $1/d^2$ falloff with finite bulb core radius $(0.30 R)$ and $(1 - (d/R)^4)_+^2$ edge
/// windowing, normalized so $d \approx 0.35 R$ matches room-scale luminaire calibration.
pub(crate) const PHYSICAL_ATTENUATION: &str = "let u = distanceSquare * inverseRangeSquared;\n    let w = saturate(1.0 - u * u);\n    return (w * w) * (0.22 / (u + 0.09));";

/// Desktop Filament perceptual roughness floor (the Physical lighting row) replacing mobile `0.089`.
const ROUGHNESS_FLOOR_EDIT: (&str, &str) = (
    "    let clampedPerceptualRoughness = clamp(perceptualRoughness, 0.089, 1.0);",
    "    let clampedPerceptualRoughness = clamp(perceptualRoughness, 0.045, 1.0);",
);

/// The Fresnel factor of direct lights' isotropic highlight in a world view stays at F0
/// in display-value lighting (`lights.rs`).
pub(crate) const FRESNEL_EDIT: (&str, &str) = (
    "    let F = fresnel(F0, LdotH);",
    "    // NVForever (nvforever lights.rs FRESNEL_EDIT): F = F0 in a world view.
    let F = select(fresnel(F0, LdotH), F0, view_bindings::lights.ambient_color.a < 0.0);",
);

/// Bevy's point and directional lights convert display-space light colour uniforms into linear
/// light in world views.
pub(crate) const SPECULAR_EDITS: [(&str, &str); 3] = [
    (
        "fn getDistanceAttenuation(",
        "// NVForever (nvforever lights.rs SPECULAR_EDITS): a light's colour uniform as linear light;
// in a world view it holds display values (scaled by pi and the light unit).
fn nvf_light_colour(c: vec3<f32>) -> vec3<f32> {
    if view_bindings::lights.ambient_color.a >= 0.0 {
        return c;
    }
    let k = view_bindings::view.exposure / PI;
    return pow(max(c * k, vec3(0.0)), vec3(2.2)) / k;
}

fn getDistanceAttenuation(",
    ),
    (
        "    return color_times_NdotL * (*light).color_inverse_square_range.rgb *
        rangeAttenuation * texture_sample;",
        "    return color_times_NdotL * nvf_light_colour((*light).color_inverse_square_range.rgb) *
        rangeAttenuation * texture_sample;",
    ),
    ("color *= (*light).color.rgb * texture_sample;", "color *= nvf_light_colour((*light).color.rgb) * texture_sample;"),
];

/// Exact IEC 61966-2-1 HDR-preserving sRGB decode for physical PBR clustered light
/// uniforms (the Physical lighting row); directional lights receive pure linear RGB directly.
const PHYSICAL_SPECULAR_EDITS: [(&str, &str); 2] = [
    (
        "fn getDistanceAttenuation(",
        "// NVForever (nvforever lights.rs PHYSICAL_SPECULAR_EDITS): the sRGB decode, kept above 1.
// nvf_display_to_linear is the display-value sum's own (DISPLAY_HELPERS), put here by
// `lighting_patched`; pbr_functions' physical helpers read nvf_srgb_to_linear_hdr from here.
NVF_DISPLAY_TO_LINEAR
fn nvf_srgb_to_linear_hdr(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3(0.0));
    let m = max(max(x.r, max(x.g, x.b)), 1.0);
    return nvf_display_to_linear(x / m) * m;
}

fn nvf_light_colour(c: vec3<f32>) -> vec3<f32> {
    if view_bindings::lights.ambient_color.a >= 0.0 {
        return c;
    }
    let k = view_bindings::view.exposure / PI;
    return nvf_srgb_to_linear_hdr(c * k) / k;
}

fn getDistanceAttenuation(",
    ),
    (
        "    return color_times_NdotL * (*light).color_inverse_square_range.rgb *
        rangeAttenuation * texture_sample;",
        "    return color_times_NdotL * (nvf_light_colour((*light).color_inverse_square_range.rgb) * 1.10) *
        rangeAttenuation * texture_sample;",
    ),
];

const OREN_NAYAR_FN: &str = "/// Single-scattering directional albedo factor G(mu) for OpenPBR EON (Portsmith JCGT 2025).
fn nvf_eon_G(mu: f32) -> f32 {
    let x = 1.0 - saturate(mu);
    return x * (0.0571085289 + x * (0.491881867 + x * (-0.332181442 + x * 0.0714429953)));
}

/// Directional dielectric specular albedo E_spec(mu) including GGX multiscatter.
fn nvf_dielectric_E(F0: vec3<f32>, perceptual_roughness: f32, mu: f32) -> vec3<f32> {
    let fab = F_AB(perceptual_roughness, max(mu, 0.0001));
    let Fss = F0 * fab.x + vec3(fab.y);
    let Ems = max(1.0 - (fab.x + fab.y), 0.0);
    let Favg = F0 + (vec3(1.0) - F0) * (1.0 / 21.0);
    let Fms = Fss * Favg / max(vec3(1.0) - Ems * Favg, vec3(1e-4)) * Ems;
    return min(Fss + Fms, vec3(0.999));
}

/// OpenPBR / Blender 4.3+ Energy-Conserving Oren-Nayar (EON) diffuse BRDF
/// coupled with reciprocal multi-scatter dielectric Fresnel transmission.
fn Fd_OrenNayar(
    input: ptr<function, LightingInput>,
    derived_input: ptr<function, DerivedLightingInput>,
    diffuse_color: vec3<f32>,
) -> vec3<f32> {
    let r = saturate((*input).layers[LAYER_BASE].perceptual_roughness);
    let NdotV = max((*input).layers[LAYER_BASE].NdotV, 0.0001);
    let NdotL = max((*derived_input).NdotL, 0.0001);
    let LdotH = (*derived_input).LdotH;

    // Fujii-improved qualitative Oren-Nayar single-scatter (trig-free via V.L = 2*(L.H)^2 - 1).
    let VdotL = clamp(2.0 * LdotH * LdotH - 1.0, -1.0, 1.0);
    let s = VdotL - NdotL * NdotV;
    let t = select(1.0, 1.0 / max(NdotL, NdotV), s > 0.0);
    let c1 = 0.28779341; // 1/2 - 2/(3*pi)
    let c2 = 0.07248821; // 2/3 - 28/(15*pi)
    let AF = 1.0 / (1.0 + c1 * r);
    let f_ss = diffuse_color * (AF * (1.0 + r * s * t) * (1.0 / PI));

    // Analytic multi-scattering interreflection bounce (Portsmith JCGT 2025 / OpenPBR).
    let E_i = AF * (1.0 + r * nvf_eon_G(NdotL));
    let E_o = AF * (1.0 + r * nvf_eon_G(NdotV));
    let E_avg = AF * (1.0 + r * c2);
    let rho = saturate(diffuse_color);
    let rho_ms = (rho * rho * E_avg) / max(vec3(1.0) - rho * (1.0 - E_avg), vec3(1e-5));
    let ms_factor = select(0.0, (1.0 - E_i) * (1.0 - E_o) / max(1.0 - E_avg, 1e-5), r > 1e-5);
    let f_eon = f_ss + rho_ms * (ms_factor * (1.0 / PI));

    // Reciprocal Kulla-Conty / OpenPBR dielectric specular-diffuse energy coupling.
    let F0_d = (*input).F0_dielectric;
    let E_spec_o = nvf_dielectric_E(F0_d, r, NdotV);
    let E_spec_i = nvf_dielectric_E(F0_d, r, NdotL);
    // 3-point Gauss-Legendre on mu in [0, 1] for 2*mu*E_spec(mu).
    let E_spec_avg = (5.0 / 18.0) * (2.0 * 0.11270167) * nvf_dielectric_E(F0_d, r, 0.11270167)
        + (8.0 / 18.0) * (2.0 * 0.5) * nvf_dielectric_E(F0_d, r, 0.5)
        + (5.0 / 18.0) * (2.0 * 0.88729833) * nvf_dielectric_E(F0_d, r, 0.88729833);
    let T_d = ((vec3(1.0) - E_spec_i) * (vec3(1.0) - E_spec_o)) / max(vec3(1.0) - E_spec_avg, vec3(1e-4));
    return f_eon * T_d;
}

";

const ENV_BRDF_ANCHOR: &str = "fn EnvBRDFApprox(";
const BURLEY_CALL: &str = "        diffuse = diffuse_color * Fd_Burley(input, &derived_input);";
const OREN_NAYAR_CALL: &str = "        // NVForever (NVF_PHYSICAL_PBR): Energy-Conserving Oren-Nayar (EON) with reciprocal dielectric coupling.
        diffuse = Fd_OrenNayar(input, &derived_input, diffuse_color);";

/// `pbr_lighting.wesl` with `Fd_OrenNayar` (EON + reciprocal dielectric coupling) added after
/// `F_AB` and both `Fd_Burley` call sites (`point_light` and `directional_light`) swapped to it.
pub(crate) fn oren_nayar_patched(source: &str) -> Option<String> {
    if source.matches(ENV_BRDF_ANCHOR).count() != 1 || source.matches(BURLEY_CALL).count() != 2 {
        return None;
    }
    let at = source.find(ENV_BRDF_ANCHOR)?;
    let mut out = String::with_capacity(source.len() + OREN_NAYAR_FN.len() + 256);
    out.push_str(&source[..at]);
    out.push_str(OREN_NAYAR_FN);
    out.push_str(&source[at..]);
    Some(out.replace(BURLEY_CALL, OREN_NAYAR_CALL))
}

/// The display-value sum's `nvf_display_to_linear` ([`DISPLAY_HELPERS`]), one copy of the
/// sRGB decode for both looks: Physical lighting's light colours and ambient read it from
/// `pbr_lighting` (prepass shaders included, so without its `PREPASS_FRAGMENT` gate).
fn display_to_linear_fn() -> Option<String> {
    let start = DISPLAY_HELPERS.find("fn nvf_display_to_linear(")?;
    let end = start + DISPLAY_HELPERS[start..].find("\n}\n")? + 3;
    Some(DISPLAY_HELPERS[start..end].to_string())
}

/// Patches `bevy_pbr/render/pbr_lighting.wesl` for either physical PBR (`physical = true`)
/// or display-value lighting (`physical = false`). Shared by `lights::tests` and
/// `nvf-shader-tests`.
#[allow(dead_code)]
pub(crate) fn lighting_patched(source: &str, physical: bool) -> Option<String> {
    let source = unix_lines(source);
    if !source.contains(BEVY_ATTENUATION) {
        return None;
    }
    let attenuation = if physical { PHYSICAL_ATTENUATION } else { GAME_ATTENUATION };
    let mut patched = source.replace(BEVY_ATTENUATION, attenuation);
    let edits: &[(&str, &str)] = if physical { &PHYSICAL_SPECULAR_EDITS } else { &SPECULAR_EDITS };
    if !edits.iter().all(|(from, _)| patched.matches(from).count() == 1) {
        return None;
    }
    for (from, to) in edits {
        patched = patched.replace(from, to);
    }
    if physical {
        patched = patched.replace("NVF_DISPLAY_TO_LINEAR\n", &display_to_linear_fn()?);
        if patched.matches(ROUGHNESS_FLOOR_EDIT.0).count() != 1 {
            return None;
        }
        patched = patched.replace(ROUGHNESS_FLOOR_EDIT.0, ROUGHNESS_FLOOR_EDIT.1);
        oren_nayar_patched(&patched)
    } else {
        if patched.matches(FRESNEL_EDIT.0).count() != 1 {
            return None;
        }
        Some(patched.replace(FRESNEL_EDIT.0, FRESNEL_EDIT.1))
    }
}

/// Diffuse and specular scales applied to shadow-casting spot lights (the player's
/// flashlight, `flashlight.rs`) on first-person shapes (`FIRST_PERSON_DEPTH`: arms, held
/// weapon, Pip-Boy) [design, #3121]. World geometry, third-person body/weapon (`BODY_LAYER`),
/// and non-shadow-casting world spot lamps (`LIGH` `SPOT`) keep 1.0.
pub const FIRST_PERSON_FLASHLIGHT_DIFFUSE: f32 = 0.15;
pub const FIRST_PERSON_FLASHLIGHT_SPECULAR: f32 = 0.10;

/// CPU mirror of the diffuse scale `nvf_spot_display` applies to a spot light.
#[cfg_attr(not(test), allow(dead_code))]
pub const fn spot_display_scale(first_person: bool, casts_shadows: bool) -> f32 {
    if first_person && casts_shadows { FIRST_PERSON_FLASHLIGHT_DIFFUSE } else { 1.0 }
}

/// CPU mirror of the specular scale `nvf_apply_pbr_lighting` applies to a spot light.
#[cfg_attr(not(test), allow(dead_code))]
pub const fn spot_specular_scale(first_person: bool, casts_shadows: bool) -> f32 {
    if first_person && casts_shadows { FIRST_PERSON_FLASHLIGHT_SPECULAR } else { 1.0 }
}

/// The display-value lighting sum (docs/spec-notes/world.md §8 "Light on surfaces in
/// display values") [hypothesis, fitted to the original's frames]: the game adds its
/// lights as display values (colour bytes / 255) — ambient + Σ sun and lights × fade ×
/// attenuation × N·L — and multiplies the texture by the sum, where Bevy adds linear
/// light. In a world view (marked by a negative ambient alpha, `lights::display_ambient`) the
/// patched `apply_pbr_lighting` leaves Bevy's diffuse terms out (diffuse colour 0 for the
/// lights and the ambient light, so only their specular parts stay), forms the sum from
/// the same lights' colours, which hold display values there (`lights::display_color`), and
/// adds, as light, the linear colour of the diffuse colour's display value × the sum, held
/// to 1: both ways by the sRGB curve, the one the texture was read through and the frame is
/// shown through, so the shown value is that product. (Until 2026-11-25 it added diffuse
/// colour × sum^2.2. A 2.2 power and the sRGB curve part ways in the dark: a floor of display
/// value 0.4 under an interior's ambient of 40 / 255 was shown as 8 / 255 where the product is
/// 16 / 255, world.md §8 "Dark values of the sum".) Views without the mark
/// (the minigame views) keep Bevy's sum. `nvf_apply_pbr_lighting` takes weights for the
/// ambient and the direct part of the sum (grass.wesl); `apply_pbr_lighting` is it with
/// weights 1. Each helper carries the condition Bevy puts on `apply_pbr_lighting`
/// (`@if(!PREPASS_FRAGMENT)`), whose line the first edit takes in.
const DISPLAY_HELPERS: &str = "// NVForever (nvforever lights.rs DISPLAY_HELPERS): the game's lighting sum in display values.
@if(!PREPASS_FRAGMENT)
fn nvf_display_lighting() -> bool {
    return view_bindings::lights.ambient_color.a < 0.0;
}

// The sRGB transfer, both ways (the texture's and the frame's own curve, IEC 61966-2-1):
// the display value of a linear colour, and back.
@if(!PREPASS_FRAGMENT)
fn nvf_linear_to_display(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3(0.0));
    return select(1.055 * pow(x, vec3(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3(0.0031308));
}

@if(!PREPASS_FRAGMENT)
fn nvf_display_to_linear(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3(0.0));
    return select(pow((x + 0.055) / 1.055, vec3(2.4)), x / 12.92, x <= vec3(0.04045));
}

// A clustered point or spot light's diffuse light on a surface at P facing N, as display
// values: colour × fade × attenuation × N.L (the uniform holds colour × fade × pi × light
// unit, nvforever lights.rs `lumens`).
@if(!PREPASS_FRAGMENT)
fn nvf_point_display(light_id: u32, P: vec3<f32>, N: vec3<f32>) -> vec3<f32> {
    let light = &view_bindings::clustered_lights.data[light_id];
    let to_light = (*light).position_radius.xyz - P;
    let d2 = dot(to_light, to_light);
    let attenuation = lighting::getDistanceAttenuation(d2, (*light).color_inverse_square_range.w);
    let n_dot_l = saturate(dot(N, to_light * inverseSqrt(max(d2, 0.0001))));
    return (*light).color_inverse_square_range.rgb * (attenuation * n_dot_l * view_bindings::view.exposure / 3.14159265);
}

// The same for a spot light, with Bevy's cone falloff (bevy_pbr::lighting::spot_light).
// First-person shapes (`FIRST_PERSON_DEPTH`: arms, weapon, Pip-Boy) sit inside the
// flashlight's inner core when aimed in; shadow-casting spot lights (flashlight.rs) are
// scaled down there so bright weapons and hands do not wash out to white (#3121).
@if(!PREPASS_FRAGMENT)
fn nvf_spot_display(light_id: u32, P: vec3<f32>, N: vec3<f32>) -> vec3<f32> {
    let light = &view_bindings::clustered_lights.data[light_id];
    var spot_dir = vec3<f32>((*light).light_custom_data.x, 0.0, (*light).light_custom_data.y);
    spot_dir.y = sqrt(max(0.0, 1.0 - spot_dir.x * spot_dir.x - spot_dir.z * spot_dir.z));
    if ((*light).flags & mesh_view_types::POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE) != 0u {
        spot_dir.y = -spot_dir.y;
    }
    let cd = dot(-spot_dir, normalize((*light).position_radius.xyz - P));
    let cone = saturate(cd * (*light).light_custom_data.z + (*light).light_custom_data.w);
    @if(!FIRST_PERSON_DEPTH)
    let nvf_fp_scale = 1.0;
    @if(FIRST_PERSON_DEPTH)
    let nvf_fp_scale = select(1.0, 0.15, ((*light).flags & mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u);
    return nvf_point_display(light_id, P, N) * (cone * cone * nvf_fp_scale);
}

// What is left of Bevy's ambient light in a world view, its highlight, takes the ambient
// colour as linear light: the factor from the display value to it (a^2.2 / a).
@if(!PREPASS_FRAGMENT)
fn nvf_ambient_ratio() -> vec3<f32> {
    return pow(max(view_bindings::lights.ambient_color.rgb * view_bindings::view.exposure, vec3(1e-6)), vec3(1.2));
}

// A directional light's diffuse light on a surface facing N, as display values.
@if(!PREPASS_FRAGMENT)
fn nvf_directional_display(i: u32, N: vec3<f32>) -> vec3<f32> {
    let light = &view_bindings::lights.directional_lights[i];
    return (*light).color.rgb * (saturate(dot(N, (*light).direction_to_light.xyz)) * view_bindings::view.exposure / 3.14159265);
}

@if(!PREPASS_FRAGMENT)
fn apply_pbr_lighting(
    in: pbr_types::PbrInput,
) -> vec4<f32> {
    return nvf_apply_pbr_lighting(in, vec2(1.0));
}

@if(!PREPASS_FRAGMENT)
fn nvf_apply_pbr_lighting(
    in: pbr_types::PbrInput,
    nvf_weights: vec2<f32>,
) -> vec4<f32> {";

/// Replacements in `apply_pbr_lighting` [observed: bevy_pbr 0.20.1 `pbr_functions.wesl`],
/// in order; the light loops' line occurs three times (point, spot, directional). A point
/// or spot light's `shadow` may be under 0 (a held lamp's weight past 1, bevy_pbr
/// shadows.wesl `nvf_weigh_shadow`): the display sum takes it as it is, Bevy's own sum
/// (the highlights, in a world view) `max(shadow, 0.0)`.
const DISPLAY_EDITS: [(&str, &str); 7] = [
    ("@if(!PREPASS_FRAGMENT)\nfn apply_pbr_lighting(\n    in: pbr_types::PbrInput,\n) -> vec4<f32> {", DISPLAY_HELPERS),
    (
        "    lighting_input.diffuse_color = diffuse_color;",
        "    let nvf_display = nvf_display_lighting();
    var nvf_direct = vec3(0.0);
    lighting_input.diffuse_color = select(diffuse_color, vec3(0.0), nvf_display);",
    ),
    (
        "        direct_light += light_contrib * shadow;",
        "        direct_light += light_contrib * max(shadow, 0.0);
        if nvf_display { nvf_direct += nvf_point_display(light_id, in.world_position.xyz, in.N) * shadow; }",
    ),
    (
        "        direct_light += light_contrib * shadow;",
        "        @if(!FIRST_PERSON_DEPTH)
        direct_light += light_contrib * max(shadow, 0.0);
        @if(FIRST_PERSON_DEPTH)
        direct_light += light_contrib * (max(shadow, 0.0) * select(1.0, 0.10, (view_bindings::clustered_lights.data[light_id].flags & mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u));
        if nvf_display { nvf_direct += nvf_spot_display(light_id, in.world_position.xyz, in.N) * shadow; }",
    ),
    (
        "        direct_light += light_contrib * shadow;",
        "        direct_light += light_contrib * shadow;
        if nvf_display { nvf_direct += nvf_directional_display(i, in.N) * shadow; }",
    ),
    (
        "        indirect_light += ambient::ambient_light(in.world_position, in.N, in.V, NdotV, diffuse_color, F0, perceptual_roughness, diffuse_occlusion);",
        "        indirect_light += ambient::ambient_light(in.world_position, in.N, in.V, NdotV, select(diffuse_color, vec3(0.0), nvf_display), F0, perceptual_roughness, diffuse_occlusion) * select(vec3(1.0), nvf_ambient_ratio(), nvf_display);",
    ),
    (
        "    // Total light\n",
        "    if nvf_display {
        let nvf_ambient = view_bindings::lights.ambient_color.rgb * view_bindings::view.exposure * diffuse_occlusion;
        let nvf_sum = max(nvf_weights.x * nvf_ambient + nvf_weights.y * nvf_direct, vec3(0.0));
        // Held to 1 per channel, as the original's lit colour is (lights.rs GAME_CLIP).
        direct_light += nvf_display_to_linear(min(nvf_linear_to_display(diffuse_color) * nvf_sum, vec3(1.0))) / view_bindings::view.exposure;
    }

    // Total light\n",
    ),
];

const PHYSICAL_HELPERS: &str = "// NVForever (nvforever lights.rs PHYSICAL_HELPERS): linear EON + Cook-Torrance PBR.
@if(!PREPASS_FRAGMENT)
fn nvf_display_lighting() -> bool {
    return view_bindings::lights.ambient_color.a < 0.0;
}

// What is left of Bevy's ambient light in a fallback view without an environment cubemap,
// taking the ambient colour as exact IEC sRGB linear light.
@if(!PREPASS_FRAGMENT)
fn nvf_ambient_ratio() -> vec3<f32> {
    let scaled = max(view_bindings::lights.ambient_color.rgb * view_bindings::view.exposure, vec3(1e-6));
    return lighting::nvf_srgb_to_linear_hdr(scaled) / scaled;
}

// Lagarde & de Rousiers (2014) Frostbite roughness-dependent specular AO.
@if(!PREPASS_FRAGMENT)
fn nvf_specular_ao(NdotV: f32, ao: f32, roughness: f32) -> f32 {
    return saturate(pow(max(NdotV + ao, 1e-4), exp2(-16.0 * roughness - 1.0)) - 1.0 + ao);
}

@if(!PREPASS_FRAGMENT)
fn apply_pbr_lighting(
    in: pbr_types::PbrInput,
) -> vec4<f32> {
    return nvf_apply_pbr_lighting(in, vec2(1.0));
}

@if(!PREPASS_FRAGMENT)
fn nvf_apply_pbr_lighting(
    in: pbr_types::PbrInput,
    nvf_weights: vec2<f32>,
) -> vec4<f32> {";

/// Replacements in `apply_pbr_lighting` for Physical lighting: keeps Bevy's linear direct
/// diffuse and specular loops active, routes world indirect lighting through procedural sky
/// `ENVIRONMENT_MAP` IBL (without double-counting flat `ambient::ambient_light`), and keeps a
/// fallback linear ambient path when `ENVIRONMENT_MAP` is absent.
const PHYSICAL_EDITS: [(&str, &str); 8] = [
    ("@if(!PREPASS_FRAGMENT)\nfn apply_pbr_lighting(\n    in: pbr_types::PbrInput,\n) -> vec4<f32> {", PHYSICAL_HELPERS),
    (
        "    lighting_input.diffuse_color = diffuse_color;",
        "    let nvf_display = nvf_display_lighting();
    lighting_input.diffuse_color = diffuse_color;",
    ),
    (
        "        direct_light += light_contrib * shadow;",
        "        direct_light += light_contrib * shadow; // nvf_physical_point",
    ),
    (
        "        direct_light += light_contrib * shadow;",
        "        direct_light += light_contrib * shadow; // nvf_physical_spot",
    ),
    (
        "    indirect_light += environment_light.diffuse * diffuse_occlusion;
    if (!use_ssr) {
        indirect_light += environment_light.specular * specular_occlusion;
    }",
        "    let nvf_ibl_diff_ao = diffuse_occlusion; // nvf_physical_ibl_diff_ao
    let nvf_ibl_spec_ao = specular_occlusion; // nvf_physical_ibl_spec_ao
    indirect_light += environment_light.diffuse * nvf_ibl_diff_ao;
    if (!use_ssr) {
        indirect_light += environment_light.specular * nvf_ibl_spec_ao;
    }",
    ),
    (
        "    if (enable_ambient) {
        indirect_light += ambient::ambient_light(in.world_position, in.N, in.V, NdotV, diffuse_color, F0, perceptual_roughness, diffuse_occlusion);
    }",
        "@if(ENVIRONMENT_MAP)
    if (enable_ambient && !nvf_display) {
        indirect_light += ambient::ambient_light(in.world_position, in.N, in.V, NdotV, diffuse_color, F0, perceptual_roughness, diffuse_occlusion);
    }
@if(!ENVIRONMENT_MAP)
    if (enable_ambient) {
        indirect_light += ambient::ambient_light(in.world_position, in.N, in.V, NdotV, select(diffuse_color, vec3(0.0), nvf_display), F0, perceptual_roughness, diffuse_occlusion) * select(vec3(1.0), nvf_ambient_ratio(), nvf_display);
    }",
    ),
    (
        "    // Total light\n",
        "    if nvf_display {
        let nvf_w = pow(max(nvf_weights, vec2(0.0)), vec2(2.2));
        direct_light *= nvf_w.y;
        @if(ENVIRONMENT_MAP) {
            indirect_light *= nvf_w.x;
            if (view_bindings::light_probes.view_cubemap_index < 0) {
                let nvf_env_fallback_lin = lighting::nvf_srgb_to_linear_hdr(view_bindings::lights.ambient_color.rgb * view_bindings::view.exposure) * diffuse_occlusion;
                indirect_light += diffuse_color * nvf_env_fallback_lin * nvf_w.x / max(view_bindings::view.exposure, 1e-6);
            }
        }
        @if(!ENVIRONMENT_MAP) {
            let nvf_ambient_lin = lighting::nvf_srgb_to_linear_hdr(view_bindings::lights.ambient_color.rgb * view_bindings::view.exposure) * diffuse_occlusion; // nvf_physical_fallback_ambient
            indirect_light = indirect_light * nvf_w.x + diffuse_color * nvf_ambient_lin * nvf_w.x / max(view_bindings::view.exposure, 1e-6);
        }
    }

    // Total light\n",
    ),
    (
        "    if fog_params.mode == mesh_view_types::FOG_MODE_LINEAR {",
        "    if fog_params.bi.x > 0.5 && fog_params.bi.z <= 0.0 && fog_params.directional_light_color.a <= 0.0 && distance > 1e-4 {
        let nvf_ray_dir = view_to_world / distance;
        var nvf_sun_cos = 0.0;
        var nvf_sun_elev = 0.25;
        if view_bindings::lights.n_directional_lights > 0u {
            let nvf_sun_dir = view_bindings::lights.directional_lights[0].direction_to_light.xyz;
            nvf_sun_cos = dot(nvf_ray_dir, nvf_sun_dir);
            nvf_sun_elev = nvf_sun_dir.y;
        }
        scattering = vec3<f32>(nvf_sun_cos, nvf_sun_elev, nvf_ray_dir.y);
    }
    if fog_params.mode == mesh_view_types::FOG_MODE_LINEAR {",
    ),
];

fn apply_sequential(source: &str, edits: &[(&str, &str)], extra_cap: usize) -> Option<String> {
    let mut out = String::with_capacity(source.len() + extra_cap);
    let mut rest = source;
    for &(from, to) in edits {
        let at = rest.find(from)?;
        out.push_str(&rest[..at]);
        out.push_str(to);
        rest = &rest[at + from.len()..];
    }
    out.push_str(rest);
    Some(out)
}

/// `source` with [`DISPLAY_EDITS`] made, each to the first occurrence after the last
/// edit; `None` when one is missing (Bevy changed).
pub(crate) fn display_patched(source: &str) -> Option<String> {
    apply_sequential(source, &DISPLAY_EDITS, 4096)
}

/// `source` with [`PHYSICAL_EDITS`] made, each to the first occurrence after the last edit;
/// `None` when one is missing (Bevy changed).
pub(crate) fn physical_patched(source: &str) -> Option<String> {
    apply_sequential(source, &PHYSICAL_EDITS, 2560)
}

/// Bevy's shader text with Windows line ends made `\n`: the patches' anchors are written
/// with `\n`. A checkout with git's `core.autocrlf` gave the bevy_pbr vendored until #939
/// `\r\n` (issue #946: no patch matched, so grass lost `nvf_apply_pbr_lighting`); Bevy
/// now comes from crates.io as published, and this stays as a guard for any patched copy.
pub fn unix_lines(source: &str) -> String {
    source.replace("\r\n", "\n")
}
