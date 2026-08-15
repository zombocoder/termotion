use termotion_core::{Fps, TerminalState, Time};

use crate::program::{apply_op, Program, RegionKind};

/// How often a full grid snapshot is taken during forward replay. A snapshot of a
/// typical 70x15 grid is a few kilobytes, so this is cheap insurance against
/// expensive backward seeks.
pub const SNAPSHOT_INTERVAL: Time = Time::from_nanos(2_000_000_000);

/// Evaluates a compiled program at any point in virtual time.
///
/// `state_at` is semantically pure: the result depends only on `t`. Forward replay
/// is an internal optimization; a backward seek restarts from the nearest snapshot.
pub struct Runtime {
    program: Program,
    state: TerminalState,
    /// Index of the next event to apply.
    next_event: usize,
    /// Time the replayed state currently corresponds to.
    at: Time,
    snapshots: Vec<Snapshot>,
    next_snapshot_at: Time,
}

struct Snapshot {
    at: Time,
    next_event: usize,
    state: TerminalState,
}

impl Runtime {
    pub fn new(program: Program) -> Self {
        let state = Self::fresh_state(&program);
        Runtime {
            program,
            state,
            next_event: 0,
            at: Time::ZERO,
            snapshots: Vec::new(),
            next_snapshot_at: SNAPSHOT_INTERVAL,
        }
    }

