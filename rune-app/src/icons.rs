//! Small 16x16 SVG icons used where the kit glyph set has no match.
//! Rune rasterises these into its glyph atlas, so no external assets.

use rune::Icon;

macro_rules! svg_icon {
    ($name:ident, $body:expr) => {
        #[allow(dead_code)] // catalog icon, not all are drawn by every app
        pub static $name: Icon = Icon {
            name: stringify!($name),
            view: 16.0,
            body: $body,
        };
    };
}

svg_icon!(
    MONITOR,
    r#"<rect x="1.5" y="2.5" width="13" height="9" rx="1" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M6 14h4M8 11.5V14" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>"#
);
svg_icon!(
    CAMERA,
    r#"<rect x="1.5" y="4.5" width="13" height="8" rx="2" stroke="currentColor" stroke-width="1.5" fill="none"/><circle cx="8" cy="8.5" r="2" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M5.5 4.5 6.5 2.5h3l1 2" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" fill="none"/>"#
);
svg_icon!(
    GRID,
    r#"<rect x="2" y="2" width="5" height="5" rx="1" stroke="currentColor" stroke-width="1.5" fill="none"/><rect x="9" y="2" width="5" height="5" rx="1" stroke="currentColor" stroke-width="1.5" fill="none"/><rect x="2" y="9" width="5" height="5" rx="1" stroke="currentColor" stroke-width="1.5" fill="none"/><rect x="9" y="9" width="5" height="5" rx="1" stroke="currentColor" stroke-width="1.5" fill="none"/>"#
);
svg_icon!(
    FULLSCREEN,
    r#"<path d="M2 6V2h4M10 2h4v4M14 10v4h-4M6 14H2v-4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
);
svg_icon!(
    HOME,
    r#"<path d="M2 7.5 8 2.5l6 5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M4 7v6.5h8V7" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" fill="none"/>"#
);
svg_icon!(
    BACK,
    r#"<path d="M6.5 3.5 2.5 8l4 4.5M2.5 8h8a3 3 0 0 1 0 6H8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
);
svg_icon!(
    RECENTS,
    r#"<circle cx="8" cy="8" r="5.5" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M8 5v3.2l2.2 1.6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
);
svg_icon!(
    POWER,
    r#"<path d="M8 2v5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" fill="none"/><path d="M4.6 4.4a5 5 0 1 0 6.8 0" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" fill="none"/>"#
);
svg_icon!(
    ROTATE,
    r#"<path d="M13 8A5 5 0 1 1 10.6 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" fill="none"/><path d="M10.5 1.8 11 4.5 8.4 4.7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
);
svg_icon!(
    PASTE,
    r#"<rect x="3.5" y="4.5" width="9" height="9" rx="1" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M6 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5M6 8h4M6 10.5h4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" fill="none"/>"#
);
svg_icon!(
    PHONE,
    r#"<rect x="4" y="1.5" width="8" height="13" rx="1.5" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M7 2.5h2M7.5 13h1" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>"#
);
svg_icon!(
    GEAR,
    r#"<circle cx="8" cy="8" r="4.8" stroke="currentColor" stroke-width="1.4" fill="none"/><circle cx="8" cy="8" r="1.9" stroke="currentColor" stroke-width="1.4" fill="none"/><path d="M8 1.2v2M8 12.8v2M1.2 8h2M12.8 8h2M3.2 3.2l1.4 1.4M11.4 11.4l1.4 1.4M12.8 3.2l-1.4 1.4M4.6 11.4l-1.4 1.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" fill="none"/>"#
);
svg_icon!(
    OFF,
    r#"<rect x="1.5" y="2.5" width="13" height="9" rx="1" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M3 13 13 3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" fill="none"/>"#
);
svg_icon!(
    SLIDERS,
    r#"<path d="M2 4.5h4M9 4.5h5M2 11.5h7M12 11.5h2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" fill="none"/><circle cx="7" cy="4.5" r="1.5" stroke="currentColor" stroke-width="1.5" fill="none"/><circle cx="10.5" cy="11.5" r="1.5" stroke="currentColor" stroke-width="1.5" fill="none"/>"#
);
svg_icon!(
    PIN,
    r#"<path d="M5 2h6M6.4 2v2.3c0 .6-.3 1.2-.8 1.6l-.9.7c-.5.4-.3 1.2.3 1.2h6c.6 0 .8-.8.3-1.2l-.9-.7c-.5-.4-.8-1-.8-1.6V2" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" fill="none"/><path d="M8 8.1v5.6" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#
);

/// Full brand glyph — the same "phone → screen" arrow used by the tray, the
/// executable resource and the README. 512-unit viewBox, scaled by the caller.
pub static BRAND: Icon = Icon {
    name: "brand",
    view: 512.0,
    body: r##"<rect width="512" height="512" rx="112" fill="#0D1117"/><g stroke="currentColor" stroke-width="32" stroke-linecap="round" stroke-linejoin="round" fill="none"><rect x="108" y="168" width="88" height="176" rx="22"/><rect x="292" y="212" width="112" height="88" rx="16"/><path d="M348 300v40"/><path d="M312 340h72"/><path d="M196 256h64"/><path d="M238 230l38 26-38 26"/></g>"##,
};
