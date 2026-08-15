use termotion_core::{Action, Fps, GridSpec, Scenario, TextRun, Time};
use termotion_timeline::{compile, Runtime};

fn scenario() -> Scenario {
    Scenario {
        metadata: Default::default(),
        canvas: Default::default(),
        terminal: Default::default(),
        font: Default::default(),
        prompt: Default::default(),
        cursor: Default::default(),
        palette: Default::default(),
        playback: Default::default(),
        seed: 42,
        timeline: vec![
            Action::Command {
                text: "./brb".into(),
                speed: Time::from_millis(45),
                enter_delay: Time::from_millis(250),
            },
            Action::Pause {
                duration: Time::from_millis(500),
            },
            Action::WriteLine {
                spans: vec![TextRun {
                    text: "Session suspended.".into(),
                    color: None,
                }],
                speed: Time::from_millis(35),
            },
            Action::PauseRandom {
                min: Time::from_millis(300),
                max: Time::from_millis(600),
            },
            Action::Cursor {
                visible: true,
                blink: Some(Time::from_millis(500)),
                duration: Time::from_millis(3_000),
            },
        ],
    }
}

fn dump(order: &[u64]) -> Vec<String> {
    let program = compile(&scenario(), GridSpec { cols: 70, rows: 15 }).unwrap();
    let mut runtime = Runtime::new(program);
    let mut results = vec![String::new(); order.len()];

    for (slot, frame) in order.iter().enumerate() {
        let t = Fps::from_integer(30).frame_time(*frame);
        let state = runtime.state_at(t);
        let text: String = state
            .grid
            .iter_rows()
            .map(|row| row.cells.iter().map(|c| c.g.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        results[slot] = format!("{}|{}", text, state.cursor.visible);
    }
    results
}

#[test]
fn frame_state_is_independent_of_access_order() {
    let frames: Vec<u64> = (0..90).collect();
    let sequential = dump(&frames);

    let mut shuffled = frames.clone();
    shuffled.reverse();
    let reversed = dump(&shuffled);

    for (i, frame) in shuffled.iter().enumerate() {
        let expected = &sequential[*frame as usize];
        assert_eq!(
            &reversed[i], expected,
            "frame {frame} differs by access order"
        );
    }
}

#[test]
fn two_compiles_of_the_same_scenario_are_identical() {
    let a = compile(&scenario(), GridSpec { cols: 70, rows: 15 }).unwrap();
    let b = compile(&scenario(), GridSpec { cols: 70, rows: 15 }).unwrap();
    assert_eq!(a.events, b.events);
    assert_eq!(a.duration, b.duration);
}
