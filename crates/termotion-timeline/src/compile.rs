use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;
use smol_str::SmolStr;
use termotion_core::{
    Action, ClearEffect, Color, GridSpec, Overflow, Scenario, StyleId, StyleTable, TerminalState,
    TextStyle, Time,
};
use termotion_schema::diag::{codes, Diagnostic};
use unicode_segmentation::UnicodeSegmentation;

use crate::program::{DynRegion, Event, Op, Program, RegionKind};

/// One row of a `clear: terminal` wipe.
const CLEAR_ROW_STEP: Time = Time::from_nanos(16_000_000);

pub fn compile(scenario: &Scenario, grid: GridSpec) -> Result<Program, Diagnostic> {
    let mut styles = StyleTable::new();
    let base_style = TextStyle {
        fg: scenario.palette.foreground,
        ..TextStyle::default()
    };
    let initial_style = styles.intern(base_style);

    let mut ctx = Compiler {
        clock: Time::ZERO,
        events: Vec::new(),
        regions: Vec::new(),
        styles,
        initial_style,
        active_style: initial_style,
        active_text_style: base_style,
        rng: ChaCha8Rng::seed_from_u64(scenario.seed),
        grid,
    };

    for action in &scenario.timeline {
        ctx.action(action, scenario);
    }

    let mut duration = ctx.clock;
    for region in &ctx.regions {
        duration = duration.max(region.end);
    }
    if scenario.playback.looping {
        duration = duration.saturating_add(scenario.playback.loop_delay);
    }

    let program = Program {
        events: ctx.events,
        regions: ctx.regions,
        duration,
        grid,
        overflow: scenario.terminal.overflow,
        styles: ctx.styles,
        initial_style,
        cursor: scenario.cursor.clone(),
    };

    if scenario.terminal.overflow == Overflow::Error {
        check_no_overflow(&program)?;
    }

    Ok(program)
}

/// Replays the whole program in scroll mode; any scroll means the content did not
/// fit, which `overflow: error` treats as a validation failure.
fn check_no_overflow(program: &Program) -> Result<(), Diagnostic> {
    let mut state = TerminalState::new(program.grid.cols, program.grid.rows, Overflow::Scroll);
    state.styles = program.styles.clone();

    for event in &program.events {
        crate::program::apply_op(&mut state, &event.op);
        if state.scrolled > 0 {
            return Err(Diagnostic::error(
                codes::OVERFLOW_ERROR_MODE,
                format!(
                    "content overflows the {} row terminal and `overflow: error` is set",
                    program.grid.rows
                ),
            )
            .at_path("terminal.overflow")
            .with_hint(
                "Set `overflow: scroll` to scroll, `clip` to drop the excess, or enlarge the terminal region.",
            ));
        }
    }
    Ok(())
}

struct Compiler {
    clock: Time,
    events: Vec<Event>,
    regions: Vec<DynRegion>,
    styles: StyleTable,
    initial_style: StyleId,
    active_style: StyleId,
    active_text_style: TextStyle,
    rng: ChaCha8Rng,
    grid: GridSpec,
}

impl Compiler {
    fn emit(&mut self, op: Op) {
        self.events.push(Event { at: self.clock, op });
    }

    fn style_with_fg(&mut self, fg: Color) -> StyleId {
        let style = TextStyle {
            fg,
            ..self.active_text_style
        };
        self.styles.intern(style)
    }

    /// Types `text` one grapheme at a time, advancing the clock by `speed` each time.
    fn type_text(&mut self, text: &str, style: StyleId, speed: Time) {
        for grapheme in text.graphemes(true) {
            self.events.push(Event {
                at: self.clock,
                op: Op::PutGrapheme {
                    g: SmolStr::new(grapheme),
                    style,
                },
            });
            self.clock = self.clock.saturating_add(speed);
        }
    }

    fn write_spans(&mut self, spans: &[termotion_core::TextRun], speed: Time) {
        for run in spans {
            let style = match run.color {
                Some(color) => self.style_with_fg(color),
                None => self.active_style,
            };
            self.type_text(&run.text, style, speed);
        }
    }