    fn fresh_state(program: &Program) -> TerminalState {
        let mut state = TerminalState::new(program.grid.cols, program.grid.rows, program.overflow);
        state.styles = program.styles.clone();
        state.current_style = program.initial_style;
        state.cursor.style = program.cursor.style;
        state.cursor.blink = program.cursor.blink;
        state.cursor.visible = program.cursor.visible;
        state
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn duration(&self) -> Time {
        self.program.duration
    }

    pub fn frame_count(&self, fps: Fps) -> u64 {
        fps.frame_count(self.program.duration)
    }

    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    pub fn state_at(&mut self, t: Time) -> &TerminalState {
        if t < self.at {
            self.rewind_to(t);
        }
        self.advance_to(t);
        self.apply_cursor(t);
        &self.state
    }

    /// Restores the newest snapshot at or before `t`, or resets to a fresh state.
    fn rewind_to(&mut self, t: Time) {
        match self.snapshots.iter().rposition(|snap| snap.at <= t) {
            Some(index) => {
                let snapshot = &self.snapshots[index];
                self.state = snapshot.state.clone();
                self.next_event = snapshot.next_event;
                self.at = snapshot.at;
            }
            None => {
                self.state = Self::fresh_state(&self.program);
                self.next_event = 0;
                self.at = Time::ZERO;
            }
        }
    }

    /// Records a snapshot at `next_snapshot_at` using the current state, and
    /// advances the boundary by one interval.
    ///
    /// A snapshot recorded at time `B` must hold state reflecting exactly the
    /// events with `at <= B`. This is only called in two places: from inside the
    /// event loop (while a not-yet-applied event's `at` is still ahead of the
    /// boundary, so the current state has no events past `B`), and after the
    /// event loop (where the invariant maintained by the first call site
    /// guarantees the same thing — see `advance_to`).
    fn record_snapshot(&mut self) {
        self.snapshots.push(Snapshot {
            at: self.next_snapshot_at,
            next_event: self.next_event,
            state: self.state.clone(),
        });
        self.next_snapshot_at = self.next_snapshot_at.saturating_add(SNAPSHOT_INTERVAL);
    }

    fn advance_to(&mut self, t: Time) {
        while self.next_event < self.program.events.len() {
            // Read the event's time as a plain `Copy` value (rather than
            // holding a `&Event` borrow of `self.program`) so the inner
            // snapshot loop below is free to take `&mut self`.
            let event_at = self.program.events[self.next_event].at;
            if event_at > t {
                break;
            }

            // Snapshot every boundary strictly before this event's time, using
            // the state as it stands right now (i.e. reflecting only events
            // already applied, all of which have `at <= next_snapshot_at`). A
            // single large gap between events can span several boundaries, so
            // this loops rather than taking just one snapshot. Strict `<` means
            // an event landing exactly on a boundary is folded into that
            // boundary's snapshot below, consistent with `at <= t` semantics.
            while self.next_snapshot_at < event_at {
                self.record_snapshot();
            }

            let op = self.program.events[self.next_event].op.clone();
            apply_op(&mut self.state, &op);
            self.next_event += 1;
        }

        // Catch up any remaining boundaries at or before `t`. This is sound
        // because the loop above guarantees every already-applied event has
        // `at <= next_snapshot_at`: had a later event existed with
        // `at <= next_snapshot_at`, it would have been applied above (the outer
        // loop only stops when the next event's `at > t`, and any event with
        // `at <= next_snapshot_at <= t` would not satisfy that).
        while t >= self.next_snapshot_at {
            self.record_snapshot();
        }

        self.at = t;
    }

    /// Cursor visibility is derived from `t`, never from an event, so it cannot
    /// desync from the rest of the animation.
    fn apply_cursor(&mut self, t: Time) {
        let mut visible = self.program.cursor.visible;
        let mut blink = self.program.cursor.blink;

        for region in &self.program.regions {
            if t >= region.start && t < region.end {
                let RegionKind::Cursor {
                    visible: region_visible,
                    blink: region_blink,
                } = &region.kind;
                visible = *region_visible;
                blink = *region_blink;
            }
        }

        if let Some(period) = blink {
            if period > Time::ZERO {
                let phase = t.as_nanos() / period.as_nanos();
                visible = visible && phase.is_multiple_of(2);
            }
        }

        self.state.cursor.visible = visible;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termotion_core::{Action, ClearEffect, Fps, GridSpec, Scenario, TextRun, Time};

    fn scenario(timeline: Vec<Action>) -> Scenario {
        Scenario {
            metadata: Default::default(),
            canvas: Default::default(),
            terminal: Default::default(),
            font: Default::default(),
            prompt: Default::default(),
            cursor: Default::default(),
            palette: Default::default(),
            playback: Default::default(),
            seed: 0,
            timeline,
        }
    }

    fn runtime(timeline: Vec<Action>) -> Runtime {
        let program = crate::compile(&scenario(timeline), GridSpec { cols: 40, rows: 8 }).unwrap();
        Runtime::new(program)
    }

    fn typed(text: &str, speed_ms: u64) -> Vec<Action> {
        vec![Action::Write {
            spans: vec![TextRun {
                text: text.to_string(),
                color: None,
            }],
            speed: Time::from_millis(speed_ms),
        }]
    }

    fn row_text(state: &termotion_core::TerminalState, row: u16) -> String {
        state
            .grid
            .row(row)
            .map(|r| r.cells.iter().map(|c| c.g.as_str()).collect::<String>())
            .unwrap_or_default()
            .trim_end()
            .to_string()
    }

    #[test]
    fn reveals_characters_progressively() {
        let mut rt = runtime(typed("hello", 100));
        assert_eq!(row_text(rt.state_at(Time::from_millis(0)), 0), "h");
        assert_eq!(row_text(rt.state_at(Time::from_millis(250)), 0), "hel");
        assert_eq!(row_text(rt.state_at(Time::from_millis(1_000)), 0), "hello");
    }

    #[test]
    fn state_before_the_first_event_is_empty() {
        let mut rt = runtime(vec![
            Action::Pause {
                duration: Time::from_millis(500),
            },
            Action::Write {
                spans: vec![TextRun {
                    text: "x".into(),
                    color: None,
                }],
                speed: Time::ZERO,
            },
        ]);
        assert_eq!(row_text(rt.state_at(Time::from_millis(499)), 0), "");
        assert_eq!(row_text(rt.state_at(Time::from_millis(500)), 0), "x");
    }

    #[test]
    fn backward_seek_matches_a_fresh_forward_replay() {
        let actions = typed("abcdefghij", 100);

        let mut seeking = runtime(actions.clone());
        // Jump to the end, then walk backwards.
        seeking.state_at(Time::from_millis(1_000));

        for ms in [900, 700, 350, 120, 0] {
            let seeking_text = row_text(seeking.state_at(Time::from_millis(ms)), 0);
            let mut fresh = runtime(actions.clone());
            let fresh_text = row_text(fresh.state_at(Time::from_millis(ms)), 0);
            assert_eq!(seeking_text, fresh_text, "mismatch at {ms}ms");
        }
    }

    #[test]
    fn repeated_queries_at_the_same_time_are_stable() {
        let mut rt = runtime(typed("abc", 100));
        let first = row_text(rt.state_at(Time::from_millis(150)), 0);
        let second = row_text(rt.state_at(Time::from_millis(150)), 0);
        assert_eq!(first, second);
    }

    #[test]
    fn snapshots_are_taken_across_a_long_scenario() {
        let mut rt = runtime(vec![
            Action::Pause {
                duration: Time::from_millis(10_000),
            },
            Action::Write {
                spans: vec![TextRun {
                    text: "end".into(),
                    color: None,
                }],
                speed: Time::ZERO,
            },
        ]);
        rt.state_at(Time::from_millis(10_000));
        assert!(rt.snapshot_count() >= 4, "expected snapshots every 2s");
    }

    #[test]
    fn seeking_past_a_clear_does_not_resurrect_cleared_text() {
        let mut rt = runtime(vec![
            Action::Write {
                spans: vec![TextRun {
                    text: "before".into(),
                    color: None,
                }],
                speed: Time::ZERO,
            },
            Action::Pause {
                duration: Time::from_millis(500),
            },
            Action::Clear {
                effect: ClearEffect::Instant,
            },
        ]);
        rt.state_at(Time::from_millis(1_000));
        assert_eq!(row_text(rt.state_at(Time::from_millis(100)), 0), "before");
        assert_eq!(row_text(rt.state_at(Time::from_millis(600)), 0), "");
    }

    #[test]
    fn cursor_blinks_as_a_function_of_time() {
        let mut rt = runtime(vec![Action::Cursor {
            visible: true,
            blink: Some(Time::from_millis(500)),
            duration: Time::from_millis(4_000),
        }]);

        assert!(rt.state_at(Time::from_millis(0)).cursor.visible);
        assert!(!rt.state_at(Time::from_millis(500)).cursor.visible);
        assert!(rt.state_at(Time::from_millis(1_000)).cursor.visible);
        // Brief correction: the source test asserted this at 1_499ms, which
        // falls inside the [1000, 1500) "on" half-cycle under the documented
        // half-period floor-division semantics (`phase = t / period`, visible
        // iff `phase` is even) — so it is provably *visible* there, not
        // invisible. 1_999ms falls inside the following [1500, 2000) "off"
        // half-cycle and is what the assertion evidently intended to probe: a
        // non-boundary instant deep inside an "off" period, distinct from the
        // exact-boundary check at 500ms above.
        assert!(!rt.state_at(Time::from_millis(1_999)).cursor.visible);
    }

    #[test]
    fn a_zero_blink_period_means_a_steady_cursor() {
        let mut rt = runtime(vec![Action::Cursor {
            visible: true,
            blink: None,
            duration: Time::from_millis(1_000),
        }]);
        let mut scenario_no_blink = scenario(vec![]);
        scenario_no_blink.cursor.blink = None;
        let program = crate::compile(&scenario_no_blink, GridSpec { cols: 10, rows: 4 }).unwrap();
        let mut steady = Runtime::new(program);
        assert!(steady.state_at(Time::from_millis(0)).cursor.visible);
        assert!(steady.state_at(Time::from_millis(750)).cursor.visible);
        let _ = rt.state_at(Time::ZERO);
    }

    #[test]
    fn a_cursor_action_can_hide_the_cursor_for_its_span() {
        let mut rt = runtime(vec![Action::Cursor {
            visible: false,
            blink: None,
            duration: Time::from_millis(1_000),
        }]);
        assert!(!rt.state_at(Time::from_millis(500)).cursor.visible);
    }

    #[test]
    fn the_cursor_follows_typed_text() {
        let mut rt = runtime(typed("abc", 100));
        let state = rt.state_at(Time::from_millis(1_000));
        assert_eq!((state.cursor.row, state.cursor.col), (0, 3));
    }

    #[test]
    fn frame_count_covers_the_full_duration() {
        let rt = runtime(typed("abcde", 200)); // 1000ms
        assert_eq!(rt.duration(), Time::from_millis(1_000));
        assert_eq!(rt.frame_count(Fps::from_integer(30)), 30);
    }

    /// Regression test for a mislabelled-snapshot bug (ruling R3): a naive
    /// implementation applies every event with `at <= t` first, and only then
    /// walks `next_snapshot_at` up to `t`, labelling each snapshot with the
    /// boundary time but storing state as of `t`. When a forward seek jumps
    /// past more than one boundary in a single call, every skipped boundary
    /// gets a snapshot that claims to hold state as of e.g. 2s but actually
    /// holds state from far later. A subsequent backward seek then restores
    /// that mislabelled snapshot (and its equally-final `next_event`, so
    /// `advance_to` has nothing left to trim) and silently returns content
    /// from the future.
    ///
    /// The scenario writes distinguishable content just before and just after
    /// both the 2s and 4s boundaries (with the gap between straddling each
    /// boundary, so no event lands exactly on one), and runs past 6s so three
    /// boundaries exist. A single forward jump to the end crosses all three
    /// in one `advance_to` call — the precondition for the bug, since a naive
    /// implementation only mislabels boundaries it crosses together. Seeking
    /// back to points before the 4s and 6s boundaries (each still at or after
    /// an earlier boundary, so a stored snapshot is actually used rather than
    /// a from-scratch reset) must then match a fresh `Runtime` computing the
    /// same time directly, which never takes snapshots and so cannot be
    /// fooled by a stale one.
    #[test]
    fn backward_seek_after_a_multi_boundary_forward_jump_matches_fresh_replay() {
        let actions = vec![
            // t=0: "AAA" appears.
            Action::Write {
                spans: vec![TextRun {
                    text: "AAA".into(),
                    color: None,
                }],
                speed: Time::from_millis(0),
            },
            // 0..1900ms, straddling nothing yet.
            Action::Pause {
                duration: Time::from_millis(1_900),
            },
            // t=1900 (just before the 2s boundary): "BBB" appears.
            Action::Write {
                spans: vec![TextRun {
                    text: "BBB".into(),
                    color: None,
                }],
                speed: Time::from_millis(0),
            },
            // 1900..2100ms, straddling the 2s boundary.
            Action::Pause {
                duration: Time::from_millis(200),
            },
            // t=2100 (just after the 2s boundary): "CCC" appears.
            Action::Write {
                spans: vec![TextRun {
                    text: "CCC".into(),
                    color: None,
                }],
                speed: Time::from_millis(0),
            },
            // 2100..3900ms.
            Action::Pause {
                duration: Time::from_millis(1_800),
            },
            // t=3900 (just before the 4s boundary): "DDD" appears.
            Action::Write {
                spans: vec![TextRun {
                    text: "DDD".into(),
                    color: None,
                }],
                speed: Time::from_millis(0),
            },
            // 3900..4100ms, straddling the 4s boundary.
            Action::Pause {
                duration: Time::from_millis(200),
            },
            // t=4100 (just after the 4s boundary): "EEE" appears.
            Action::Write {
                spans: vec![TextRun {
                    text: "EEE".into(),
                    color: None,
                }],
                speed: Time::from_millis(0),
            },
            // Pad out well past the 6s boundary; duration ends at 6600ms.
            Action::Pause {
                duration: Time::from_millis(2_500),
            },
        ];

        let mut seeking = runtime(actions.clone());
        assert!(
            seeking.duration() > Time::from_millis(6_000),
            "scenario must run past 6s"
        );

        // Single forward jump to the end, crossing the 2s, 4s, and 6s
        // boundaries all in one `advance_to` call.
        let dur = seeking.duration();
        seeking.state_at(dur);
        assert!(
            seeking.snapshot_count() >= 3,
            "expected snapshots at 2s, 4s, 6s"
        );

        // Seek backward to points before the 2s, 4s, and 6s boundaries. Each
        // check re-jumps to the end first, so every backward seek genuinely
        // goes through `rewind_to` starting from the post-jump `at` — rather
        // than, say, chaining seeks in ascending order, where the second seek
        // would just be a forward continuation from the first and never touch
        // a stored snapshot at all. The 3_950ms and 5_950ms points land at or
        // after an earlier boundary (2s and 4s respectively), so `rewind_to`
        // actually reuses a stored snapshot instead of resetting from
        // scratch — exactly the case the naive implementation gets wrong.
        for ms in [1_950u64, 3_950, 5_950] {
            seeking.state_at(dur);
            let mut fresh = runtime(actions.clone());
            let expected = fresh.state_at(Time::from_millis(ms)).clone();
            let actual = seeking.state_at(Time::from_millis(ms)).clone();
            assert_eq!(
                actual, expected,
                "mismatch at {ms}ms after a multi-boundary forward jump then backward seek"
            );
        }
    }
}
