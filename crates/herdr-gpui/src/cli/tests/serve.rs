use super::*;

fn serve(args: &[&str]) -> Result<Option<ServeOptions>, CliError> {
    LaunchOptions::parse(args.iter().copied()).map(|options| options.serve)
}

fn port(args: &[&str]) -> Result<Option<u16>, CliError> {
    serve(args).map(|options| options.map(|options| options.port))
}

#[test]
fn serve_defaults_its_port_and_takes_another() {
    assert_eq!(port(&["serve"]).unwrap(), Some(DEFAULT_SERVE_PORT));
    assert_eq!(port(&["serve", "--port", "8080"]).unwrap(), Some(8080));
    assert_eq!(serve(&[]).unwrap(), None);
}

#[test]
fn serve_keeps_the_connection_options() {
    let options = LaunchOptions::parse(["serve", "--session", "work", "--port", "4000"]).unwrap();
    assert_eq!(options.mode, LaunchMode::Normal);
    assert_eq!(options.serve.map(|serve| serve.port), Some(4000));
    assert!(matches!(options.target, ConnectTarget::Session { ref name, .. } if name == "work"));
}

#[test]
fn bad_ports_are_refused_before_the_gui_starts() {
    assert_eq!(
        serve(&["serve", "--port"]),
        Err(CliError::MissingValue("--port"))
    );
    for bad in ["0", "70000", "http"] {
        assert_eq!(
            serve(&["serve", "--port", bad]),
            Err(CliError::InvalidValue("--port"))
        );
    }
    assert_eq!(
        serve(&["serve", "--port", "1", "--port", "2"]),
        Err(CliError::DuplicateOption("--port"))
    );
    // Only `serve` takes a port.
    assert!(matches!(
        serve(&["--port", "80"]),
        Err(CliError::UnknownOption(_))
    ));
    // `serve` must come first, like the command families.
    assert!(matches!(
        serve(&["--dev", "serve"]),
        Err(CliError::UnknownOption(_))
    ));
}

#[test]
fn serve_takes_its_public_hosts_and_access_check() {
    let options = serve(&[
        "serve",
        "--public-host",
        "herdr.example.com",
        "--public-host",
        "other.example.com",
        "--access-team",
        "myteam",
        "--access-aud",
        "tag",
    ])
    .unwrap()
    .unwrap();
    assert_eq!(
        options.public_hosts,
        ["herdr.example.com", "other.example.com"]
    );
    assert_eq!(options.access_team.as_deref(), Some("myteam"));
    assert_eq!(options.access_audience.as_deref(), Some("tag"));
    assert!(!options.no_access_check);
    assert!(
        serve(&["serve", "--no-access-check"])
            .unwrap()
            .unwrap()
            .no_access_check
    );
    assert_eq!(
        serve(&["serve", "--public-host"]),
        Err(CliError::MissingValue("--public-host"))
    );
    assert_eq!(
        serve(&["serve", "--access-team", "a", "--access-team", "b"]),
        Err(CliError::DuplicateOption("--access-team"))
    );
    // Only serve takes them.
    assert!(matches!(
        serve(&["--public-host", "x"]),
        Err(CliError::UnknownOption(_))
    ));
}

#[test]
fn serve_has_device_commands_that_need_no_app() {
    assert_eq!(
        serve(&["serve", "devices"]).unwrap().unwrap().action,
        ServeAction::Devices
    );
    assert_eq!(
        serve(&["serve", "forget", "Old phone"])
            .unwrap()
            .unwrap()
            .action,
        ServeAction::Forget("Old phone".into())
    );
    assert_eq!(
        serve(&["serve", "forget"]).unwrap().unwrap().action,
        ServeAction::Forget(String::new())
    );
}
