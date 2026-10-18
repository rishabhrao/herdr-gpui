use super::*;

#[test]
fn the_activity_bar_is_on_unless_turned_off() -> anyhow::Result<()> {
    for config in [Config::default(), Config::parse(DEFAULT_CONFIG)?] {
        assert!(config.sidebar_style.activity_bar);
    }
    let off = Config::parse("[sidebar]\nactivity_bar = false")?;
    assert!(!off.sidebar_style.activity_bar);
    assert!(Config::parse("[sidebar]\nactivity_bar = \"off\"").is_err());
    Ok(())
}
