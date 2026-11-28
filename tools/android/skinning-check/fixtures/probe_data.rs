//! Original tiny mesh/oracle data shared with the CPU-only harness. No engine dependencies.
pub const POINTS: [[f32; 3]; 5] = [
    [-1.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 2.0, 0.0],
    [-1.0, 2.0, 0.0],
    [0.0, 1.0, 0.0],
];
pub const WEIGHTS: [[f32; 4]; 5] = [
    [1.0, 0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.25, 0.75, 0.0, 0.0],
];
pub const INDICES: [u32; 12] = [0, 1, 4, 1, 2, 4, 2, 3, 4, 3, 0, 4];

pub fn joint_columns(frame: usize) -> [[f32; 16]; 2] {
    let cols = if frame % 2 == 0 {
        [
            [
                2., 0., 0., 0., 0., 3., 0., 0., 0., 0., 1., 0., 1., 2., 3., 1.,
            ],
            [
                0., 1., 0., 0., -2., 0., 0., 0., 0., 0., 3., 0., -3., 4., 5., 1.,
            ],
        ]
    } else {
        [
            [
                0., -2., 0., 0., 3., 0., 0., 0., 0., 0., 1., 0., 2., -1., 4., 1.,
            ],
            [
                -1., 0., 0., 0., 0., 2., 0., 0., 0., 0., -3., 0., 4., 3., -2., 1.,
            ],
        ]
    };
    cols
}
pub fn morph_weight(frame: usize) -> f32 {
    if frame % 2 == 0 { 0.0 } else { 0.5 }
}
pub fn cpu_positions(frame: usize) -> Vec<[f32; 3]> {
    let joints = joint_columns(frame);
    POINTS
        .iter()
        .zip(WEIGHTS)
        .map(|(point, weight)| {
            let p = [point[0], point[1], point[2] + morph_weight(frame), 1.0];
            std::array::from_fn(|row| {
                (0..2)
                    .map(|joint| {
                        weight[joint]
                            * (0..4)
                                .map(|col| joints[joint][col * 4 + row] * p[col])
                                .sum::<f32>()
                    })
                    .sum()
            })
        })
        .collect()
}