    fn action(&mut self, action: &Action, scenario: &Scenario) {
        match action {
            Action::Write { spans, speed } => self.write_spans(spans, *speed),
            Action::WriteLine { spans, speed } => {
                self.write_spans(spans, *speed);
                self.emit(Op::Newline);
            }
            Action::Command {
                text,
                speed,
                enter_delay,
            } => {
                // The prompt appears at once; only the command is typed.
                let prompt_spans = scenario.prompt.spans(&scenario.palette);
                for (segment, style) in prompt_spans {
                    let id = self.styles.intern(style);
                    for grapheme in segment.graphemes(true) {
                        self.events.push(Event {
                            at: self.clock,
                            op: Op::PutGrapheme {
                                g: SmolStr::new(grapheme),
                                style: id,
                            },
                        });
                    }
                }
                let command_style = self.style_with_fg(scenario.palette.command);
                self.type_text(text, command_style, *speed);
                self.clock = self.clock.saturating_add(*enter_delay);
                self.emit(Op::Newline);
            }
            Action::Pause { duration } => {
                self.clock = self.clock.saturating_add(*duration);
            }
            Action::PauseRandom { min, max } => {
                let low = min.as_nanos();
                let high = max.as_nanos().max(low);
                let drawn = if low == high {
                    low
                } else {
                    self.rng.random_range(low..=high)
                };
                self.clock = self.clock.saturating_add(Time::from_nanos(drawn));
            }
            Action::Newline { count } => {
                for _ in 0..*count {
                    self.emit(Op::Newline);
                }
            }
            Action::Clear { effect } => match effect {
                ClearEffect::Instant => self.emit(Op::Clear),
                ClearEffect::Terminal => {
                    for row in 0..self.grid.rows {
                        self.emit(Op::ClearRow { row });
                        self.clock = self.clock.saturating_add(CLEAR_ROW_STEP);
                    }
                }
            },
            Action::Backspace { count, speed } => {
                for _ in 0..*count {
                    self.emit(Op::Backspace);
                    self.clock = self.clock.saturating_add(*speed);
                }
            }
            Action::Cursor {
                visible,
                blink,
                duration,
            } => {
                let blink = blink.or(scenario.cursor.blink);
                self.regions.push(DynRegion {
                    start: self.clock,
                    end: self.clock.saturating_add(*duration),
                    kind: RegionKind::Cursor {
                        visible: *visible,
                        blink,
                    },
                });
                self.clock = self.clock.saturating_add(*duration);
            }
            Action::SetColor {
                foreground,
                background,
            } => {
                let mut style = self.active_text_style;
                if let Some(fg) = foreground {
                    style.fg = *fg;
                }
                if let Some(bg) = background {
                    style.bg = Some(*bg);
                }
                self.active_text_style = style;
                self.active_style = self.styles.intern(style);
                self.emit(Op::SetStyle(self.active_style));
            }
            Action::ResetStyle => {
                self.active_style = self.initial_style;
                self.active_text_style = self.styles.get(self.initial_style);
                self.emit(Op::SetStyle(self.initial_style));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termotion_core::{Action, ClearEffect, GridSpec, Scenario, TextRun, Time};

    fn grid() -> GridSpec {
        GridSpec { cols: 40, rows: 10 }
    }

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

    fn run(text: &str) -> Vec<TextRun> {
        vec![TextRun {
            text: text.to_string(),
            color: None,
        }]
    }

    /// Renders the program as `<ms> <op>` lines — the same listing `inspect` prints.
    fn listing(program: &Program) -> String {
        program
            .events
            .iter()
            .map(|event| format!("{:>6} {}", event.at.as_millis(), event.op.describe()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn typing_emits_one_event_per_grapheme_at_the_configured_speed() {
        let program = compile(
            &scenario(vec![Action::Write {
                spans: run("zom"),
                speed: Time::from_millis(45),
            }]),
            grid(),
        )
        .unwrap();

        assert_eq!(
            listing(&program),
            "     0 type \"z\"\n    45 type \"o\"\n    90 type \"m\""
        );
        assert_eq!(program.duration, Time::from_millis(135));
    }

    #[test]
    fn zero_speed_types_the_whole_string_instantly() {
        let program = compile(
            &scenario(vec![Action::Write {
                spans: run("abc"),
                speed: Time::ZERO,
            }]),
            grid(),
        )
        .unwrap();
        assert!(program.events.iter().all(|e| e.at == Time::ZERO));
        assert_eq!(program.duration, Time::ZERO);
    }

    #[test]
    fn grapheme_clusters_are_never_split() {
        let program = compile(
            &scenario(vec![Action::Write {
                spans: run("e\u{0301}!"),
                speed: Time::from_millis(10),
            }]),
            grid(),
        )
        .unwrap();
        assert_eq!(program.events.len(), 2);
    }

    #[test]
    fn write_line_appends_a_newline_without_advancing_the_clock() {
        let program = compile(
            &scenario(vec![Action::WriteLine {
                spans: run("ab"),
                speed: Time::from_millis(10),
            }]),
            grid(),
        )
        .unwrap();

        assert_eq!(
            listing(&program),
            "     0 type \"a\"\n    10 type \"b\"\n    20 newline"
        );
        assert_eq!(program.duration, Time::from_millis(20));
    }

    #[test]
    fn pause_advances_the_clock_and_emits_nothing() {
        let program = compile(
            &scenario(vec![
                Action::Write {
                    spans: run("a"),
                    speed: Time::from_millis(10),
                },
                Action::Pause {
                    duration: Time::from_millis(500),
                },
                Action::Write {
                    spans: run("b"),
                    speed: Time::from_millis(10),
                },
            ]),
            grid(),
        )
        .unwrap();

        assert_eq!(listing(&program), "     0 type \"a\"\n   510 type \"b\"");
    }

    #[test]
    fn command_renders_the_prompt_instantly_then_types_then_presses_enter() {
        let mut scenario = scenario(vec![Action::Command {
            text: "./brb".to_string(),
            speed: Time::from_millis(45),
            enter_delay: Time::from_millis(250),
        }]);
        scenario.prompt.user = "zc".into();
        scenario.prompt.host = "tw".into();
        scenario.prompt.path = "~".into();
        scenario.prompt.symbol = "$".into();

        let program = compile(&scenario, grid()).unwrap();

        // Prompt "zc@tw:~$ " is 9 graphemes, all at t=0.
        let at_zero = program.events.iter().filter(|e| e.at == Time::ZERO).count();
        assert_eq!(at_zero, 10); // 9 prompt cells + the first typed char

        let last = program.events.last().unwrap();
        assert_eq!(last.op.describe(), "newline");
        // 5 chars typed => clock 225ms, + 250ms enter delay
        assert_eq!(last.at, Time::from_millis(475));
        assert_eq!(program.duration, Time::from_millis(475));
    }

    #[test]
    fn newline_count_emits_that_many_newlines_instantly() {
        let program = compile(&scenario(vec![Action::Newline { count: 3 }]), grid()).unwrap();
        assert_eq!(program.events.len(), 3);
        assert!(program.events.iter().all(|e| e.at == Time::ZERO));
    }

    #[test]
    fn backspace_repeats_at_the_configured_speed() {
        let program = compile(
            &scenario(vec![Action::Backspace {
                count: 3,
                speed: Time::from_millis(40),
            }]),
            grid(),
        )
        .unwrap();

        assert_eq!(
            listing(&program),
            "     0 backspace\n    40 backspace\n    80 backspace"
        );
        assert_eq!(program.duration, Time::from_millis(120));
    }

    #[test]
    fn instant_clear_is_one_event() {
        let program = compile(
            &scenario(vec![Action::Clear {
                effect: ClearEffect::Instant,
            }]),
            grid(),
        )
        .unwrap();
        assert_eq!(program.events.len(), 1);
        assert_eq!(program.events[0].op.describe(), "clear");
    }

    #[test]
    fn terminal_clear_wipes_one_row_at_a_time() {
        let program = compile(
            &scenario(vec![Action::Clear {
                effect: ClearEffect::Terminal,
            }]),
            grid(),
        )
        .unwrap();
        assert_eq!(program.events.len(), 10); // one per row
        assert_eq!(program.events[1].at, Time::from_millis(16));
        assert_eq!(program.duration, Time::from_millis(160));
    }

    #[test]
    fn set_color_and_reset_style_change_the_active_style() {
        use termotion_core::Color;
        let program = compile(
            &scenario(vec![
                Action::SetColor {
                    foreground: Some(Color::rgb(255, 0, 0)),
                    background: None,
                },
                Action::Write {
                    spans: run("x"),
                    speed: Time::ZERO,
                },
                Action::ResetStyle,
                Action::Write {
                    spans: run("y"),
                    speed: Time::ZERO,
                },
            ]),
            grid(),
        )
        .unwrap();

        let styles: Vec<_> = program
            .events
            .iter()
            .filter_map(|e| match &e.op {
                Op::PutGrapheme { style, .. } => Some(*style),
                _ => None,
            })
            .collect();
        assert_ne!(styles[0], styles[1]);
        assert_eq!(program.styles.get(styles[0]).fg, Color::rgb(255, 0, 0));
    }

    #[test]
    fn spans_carry_their_own_colors_without_disturbing_the_active_style() {
        use termotion_core::Color;
        let program = compile(
            &scenario(vec![Action::Write {
                spans: vec![
                    TextRun {
                        text: "a".into(),
                        color: Some(Color::rgb(1, 2, 3)),
                    },
                    TextRun {
                        text: "b".into(),
                        color: None,
                    },
                ],
                speed: Time::ZERO,
            }]),
            grid(),
        )
        .unwrap();

        let styles: Vec<_> = program
            .events
            .iter()
            .filter_map(|e| match &e.op {
                Op::PutGrapheme { style, .. } => Some(*style),
                _ => None,
            })
            .collect();
        assert_eq!(program.styles.get(styles[0]).fg, Color::rgb(1, 2, 3));
        assert_eq!(styles[1], program.initial_style);
    }

    #[test]
    fn pause_random_is_reproducible_for_a_given_seed() {
        let actions = vec![Action::PauseRandom {
            min: Time::from_millis(300),
            max: Time::from_millis(600),
        }];
        let mut a = scenario(actions.clone());
        a.seed = 12345;
        let mut b = scenario(actions);
        b.seed = 12345;

        let first = compile(&a, grid()).unwrap().duration;
        let second = compile(&b, grid()).unwrap().duration;
        assert_eq!(first, second);
        assert!(first >= Time::from_millis(300) && first <= Time::from_millis(600));
    }

    #[test]
    fn different_seeds_produce_different_random_pauses() {
        let actions = vec![Action::PauseRandom {
            min: Time::from_millis(300),
            max: Time::from_millis(600),
        }];
        let mut a = scenario(actions.clone());
        a.seed = 1;
        let mut b = scenario(actions);
        b.seed = 2;
        assert_ne!(
            compile(&a, grid()).unwrap().duration,
            compile(&b, grid()).unwrap().duration
        );
    }

    #[test]
    fn loop_delay_extends_the_duration_only_when_looping() {
        let mut looping = scenario(vec![Action::Pause {
            duration: Time::from_millis(100),
        }]);
        looping.playback.looping = true;
        looping.playback.loop_delay = Time::from_millis(1_000);
        assert_eq!(
            compile(&looping, grid()).unwrap().duration,
            Time::from_millis(1_100)
        );

        let mut once = looping.clone();
        once.playback.looping = false;
        assert_eq!(
            compile(&once, grid()).unwrap().duration,
            Time::from_millis(100)
        );
    }

    #[test]
    fn overflow_error_mode_rejects_a_scenario_that_would_scroll() {
        let mut scenario = scenario(vec![Action::Newline { count: 20 }]);
        scenario.terminal.overflow = termotion_core::Overflow::Error;
        let err = compile(&scenario, GridSpec { cols: 40, rows: 3 }).unwrap_err();
        assert_eq!(err.code, termotion_schema::diag::codes::OVERFLOW_ERROR_MODE);
    }

    #[test]
    fn overflow_scroll_mode_accepts_the_same_scenario() {
        let mut scenario = scenario(vec![Action::Newline { count: 20 }]);
        scenario.terminal.overflow = termotion_core::Overflow::Scroll;
        assert!(compile(&scenario, GridSpec { cols: 40, rows: 3 }).is_ok());
    }
}
