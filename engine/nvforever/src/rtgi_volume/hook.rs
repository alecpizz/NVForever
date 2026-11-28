//! The bounce light's text hook in bevy_pbr's `pbr_functions` (rtgi_volume.rs module notes).
//! Plain text and the standard library only, as ssao_lighting/hook.rs.

/// The line of bevy_pbr's imports the hook's own goes after.
pub const IMPORT_LINE: &str = "import bevy_render::maths::{E, powsafe};\n";

/// The same with the import after it.
pub const IMPORT_LINE_HOOKED: &str = "import bevy_render::maths::{E, powsafe};
// NVForever RTGI volume (nvforever rtgi_volume.rs).
@if(NVF_RTGI_VOLUME) import nvforever::rtgi_volume::nvf_rtgi_light;
";

/// The diffuse light of the display-value sum (lights/display_shader.rs `DISPLAY_EDITS`).
pub const DIFFUSE_LINE: &str = "        direct_light += nvf_display_to_linear(min(nvf_linear_to_display(diffuse_color) * nvf_sum, vec3(1.0))) / view_bindings::view.exposure;\n";

/// The same kept without the def, and its twin with the bounce light under it. The bounce
/// light's function works in the light the 2.2 power gives (`nvf_rtgi_light`, in the place of
/// S^2.2); its result is taken back to a display sum by the 1/2.2 power and then shown as the
/// sum is, by the sRGB curve (lights/display_shader.rs): with the volume's change at 0 that
/// is the sum itself, so the line is the one it replaces. The occlusion
/// is the ambient's own: `diffuse_occlusion`, times the ambient occlusion's factor per colour
/// channel where its hook runs (ssao_lighting/hook.rs `AMBIENT_LINE_HOOKED`; its one read of
/// the texels is used again).
pub const DIFFUSE_LINE_HOOKED: &str = "        @if(!NVF_RTGI_VOLUME)
        direct_light += nvf_display_to_linear(min(nvf_linear_to_display(diffuse_color) * nvf_sum, vec3(1.0))) / view_bindings::view.exposure;
        // NVForever RTGI volume (nvforever rtgi_volume.rs): the light with the volume's bounce.
        @if(NVF_RTGI_VOLUME && !NVF_AMBIENT_OCCLUSION)
        let nvf_rtgi_occlusion = diffuse_occlusion;
        @if(NVF_RTGI_VOLUME && NVF_AMBIENT_OCCLUSION)
        let nvf_rtgi_occlusion = diffuse_occlusion * nvf_ao_multibounce(nvf_ao_ambient, diffuse_color);
        @if(NVF_RTGI_VOLUME)
        direct_light += nvf_display_to_linear(min(nvf_linear_to_display(diffuse_color) * pow(max(nvf_rtgi_light(nvf_weights.x * nvf_ambient, nvf_sum, nvf_rtgi_occlusion, in.world_position.xyz, in.N, in.V), vec3(0.0)), vec3(1.0 / 2.2)), vec3(1.0))) / view_bindings::view.exposure;
";

/// What must be in the source already: the ambient occlusion's hook, whose names the hooked
/// line uses.
const BEFORE: &str = "@if(NVF_AMBIENT_OCCLUSION) import ";

/// The hook, (line, what stands in its place) in the file's order [documented:
/// engine/vendor/bevy_pbr/src/render/pbr_functions.wesl (bevy_pbr 0.20.1) with lights.rs
/// `DISPLAY_EDITS`, sun_visibility.rs `HOOK_EDITS` and ssao_lighting/hook.rs `HOOK_EDITS`
/// made]. Every statement it adds is under `@if(NVF_RTGI_VOLUME)`, and the line it replaces is
/// kept word for word under `@if(!NVF_RTGI_VOLUME)`.
pub const HOOK_EDITS: [(&str, &str); 2] = [(IMPORT_LINE, IMPORT_LINE_HOOKED), (DIFFUSE_LINE, DIFFUSE_LINE_HOOKED)];

/// `source` (lights.rs's patches up to the ambient occlusion's made) with [`HOOK_EDITS`];
/// `None` when the ambient occlusion's hook or a line is missing, or when a line is there
/// twice: the caller keeps `source`, and no view may get a volume.
pub fn hook_patched(source: &str) -> Option<String> {
    if !source.contains(BEFORE) || HOOK_EDITS.iter().any(|(line, _)| source.matches(line).count() != 1) {
        return None;
    }
    Some(HOOK_EDITS.iter().fold(source.to_string(), |text, (line, hooked)| text.replacen(line, hooked, 1)))
}
