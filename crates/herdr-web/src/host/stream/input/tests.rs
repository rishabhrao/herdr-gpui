#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn modifiers_use_the_devtools_bits() {
    let all = Mods {
        control: true,
        alt: true,
        shift: true,
        platform: true,
    };
    assert_eq!(modifiers(all), 15);
    assert_eq!(
        modifiers(Mods {
            shift: true,
            ..Mods::default()
        }),
        8
    );
}

#[test]
fn keys_that_act_carry_their_codes_and_typing_keys_their_text() {
    let press = |key: &str, text: Option<&str>| {
        commands(&StreamInput::Key {
            down: true,
            key: key.into(),
            code: String::new(),
            text: text.map(String::from),
            mods: Mods::default(),
        })
        .remove(0)
        .1
    };
    let enter = press("Enter", None);
    assert_eq!(enter["type"], "keyDown");
    assert_eq!(enter["text"], "\r");
    assert_eq!(enter["windowsVirtualKeyCode"], 13);
    let left = press("ArrowLeft", None);
    assert_eq!(left["type"], "rawKeyDown");
    assert!(left.get("text").is_none());
    let a = press("a", Some("a"));
    assert_eq!(
        (a["type"].as_str(), a["text"].as_str()),
        (Some("keyDown"), Some("a"))
    );
    assert_eq!(a["windowsVirtualKeyCode"], 65);
}

#[test]
fn pointer_and_text_map_to_their_commands() {
    let click = commands(&StreamInput::Mouse {
        kind: MouseKind::Down,
        x: 10.,
        y: 20.,
        button: Some(Button::Left),
        clicks: 1,
        mods: Mods::default(),
    });
    assert_eq!(click[0].1["type"], "mousePressed");
    assert_eq!(click[0].1["button"], "left");
    let text = commands(&StreamInput::Text("héllo".into()));
    assert_eq!(text[0], ("Input.insertText", json!({ "text": "héllo" })));
    assert_eq!(decode_picture("AQID"), Some(vec![1, 2, 3]));
    assert!(
        super::super::cdp::split_url("ws://127.0.0.1:9/devtools/browser/x").is_some_and(
            |(address, path)| address == "127.0.0.1:9" && path == "/devtools/browser/x"
        )
    );
}
