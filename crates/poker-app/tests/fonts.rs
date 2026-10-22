//! Font discovery checks. These assert the current environment can provide CJK glyphs,
//! while remaining tolerant on machines without the fonts installed.

#[test]
fn system_font_candidate_list_is_not_empty_on_desktop_os() {
    if cfg!(any(target_os = "linux", target_os = "windows")) {
        assert!(
            !poker_app::fonts::SYSTEM_CJK_FONTS.is_empty(),
            "expected known CJK font candidates for this OS"
        );
    }
}

#[test]
fn explicit_system_font_loads_when_available() {
    match poker_app::fonts::load_system_font() {
        Some(font) => {
            assert!(!font.bytes.is_empty(), "loaded font has no bytes");
            assert!(
                font.bytes.len() > 100_000,
                "font file suspiciously small: {} bytes",
                font.bytes.len()
            );
            assert!(
                !font.label.is_empty(),
                "loaded font should have a human readable label"
            );
        }
        None => {
            eprintln!("no system CJK font found; skipping explicit font assertion");
        }
    }
}

#[test]
fn bundled_font_path_points_into_assets() {
    let path = poker_app::fonts::bundled_font_path();
    assert!(
        path.to_string_lossy().contains("assets"),
        "bundled font should live under assets/: {}",
        path.display()
    );
}
