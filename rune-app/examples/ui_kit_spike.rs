//! UI-kit shell spike: official OpenCode theme, SplitView shell, Sidebar,
//! Toolbar, Card and the form controls the real UI will use.
//!
//! Run windowed:  cargo run -p scrcpy-gui --example ui_kit_spike
//! Headless:      cargo run -p scrcpy-gui --example ui_kit_spike -- --snap out.png

use rune::{AppSpec, Cx, El, ScrollSpec, col, row};
use rune_kit::glyphs;
use rune_kit::{
    Button, ButtonVariant, Card, ControlSize, Field, Kit, PaneState, SegmentedControl, Select,
    Sidebar, SidebarRow, SidebarSection, Slider, SplitView, Switch, Toolbar, root,
};

struct Spike {
    sidebar: PaneState,
    inspector: PaneState,
    tab: usize,
    device: usize,
    resolution: String,
    bitrate: f32,
    vsync: bool,
    playing: bool,
}

impl Default for Spike {
    fn default() -> Self {
        Self {
            sidebar: PaneState::new(280.0).range(220.0, 380.0),
            inspector: PaneState::new(300.0).range(240.0, 420.0),
            tab: 0,
            device: 0,
            resolution: "Original".into(),
            bitrate: 8.0,
            vsync: true,
            playing: false,
        }
    }
}

fn view(s: &Spike, cx: &mut Cx<Spike>) -> El {
    let k: &Kit = rune_theme_opencode::kit(cx.system_dark());
    let insets = cx.chrome_insets();
    let mut layer: Vec<El> = Vec::new();

    let sidebar = Sidebar::new()
        .header(
            SidebarRow::new("Devices")
                .icon(&glyphs::SPARKLE)
                .build(k)
                .key("devices-header"),
        )
        .section(
            SidebarSection::new("Connected").count(1).row(
                SidebarRow::new("PJA110")
                    .subtitle("706fc5a1 · USB")
                    .icon(&glyphs::SPARKLE)
                    .selected(s.device == 0)
                    .on_select(|s: &mut Spike, _| s.device = 0)
                    .build(k)
                    .key("device-0"),
            ),
        )
        .section(
            SidebarSection::new("Recent").row(
                SidebarRow::new("192.168.1.20:5555")
                    .subtitle("Wi-Fi")
                    .detail("2m")
                    .build(k)
                    .key("recent-0"),
            ),
        )
        .footer(
            SidebarRow::new("Settings")
                .icon(&glyphs::KEYBOARD)
                .build(k)
                .key("settings"),
        )
        .build(k);

    let mut form = row().w_full().gap(12.0);
    form = form.child(
        col().grow(1.0).min_w(0.0).child(
            Field::new()
                .label("Resolution")
                .control(
                    Select::new("resolution", &["Original", "1920", "1280"])
                        .current(Some(&s.resolution))
                        .label("Resolution")
                        .width(240.0)
                        .on_choose(|s: &mut Spike, v: &str, _| s.resolution = v.into())
                        .build(k, &mut layer),
                )
                .build(k),
        ),
    );
    form = form.child(
        col().grow(1.0).min_w(0.0).child(
            Field::new()
                .label("FPS")
                .control(
                    Select::new("fps", &["Auto", "30", "60", "120"])
                        .current(Some("Auto"))
                        .label("FPS")
                        .width(240.0)
                        .on_choose(|_: &mut Spike, _, _| {})
                        .build(k, &mut layer),
                )
                .build(k),
        ),
    );

    let main = col()
        .key("spike-main")
        .grow(1.0)
        .min_h(0.0)
        .gap(16.0)
        .padding(16.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            SegmentedControl::new(&["Screen", "Camera", "Desktop"], Some(s.tab))
                .label("Capture source")
                .width(None)
                .on_pick(|s: &mut Spike, i, _| s.tab = i)
                .build(k),
        )
        .child(
            Card::new()
                .title("Engine configuration", Some(&glyphs::SPARKLE))
                .child(form)
                .child(
                    Field::new()
                        .label("Bitrate")
                        .control(
                            Slider::new("bitrate", s.bitrate)
                                .range(1.0..=50.0)
                                .step(1.0)
                                .label("Bitrate")
                                .value_text(format!("{:.0} Mbps", s.bitrate))
                                .width(360.0)
                                .on_change(|s: &mut Spike, v, _| s.bitrate = v)
                                .build(k),
                        )
                        .build(k),
                )
                .build(k)
                .key("engine"),
        )
        .child(
            Button::new(if s.playing {
                "Stop session"
            } else {
                "Start session"
            })
            .variant(ButtonVariant::Contrast)
            .control_size(ControlSize::Large)
            .on_press(|s: &mut Spike| s.playing = !s.playing)
            .build(k),
        )
        .child(
            Card::new()
                .title("System console", Some(&glyphs::FILE_CODE))
                .child(
                    col()
                        .key("console-lines")
                        .gap(4.0)
                        .padding(12.0)
                        .fill(k.c.bg_base)
                        .radius(k.radius.md)
                        .child(
                            rune::text("12:34:56  [SYSTEM] ScrcpyGUI ready")
                                .font(k.ty.mono)
                                .color(k.c.text_muted),
                        )
                        .child(
                            rune::text("12:34:56  [SYSTEM] Device: PJA110")
                                .font(k.ty.mono)
                                .color(k.c.text_muted),
                        ),
                )
                .build(k)
                .key("console"),
        );

    let inspector = col()
        .key("spike-inspector")
        .grow(1.0)
        .min_h(0.0)
        .gap(16.0)
        .padding(16.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            Card::new()
                .title("Behavior", None)
                .child(
                    Switch::new(s.vsync)
                        .label("Vertical sync")
                        .on_toggle(|s: &mut Spike| s.vsync = !s.vsync)
                        .build(k),
                )
                .build(k)
                .key("behavior"),
        )
        .child(
            Card::new()
                .title("Shortcuts", Some(&glyphs::KEYBOARD))
                .child(rune_kit::Keybind::new(&["Alt", "F"]).build(k))
                .child(rune_kit::Keybind::new(&["Alt", "H"]).build(k))
                .build(k)
                .key("shortcuts"),
        );

    let toolbar = Toolbar::new().title("ScrcpyGUI").trailing(
        row().mr(insets.trailing).child(
            Button::new("Install core")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|_: &mut Spike| {})
                .build(k),
        ),
    );

    let shell = SplitView::new(main)
        .sidebar(&s.sidebar, |s: &mut Spike| &mut s.sidebar, sidebar)
        .inspector(&s.inspector, |s: &mut Spike| &mut s.inspector, inspector)
        .toolbar(toolbar)
        .titlebar_inset(cx.titlebar_inset())
        .build(k);

    root(shell, layer)
}

fn main() {
    let spec = AppSpec::<Spike> {
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        min_size: Some((1080.0, 760.0)),
        ..AppSpec::new(
            "ScrcpyGUI · UI kit spike",
            (1200.0, 890.0),
            Spike::default,
            view,
        )
    };
    rune::App::new(spec)
        .titlebar(rune::Titlebar::new(44.0))
        .run();
}
