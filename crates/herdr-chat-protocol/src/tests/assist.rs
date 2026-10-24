use super::*;
use crate::assist::{History, completions, next_mode, step};

fn command(name: &str) -> SlashCommand {
    SlashCommand {
        description: String::new(),
        hint: None,
        name: name.to_owned(),
    }
}

fn select(id: &str, category: OptionCategory, choices: &[&str], current: &str) -> ConfigOption {
    ConfigOption {
        category,
        description: None,
        id: id.to_owned(),
        kind: OptionKind::Select {
            choices: choices
                .iter()
                .map(|choice| Choice {
                    description: None,
                    group: None,
                    id: (*choice).to_owned(),
                    name: choice.to_uppercase(),
                })
                .collect(),
            current: current.to_owned(),
        },
        name: id.to_owned(),
    }
}

#[test]
fn slash_commands_complete_a_name_being_typed() {
    let mut transcript = Transcript::new("Claude");
    transcript.commands = vec![command("review"), command("reset"), command("init")];
    let names = |text| {
        completions(&transcript, text)
            .into_iter()
            .map(|command| command.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names("/re"), ["review", "reset"]);
    assert_eq!(names("/"), ["review", "reset", "init"]);
    assert!(names("/review").is_empty(), "already complete");
    assert!(names("/review this").is_empty());
    assert!(names("re").is_empty());
}

#[test]
fn history_steps_back_and_forth() {
    let mut history = History::default();
    assert_eq!(history.older(), None);
    history.push("one");
    history.push("two");
    history.push("two");
    assert_eq!(history.older(), Some("two"));
    assert_eq!(history.older(), Some("one"));
    assert_eq!(history.older(), Some("one"));
    assert_eq!(history.newer(), Some("two"));
    assert_eq!(history.newer(), Some(""));
    assert!(!history.recalling());
}

#[test]
fn settings_step_through_choices_and_wrap() {
    let model = select("model", OptionCategory::Model, &["a", "b", "c"], "c");
    assert_eq!(
        step(&model, 1),
        Some(Command::SetOption {
            id: "model".into(),
            value: OptionValue::Choice { id: "a".into() }
        })
    );
    assert_eq!(
        step(&model, -1),
        Some(Command::SetOption {
            id: "model".into(),
            value: OptionValue::Choice { id: "b".into() }
        })
    );
    let alone = select("model", OptionCategory::Model, &["a"], "a");
    assert_eq!(step(&alone, 1), None);
    let toggle = ConfigOption {
        category: OptionCategory::Other,
        description: None,
        id: "fast".into(),
        kind: OptionKind::Toggle { current: true },
        name: "Fast".into(),
    };
    assert_eq!(
        step(&toggle, 1),
        Some(Command::SetOption {
            id: "fast".into(),
            value: OptionValue::Toggle { value: false }
        })
    );
}

#[test]
fn the_mode_setting_wins_over_the_older_modes() {
    let mut transcript = Transcript::new("Claude");
    transcript.modes = Some(Modes {
        available: vec![
            Mode {
                description: None,
                id: "default".into(),
                name: "Default".into(),
            },
            Mode {
                description: None,
                id: "plan".into(),
                name: "Plan".into(),
            },
        ],
        current: "default".into(),
    });
    assert_eq!(
        next_mode(&transcript),
        Some(Command::SetMode { id: "plan".into() })
    );
    transcript.options = vec![select(
        "mode",
        OptionCategory::Mode,
        &["ask", "auto"],
        "ask",
    )];
    assert_eq!(
        next_mode(&transcript),
        Some(Command::SetOption {
            id: "mode".into(),
            value: OptionValue::Choice { id: "auto".into() }
        })
    );
}
