//! Session-only policy; no engine/settings dependencies, shared with the CPU replay tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Level {
    #[default]
    Manual,
    Warm,
    Hot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Sample {
    pub active: bool,
    pub status: i32,
    pub headroom_milli: i32,
    pub age_ms: u64,
}

impl Sample {
    #[cfg(any(target_os = "android", test))]
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() > 40 {
            return None;
        }
        let mut parts = text.split(',');
        let active = match parts.next()? {
            "0" => false,
            "1" => true,
            _ => return None,
        };
        let status = parts.next()?.parse().ok()?;
        let headroom_milli = parts.next()?.parse().ok()?;
        let age_ms = parts.next()?.parse().ok()?;
        if parts.next().is_some()
            || !(-1..=6).contains(&status)
            || !(-1..=10000).contains(&headroom_milli)
        {
            return None;
        }
        Some(Self {
            active,
            status,
            headroom_milli,
            age_ms,
        })
    }
    pub fn usable(self) -> bool {
        self.active && self.age_ms <= 25_000 && (self.status >= 0 || self.headroom_milli >= 0)
    }
    fn desired(self) -> Level {
        if self.status >= 3 || self.headroom_milli >= 1000 {
            Level::Hot
        } else if self.status >= 2 || self.headroom_milli >= 800 {
            Level::Warm
        } else {
            Level::Manual
        }
    }
    fn cool(self) -> bool {
        self.status <= 1 && (self.headroom_milli < 0 || self.headroom_milli < 650)
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Policy {
    pub level: Level,
    warm_since: Option<u64>,
    cool_since: Option<u64>,
    last_time: Option<u64>,
}

impl Policy {
    pub fn update(&mut self, now_ms: u64, consent: bool, sample: Option<Sample>) {
        if self.last_time.is_some_and(|last| now_ms < last) {
            *self = Self::default();
        }
        if self
            .last_time
            .is_some_and(|last| now_ms.saturating_sub(last) > 25_000)
        {
            // A foreground stall is not evidence of continuous warming/cooling.
            self.warm_since = None;
            self.cool_since = None;
        }
        self.last_time = Some(now_ms);
        let Some(sample) = sample.filter(|s| consent && s.usable()) else {
            *self = Self {
                last_time: Some(now_ms),
                ..Self::default()
            };
            return;
        };
        let desired = sample.desired();
        if desired == Level::Hot {
            self.level = Level::Hot;
            self.warm_since = None;
            self.cool_since = None;
        } else if desired == Level::Warm {
            self.cool_since = None;
            if self.level == Level::Manual {
                let since = *self.warm_since.get_or_insert(now_ms);
                if now_ms.saturating_sub(since) >= 5_000 {
                    self.level = Level::Warm;
                    self.warm_since = None;
                }
            }
        } else {
            self.warm_since = None;
            if sample.cool() && self.level != Level::Manual {
                let since = *self.cool_since.get_or_insert(now_ms);
                if now_ms.saturating_sub(since) >= 60_000 {
                    self.level = match self.level {
                        Level::Hot => Level::Warm,
                        _ => Level::Manual,
                    };
                    self.cool_since = None;
                }
            } else {
                self.cool_since = None;
            }
        }
    }
    pub fn cap(&self, requested: u32) -> u32 {
        let temporary = match self.level {
            Level::Manual => return requested,
            Level::Warm => 45,
            Level::Hot => 30,
        };
        if requested == 0 {
            temporary
        } else {
            requested.min(temporary)
        }
    }
    /// Rank 0=native AA, 1=67%, 2=59%, 3=50%, 4=33%. Never increases workload.
    pub fn fsr_rank(&self, requested: u8) -> u8 {
        requested.max(match self.level {
            Level::Manual => 0,
            Level::Warm => 2,
            Level::Hot => 3,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reading(status: i32, headroom: i32) -> Option<Sample> {
        Some(Sample {
            active: true,
            status,
            headroom_milli: headroom,
            age_ms: 0,
        })
    }
    #[test]
    fn consent_and_feedback_loss_use_manual_settings() {
        let mut p = Policy::default();
        p.update(0, false, reading(6, 1200));
        assert_eq!(p.cap(60), 60);
        p.update(1, true, reading(3, -1));
        assert_eq!(p.cap(60), 30);
        for sample in [
            None,
            Some(Sample {
                active: false,
                ..reading(3, -1).unwrap()
            }),
            Some(Sample {
                age_ms: 25001,
                ..reading(3, -1).unwrap()
            }),
            reading(-1, -1),
        ] {
            p.update(2, true, reading(3, -1));
            p.update(3, true, sample);
            assert_eq!(p.level, Level::Manual);
            assert_eq!(p.cap(0), 0);
        }
    }
    #[test]
    fn warm_requires_continuous_five_seconds_and_hot_is_immediate() {
        let mut p = Policy::default();
        p.update(0, true, reading(2, -1));
        p.update(4999, true, reading(2, -1));
        assert_eq!(p.level, Level::Manual);
        p.update(5000, true, reading(1, -1));
        p.update(6000, true, reading(0, 850));
        p.update(10999, true, reading(0, 850));
        assert_eq!(p.level, Level::Manual);
        p.update(11000, true, reading(0, 850));
        assert_eq!(p.cap(60), 45);
        p.update(11001, true, reading(0, 1000));
        assert_eq!(p.cap(60), 30);
    }
    #[test]
    fn recovery_needs_continuous_cooling_and_one_step_per_minute() {
        let mut p = Policy::default();
        p.update(0, true, reading(3, 1100));
        p.update(1, true, reading(0, 600));
        for now in [10001, 20001, 30001, 40001, 50001, 60000] {
            p.update(now, true, reading(0, 600));
        }
        assert_eq!(p.level, Level::Hot);
        p.update(60001, true, reading(0, 700));
        p.update(70000, true, reading(1, 600));
        for now in [80000, 90000, 100000, 110000, 120000, 130000] {
            p.update(now, true, reading(1, 600));
        }
        assert_eq!(p.level, Level::Warm);
        p.update(130001, true, reading(0, 600));
        for now in [140001, 150001, 160001, 170001, 180001, 190000] {
            p.update(now, true, reading(0, 600));
        }
        assert_eq!(p.level, Level::Warm);
        p.update(190001, true, reading(0, 600));
        assert_eq!(p.level, Level::Manual);
    }
    #[test]
    fn manual_edits_are_always_the_baseline_not_an_old_snapshot() {
        let mut p = Policy::default();
        p.update(0, true, reading(3, -1));
        assert_eq!(p.cap(20), 20);
        assert_eq!(p.cap(120), 30);
        assert_eq!(p.fsr_rank(4), 4);
        assert_eq!(p.fsr_rank(1), 3);
        p.update(1, false, reading(3, -1));
        assert_eq!(p.cap(120), 120);
        assert_eq!(p.fsr_rank(1), 1);
    }
    #[test]
    fn sample_parser_rejects_unbounded_unknown_and_non_numeric_input() {
        assert_eq!(Sample::parse("1,3,900,10").unwrap().status, 3);
        for bad in [
            "",
            "2,0,0,0",
            "1,7,0,0",
            "1,0,NaN,0",
            "1,0,10001,0",
            "1,0,0,-1",
            "1,0,0,0,extra",
        ] {
            assert!(Sample::parse(bad).is_none(), "{bad}");
        }
    }
    #[test]
    fn missing_feedback_intervals_do_not_count_as_continuous_cooling() {
        let mut p = Policy::default();
        p.update(0, true, reading(3, -1));
        p.update(1, true, reading(0, 500));
        p.update(120001, true, reading(0, 500));
        assert_eq!(
            p.level,
            Level::Hot,
            "a long foreground stall is not a minute of observed cooling"
        );
        for now in [130001, 140001, 150001, 160001, 170001, 180001] {
            p.update(now, true, reading(0, 500));
        }
        assert_eq!(p.level, Level::Warm);
    }
}
