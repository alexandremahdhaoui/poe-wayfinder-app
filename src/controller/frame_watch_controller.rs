use crate::types::time::Millis;

pub const STALL_AFTER: Millis = 3_000;
pub const REPEAT_EVERY: Millis = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Stalled { silent_ms: Millis },
    Ticking,
    Recovered { stalled_for_ms: Millis },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FrameWatch {
    told_at: Option<Millis>,
    stalled_since: Option<Millis>,
}

impl FrameWatch {
    pub fn look(&mut self, now: Millis, last_tick: Millis) -> Option<Verdict> {
        let silent_ms = now.saturating_sub(last_tick);

        if silent_ms < STALL_AFTER {
            self.told_at = None;

            let Some(since) = self.stalled_since.take() else {
                return Some(Verdict::Ticking);
            };

            return Some(Verdict::Recovered {
                stalled_for_ms: last_tick.saturating_sub(since),
            });
        }

        self.stalled_since.get_or_insert(last_tick);

        let due = match self.told_at {
            None => true,
            Some(told) => now.saturating_sub(told) >= REPEAT_EVERY,
        };

        if !due {
            return None;
        }

        self.told_at = Some(now);

        Some(Verdict::Stalled { silent_ms })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loop_that_ticked_just_now_is_ticking_and_has_no_stall_to_report() {
        let mut watch = FrameWatch::default();

        assert_eq!(watch.look(1_000, 900), Some(Verdict::Ticking));
    }

    #[test]
    fn a_loop_silent_for_longer_than_the_stall_window_is_reported_once() {
        let mut watch = FrameWatch::default();

        assert_eq!(
            watch.look(STALL_AFTER, 0),
            Some(Verdict::Stalled {
                silent_ms: STALL_AFTER
            })
        );
        assert_eq!(watch.look(STALL_AFTER + 1_000, 0), None);
    }

    #[test]
    fn a_stall_that_lasts_is_repeated_so_a_long_log_still_shows_it() {
        let mut watch = FrameWatch::default();

        watch.look(STALL_AFTER, 0);

        assert_eq!(
            watch.look(STALL_AFTER + REPEAT_EVERY, 0),
            Some(Verdict::Stalled {
                silent_ms: STALL_AFTER + REPEAT_EVERY
            })
        );
    }

    #[test]
    fn a_recovery_reports_how_long_the_hotkey_was_dead_rather_than_the_current_gap() {
        let mut watch = FrameWatch::default();

        watch.look(STALL_AFTER, 0);

        assert_eq!(
            watch.look(STALL_AFTER + 100, STALL_AFTER + 50),
            Some(Verdict::Recovered {
                stalled_for_ms: STALL_AFTER + 50
            })
        );
    }

    #[test]
    fn a_recovery_is_reported_once_so_the_stall_has_exactly_one_end() {
        let mut watch = FrameWatch::default();

        watch.look(STALL_AFTER, 0);
        watch.look(STALL_AFTER + 100, STALL_AFTER + 50);

        assert_eq!(
            watch.look(STALL_AFTER + 200, STALL_AFTER + 150),
            Some(Verdict::Ticking)
        );
    }

    #[test]
    fn a_second_stall_reports_its_own_length_rather_than_counting_from_the_first_one() {
        let mut watch = FrameWatch::default();

        assert_eq!(
            watch.look(3_000, 0),
            Some(Verdict::Stalled { silent_ms: 3_000 })
        );
        assert_eq!(
            watch.look(3_100, 3_050),
            Some(Verdict::Recovered {
                stalled_for_ms: 3_050
            })
        );
        assert_eq!(watch.look(3_200, 3_150), Some(Verdict::Ticking));
        assert_eq!(
            watch.look(7_000, 3_150),
            Some(Verdict::Stalled { silent_ms: 3_850 })
        );
        assert_eq!(
            watch.look(7_100, 7_050),
            Some(Verdict::Recovered {
                stalled_for_ms: 3_900
            })
        );
    }

    #[test]
    fn a_loop_that_never_stalled_never_reports_a_recovery() {
        let mut watch = FrameWatch::default();

        for now in [100, 200, 300, 400] {
            assert_eq!(watch.look(now, now), Some(Verdict::Ticking));
        }
    }

    #[test]
    fn a_tick_stamped_after_the_clock_read_is_treated_as_alive_rather_than_panicking() {
        let mut watch = FrameWatch::default();

        assert_eq!(watch.look(100, 500), Some(Verdict::Ticking));
    }
}
