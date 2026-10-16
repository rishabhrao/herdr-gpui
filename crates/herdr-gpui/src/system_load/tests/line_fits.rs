#[test]
fn memory_goes_under_cpu_only_when_both_do_not_fit() {
    let glyph = 7.;
    // CPU with its sparkline and MEM with its meter side by side.
    assert!(crate::system_load::line_fits(400., glyph));
    // A sidebar dragged narrow: memory takes a line of its own.
    assert!(!crate::system_load::line_fits(150., glyph));
    // Bigger text needs more room for the same parts.
    let fits_at = (100..600)
        .map(|width| width as f32)
        .find(|width| crate::system_load::line_fits(*width, glyph))
        .unwrap();
    assert!(!crate::system_load::line_fits(fits_at, glyph * 2.));
}
