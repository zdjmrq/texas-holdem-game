//! Embedded asset tests: CJK font, window icon and music discovery.

#[test]
fn bundled_font_is_a_large_opentype_file() {
    let bytes = poker_app::assets::BUNDLED_FONT_OTF;
    assert!(bytes.len() > 1_000_000, "bundled font looks too small");
    assert_eq!(&bytes[0..4], b"OTTO", "expected an OpenType CFF font");
}

#[test]
fn window_icon_decodes_from_embedded_png() {
    let icon = poker_app::window_icon().expect("window icon should decode");
    assert_eq!(icon.width, 32);
    assert_eq!(icon.height, 32);
    assert_eq!(icon.rgba.len(), 32 * 32 * 4);
}

#[test]
fn music_assets_are_discoverable() {
    let tracks = poker_app::assets::music_tracks();
    assert!(
        !tracks.is_empty(),
        "expected mp3 files under assets/music (run from the workspace)"
    );
    assert!(tracks.iter().all(|path| path.exists()));
}

#[test]
fn config_dir_uses_override_environment() {
    if std::env::var_os("POKER_APP_CONFIG_DIR").is_none() {
        let dir = poker_app::assets::config_dir();
        assert!(dir.ends_with("texas-holdem-rs"));
    }
}
