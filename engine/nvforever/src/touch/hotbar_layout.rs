//! Bounded, deterministic packing around actual active control geometry.
//! No bindings or saved placement are changed by responsive fitting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect2 {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect2 {
    pub fn valid(self) -> bool {
        [self.x, self.y, self.w, self.h]
            .into_iter()
            .all(f32::is_finite)
            && self.w > 0.
            && self.h > 0.
    }
    pub fn center(self) -> [f32; 2] {
        [self.x + self.w * 0.5, self.y + self.h * 0.5]
    }
    fn expanded(self, amount: f32) -> Self {
        Self {
            x: self.x - amount,
            y: self.y - amount,
            w: self.w + amount * 2.,
            h: self.h + amount * 2.,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Occupied {
    pub rect: Rect2,
    pub radius: f32,
}
impl Occupied {
    pub fn overlaps(self, rect: Rect2, gap: f32) -> bool {
        let rect = rect.expanded(gap);
        if rect.x >= self.rect.x + self.rect.w
            || rect.x + rect.w <= self.rect.x
            || rect.y >= self.rect.y + self.rect.h
            || rect.y + rect.h <= self.rect.y
        {
            return false;
        }
        let radius = self.radius.clamp(0., self.rect.w.min(self.rect.h) * 0.5);
        if radius == 0. {
            return true;
        }
        let center = self.rect.center();
        let dx = (center[0] - rect.x)
            .min(0.)
            .abs()
            .max((center[0] - (rect.x + rect.w)).max(0.));
        let dy = (center[1] - rect.y)
            .min(0.)
            .abs()
            .max((center[1] - (rect.y + rect.h)).max(0.));
        let x = (dx - (self.rect.w * 0.5 - radius)).max(0.);
        let y = (dy - (self.rect.h * 0.5 - radius)).max(0.);
        x * x + y * y <= radius * radius
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Resolved {
    pub cells: Vec<Rect2>,
    pub panel: Rect2,
    pub side: f32,
    pub compacted: bool,
}
const GAP: f32 = 6.;
const MARGIN: f32 = 4.;
const MAX_AXIS: usize = 64;
fn axis(limit: f32, extent: f32, wanted: f32, occupied: &[Occupied], x: bool) -> Vec<f32> {
    let high = limit - extent - MARGIN;
    let mut values = vec![
        wanted.clamp(MARGIN, high),
        MARGIN,
        high,
        (limit - extent) * 0.5,
    ];
    for shape in occupied {
        let (start, size) = if x {
            (shape.rect.x, shape.rect.w)
        } else {
            (shape.rect.y, shape.rect.h)
        };
        values.push((start - MARGIN - extent).clamp(MARGIN, high));
        values.push((start + size + MARGIN).clamp(MARGIN, high));
    }
    values.sort_by(|a, b| {
        (a - wanted)
            .abs()
            .total_cmp(&(b - wanted).abs())
            .then(a.total_cmp(b))
    });
    values.dedup_by(|a, b| (*a - *b).abs() < 0.001);
    values.truncate(MAX_AXIS);
    values
}
pub(super) fn fit(
    size: [f32; 2],
    wanted: [f32; 2],
    requested_side: f32,
    count: usize,
    occupied: &[Occupied],
) -> Option<Resolved> {
    if !size
        .into_iter()
        .chain(wanted)
        .chain([requested_side])
        .all(f32::is_finite)
        || size[0] < 56.
        || size[1] < 56.
        || !(1..=9).contains(&count)
        || occupied.len() > 32
        || occupied
            .iter()
            .any(|s| !s.rect.valid() || !s.radius.is_finite() || s.radius < 0.)
    {
        return None;
    }
    let requested = requested_side.clamp(48., 96.);
    let mut side = requested;
    loop {
        let mut best: Option<(f32, Rect2, usize)> = None;
        for columns in 1..=count {
            let rows = count.div_ceil(columns);
            let width = columns as f32 * (side + GAP) - GAP;
            let height = rows as f32 * (side + GAP) - GAP;
            if width > size[0] - MARGIN * 2. || height > size[1] - MARGIN * 2. {
                continue;
            }
            let preferred = [
                wanted[0].clamp(0., 1.) * size[0] - width * 0.5,
                wanted[1].clamp(0., 1.) * size[1] - height * 0.5,
            ];
            let xs = axis(size[0], width, preferred[0], occupied, true);
            let ys = axis(size[1], height, preferred[1], occupied, false);
            for &x in &xs {
                for &y in &ys {
                    let center = [x + width * 0.5, y + height * 0.5];
                    let distance = (center[0] - wanted[0] * size[0]).powi(2)
                        + (center[1] - wanted[1] * size[1]).powi(2);
                    let score = distance + ((rows - 1) as f32 * side * 0.2).powi(2);
                    if best.as_ref().is_some_and(|(old, _, _)| score >= *old) {
                        continue;
                    }
                    let valid = (0..count).all(|i| {
                        let rect = Rect2 {
                            x: x + (i % columns) as f32 * (side + GAP),
                            y: y + (i / columns) as f32 * (side + GAP),
                            w: side,
                            h: side,
                        };
                        occupied.iter().all(|shape| !shape.overlaps(rect, MARGIN))
                    });
                    if valid {
                        best = Some((
                            score,
                            Rect2 {
                                x,
                                y,
                                w: width,
                                h: height,
                            },
                            columns,
                        ));
                    }
                }
            }
        }
        if let Some((_, panel, columns)) = best {
            let cells = (0..count)
                .map(|i| Rect2 {
                    x: panel.x + (i % columns) as f32 * (side + GAP),
                    y: panel.y + (i / columns) as f32 * (side + GAP),
                    w: side,
                    h: side,
                })
                .collect();
            return Some(Resolved {
                cells,
                panel,
                side,
                compacted: side < requested,
            });
        }
        if side <= 48. {
            return None;
        }
        side = (side - 8.).max(48.);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn occupied_corner_geometry_is_not_its_bounding_square() {
        let circle = Occupied {
            rect: Rect2 {
                x: 0.,
                y: 0.,
                w: 100.,
                h: 100.,
            },
            radius: 50.,
        };
        assert!(!circle.overlaps(
            Rect2 {
                x: 0.,
                y: 0.,
                w: 5.,
                h: 5.
            },
            0.
        ));
        assert!(circle.overlaps(
            Rect2 {
                x: 45.,
                y: 0.,
                w: 10.,
                h: 5.
            },
            0.
        ));
        let square = Occupied {
            radius: 0.,
            ..circle
        };
        assert!(square.overlaps(
            Rect2 {
                x: 0.,
                y: 0.,
                w: 5.,
                h: 5.
            },
            0.
        ));
    }
    #[test]
    fn fitting_respects_cells_bounds_obstacles_minimum_and_saved_anchor() {
        let obstacles = [
            Occupied {
                rect: Rect2 {
                    x: 280.,
                    y: 98.,
                    w: 48.,
                    h: 48.,
                },
                radius: 24.,
            },
            Occupied {
                rect: Rect2 {
                    x: 40.,
                    y: 206.,
                    w: 136.,
                    h: 136.,
                },
                radius: 68.,
            },
            Occupied {
                rect: Rect2 {
                    x: 295.,
                    y: 155.,
                    w: 50.,
                    h: 50.,
                },
                radius: 0.,
            },
        ];
        for count in [5, 7, 9] {
            let resolved = fit([640., 272.], [0.5, 0.82], 96., count, &obstacles)
                .expect("responsive panel has room");
            assert_eq!(resolved.cells.len(), count);
            assert!(resolved.side >= 48.);
            for r in resolved.cells {
                assert!(r.x >= 4. && r.y >= 4. && r.x + r.w <= 636. && r.y + r.h <= 268.);
                assert!(obstacles.iter().all(|o| !o.overlaps(r, 0.)));
            }
        }
    }
    #[test]
    fn invalid_or_fully_occupied_canvas_never_produces_hidden_hit_targets() {
        assert!(fit([640., 272.], [f32::NAN, 0.8], 48., 9, &[]).is_none());
        assert!(fit([40., 40.], [0.5, 0.8], 48., 9, &[]).is_none());
        assert!(fit(
            [640., 272.],
            [0.5, 0.8],
            48.,
            9,
            &[Occupied {
                rect: Rect2 {
                    x: 0.,
                    y: 0.,
                    w: 640.,
                    h: 272.
                },
                radius: 0.
            }]
        )
        .is_none());
    }
}
