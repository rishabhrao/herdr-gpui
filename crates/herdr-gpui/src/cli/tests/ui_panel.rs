use super::*;
use crate::sidebar::ActivityPanel;
use control::{ControlCommand, Invocation, UiCommand};

fn parse(args: &[&str]) -> Result<Invocation, CliError> {
    LaunchOptions::parse(args.iter().copied()).map(|options| match options.mode {
        LaunchMode::Control(invocation) => invocation,
        mode => panic!("{args:?} parsed as {mode:?}"),
    })
}

#[test]
fn ui_panel_takes_every_panel_by_name() {
    for &panel in ActivityPanel::ALL {
        assert_eq!(
            parse(&["ui", "panel", panel.name(), "--json"]).unwrap(),
            Invocation {
                command: ControlCommand::Ui(UiCommand::Panel(panel)),
                json: true,
                window: None,
            }
        );
    }
}

#[test]
fn ui_panel_refuses_a_missing_or_unknown_panel() {
    let names = "workspaces|overview|pull-requests|issues|usage";
    assert_eq!(
        parse(&["ui", "panel", "dashboard"]).unwrap_err(),
        CliError::InvalidValue(names)
    );
    assert!(parse(&["ui", "panel"]).is_err());
}
