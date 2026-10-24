#![allow(clippy::unwrap_used)]
use super::*;
use core::prelude::v1::test;
use herdr_chat_protocol::{Choice, ConfigOption};

fn option(id: &str, category: OptionCategory) -> ConfigOption {
    ConfigOption {
        category,
        description: None,
        id: id.into(),
        kind: OptionKind::Select {
            choices: vec![Choice {
                description: None,
                group: None,
                id: "a".into(),
                name: "A".into(),
            }],
            current: "a".into(),
        },
        name: id.into(),
    }
}

#[test]
fn only_settings_without_a_button_go_behind_the_more_button() {
    let mut transcript = Transcript::new("Claude");
    transcript.options = vec![
        option("model", OptionCategory::Model),
        option("effort", OptionCategory::ThoughtLevel),
        option("variant", OptionCategory::ModelConfig),
        option("second-model", OptionCategory::Model),
        option("allow_all", OptionCategory::Other),
    ];
    let ids: Vec<_> = others(&transcript)
        .into_iter()
        .map(|option| option.id.as_str())
        .collect();
    assert_eq!(ids, ["variant", "second-model", "allow_all"]);
}
