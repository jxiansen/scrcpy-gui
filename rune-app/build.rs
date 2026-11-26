fn main() {
    // Windows takes the taskbar/titlebar icon from the executable resource;
    // Rune's AppSpec has no icon field. Embed the same glyph used by the tray
    // (tray-icon.rgba) and README (icon.svg) so all three stay in sync.
    if cfg!(target_os = "windows") {
        match embed_resource::compile("icons/app.rc", embed_resource::NONE) {
            embed_resource::CompilationResult::Ok => {}
            other => panic!("failed to embed the Windows app icon: {other:?}"),
        }
    }
}
