//! The tonemapper's input (tone_input.wesl): the shared table of Tony McMapface's response to
//! grey and its inverse, for the CPU mirrors of the shaders that turn display values into the
//! main view's tonemapper input (nvforever reflection_sky.rs). The view's curve itself is
//! nvforever lights.rs `ToneCurve`.

/// Tony McMapface's response to grey (input linear, output linear), the same numbers as
/// tone_input.wesl's `TONY_IN` / `TONY_OUT` [observed there: Bevy 0.19.1 renders of a flat
/// grey sky].
pub const TONY_IN: [f32; 24] = [0.0, 0.005, 0.01, 0.02, 0.04, 0.07, 0.1, 0.15, 0.2, 0.3, 0.4, 0.5, 0.6, 0.8, 1.0, 1.3, 1.7, 2.2, 3.0, 5.0, 8.0, 16.0, 40.0, 100.0];
pub const TONY_OUT: [f32; 24] = [0.0, 0.0056, 0.0113, 0.0222, 0.0437, 0.0723, 0.0999, 0.1470, 0.1812, 0.2542, 0.3140, 0.3663, 0.4125, 0.4910, 0.5457, 0.6172, 0.6867, 0.7454, 0.8070, 0.8796, 0.9216, 0.9473, 0.9771, 0.9875];

/// tone_input.wesl's `tony_inverse`: the linear input the table maps to `y`, piecewise
/// linear; the last input above the table.
pub fn tony_inverse(y: f32) -> f32 {
    for i in 1..TONY_IN.len() {
        if y <= TONY_OUT[i] {
            let t = (y - TONY_OUT[i - 1]) / (TONY_OUT[i] - TONY_OUT[i - 1]).max(1e-6);
            return TONY_IN[i - 1] + (TONY_IN[i] - TONY_IN[i - 1]) * t;
        }
    }
    TONY_IN[TONY_IN.len() - 1]
}

/// The table forward (the tone map `tony_inverse` undoes), piecewise linear.
pub fn tony_forward(x: f32) -> f32 {
    for i in 1..TONY_IN.len() {
        if x <= TONY_IN[i] {
            let t = (x - TONY_IN[i - 1]) / (TONY_IN[i] - TONY_IN[i - 1]);
            return TONY_OUT[i - 1] + (TONY_OUT[i] - TONY_OUT[i - 1]) * t;
        }
    }
    TONY_OUT[TONY_OUT.len() - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wesl_array(source: &str, name: &str) -> Vec<f32> {
        let start = source.find(&format!("var<private> {name}: array<f32, 24> = array<f32, 24>(")).expect(name);
        let body = &source[start..];
        let open = body.find(">(").unwrap() + 2;
        let close = body.find(");").unwrap();
        body[open..close].split(',').map(|v| v.trim().parse().unwrap()).collect()
    }

    #[test]
    fn the_shader_and_the_cpu_hold_the_same_table() {
        let source = include_str!("tone_input.wesl");
        assert_eq!(wesl_array(source, "TONY_IN"), TONY_IN.to_vec());
        assert_eq!(wesl_array(source, "TONY_OUT"), TONY_OUT.to_vec());
    }

    #[test]
    fn the_inverse_undoes_the_table() {
        for (x, y) in TONY_IN.iter().zip(TONY_OUT.iter()) {
            assert!((tony_inverse(*y) - x).abs() < 1e-4 && (tony_forward(*x) - y).abs() < 1e-6, "{x} {y}");
        }
    }
}
