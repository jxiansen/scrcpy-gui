//! The native UI: a close port of the React layout to Rune elements.

use rune::tok::{self, type_};
use rune::{Cx, El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Select, Switch, TextInput};

use crate::i18n::t;
use crate::ui::state::{SessionKind, Ui};

const FPS: &[&str] = &["Auto", "30", "60", "90", "120"];
const RES: &[&str] = &["0", "3840", "2560", "1920", "1600", "1280", "1024", "800"];
const ROT: &[&str] = &["0", "270", "90", "180"];
const CODEC: &[&str] = &["h264", "h265", "av1", "vp8", "vp9"];
const RENDERERS: &[&str] = &["Auto", "swiftshader", "native"];
const HWDEC: &[&str] = &["auto", "disabled", "d3d11va", "vaapi", "videotoolbox"];
const ASPECT: &[&str] = &["0", "16:9", "4:3"];
const AUDIO_CODEC: &[&str] = &["auto", "opus", "aac", "flac", "raw"];
const LENSES: &[&str] = &["Auto Select", "Back", "Front", "External"];
const PRESETS: &[&str] = &["Custom", "1080p", "1440p", "4K", "21:9"];
const LOCALES: &[&str] = &["en", "zh-CN", "zh-TW", "fr", "pt-BR", "ru", "id", "ar"];

pub fn view(s: &Ui, _cx: &mut Cx<Ui>) -> El {
    let k = Kit::neutral(s.dark);
    let mut layer: Vec<El> = Vec::new();
    let tr = |key: &str| t(&s.locale, key);

    let body = col()
        .key("window")
        .w_full()
        .h_full()
        .fill(k.c.page)
        .child(header(s, &k, &tr))
        .child(
            row()
                .key("workspace")
                .grow(1.0)
                .min_h(0.0)
                .gap(10.0)
                .px(12.0)
                .py(10.0)
                .child(sidebar(s, &k, &tr))
                .child(center(s, &k, &mut layer, &tr))
                .child(right(s, &k, &mut layer, &tr)),
        )
        .child(footer(&k, &tr));

    if s.onboarding_open && s.scrcpy_ok != Some(true) {
        layer.push(onboarding(s, &k, &tr));
    }
    if let Some(message) = &s.alert {
        layer.push(alert(&k, message));
    }
    rune_kit::root(body, layer)
}

fn header<F: Fn(&str) -> &'static str>(s: &Ui, k: &Kit, tr: &F) -> El {
    let dot = match s.scrcpy_ok {
        Some(true) => tok::accent(3, s.dark),
        Some(false) => k.c.danger.fg,
        None => k.c.warning.fg,
    };
    let status = if s.download.active {
        format!("{} {}%", tr("header.downloading"), s.download.percent)
    } else {
        match s.scrcpy_ok {
            Some(true) => tr("header.ready").to_string(),
            Some(false) => {
                if s.scrcpy_message.is_empty() {
                    tr("header.notFound").to_string()
                } else {
                    s.scrcpy_message.clone()
                }
            }
            None => tr("header.checking").to_string(),
        }
    };

    let mut themes = row().key("themes").gap(6.0).center_y();
    for i in 0..5 {
        themes = themes.child(
            el()
                .key(format!("theme-{i}"))
                .size(15.0, 15.0)
                .radius(8.0)
                .fill(tok::accent(i, s.dark))
                .border(if i == s.accent { 2.0 } else { 0.0 }, k.c.text_base)
                .on_press(move |s: &mut Ui| s.accent = i),
        );
    }

    let locale = Select::new("locale", LOCALES)
        .width(90.0)
        .current(Some(&s.locale))
        .label(tr("header.language"))
        .open(s.open_select.as_deref() == Some("locale"))
        .on_toggle(|s: &mut Ui| toggle_select(s, "locale"))
        .on_close(|s: &mut Ui| s.open_select = None)
        .on_choose(|s: &mut Ui, v: &str, _| s.set_locale(v))
        .build(k, &mut Vec::new());

    let engine = row()
        .key("engine")
        .gap(8.0)
        .center_y()
        .child(el().key("engine-dot").size(8.0, 8.0).radius(4.0).fill(dot))
        .child(text(status).font(type_::LABEL).color(k.c.text_base))
        .child(
            Button::new(tr("header.install"))
                .variant(ButtonVariant::Contrast)
                .disabled(s.scrcpy_ok == Some(true) || s.download.active)
                .on_press(|s: &mut Ui| s.download_scrcpy())
                .build(k),
        );

    row()
        .key("header")
        .h(62.0)
        .px(14.0)
        .gap(12.0)
        .center_y()
        .fill(k.c.bg_base)
        .border(1.0, k.c.border_muted)
        .child(row().key("header-left").grow(1.0).gap(8.0).center_y()
            .child(text(tr("header.theme")).font(type_::LABEL).color(k.c.text_muted))
            .child(themes)
            .child(Button::new(if s.dark { tr("header.dark") } else { tr("header.light") })
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut Ui| s.set_dark(!s.dark)).build(k))
            .child(locale))
        .child(row().key("header-center").gap(8.0).center_y()
            .child(text("scrcpy ").font(type_::TITLE).color(k.c.text_base))
            .child(text("GUI").font(type_::TITLE).color(tok::accent(s.accent, s.dark))))
        .child(row().key("header-right").grow(1.0).gap(8.0).center().child(engine))
}

fn sidebar<F: Fn(&str) -> &'static str>(s: &Ui, k: &Kit, tr: &F) -> El {
    let mut devices = col().key("devices").gap(4.0);
    if s.devices.is_empty() {
        devices = devices.child(text(tr("sidebar.noDevices")).font(type_::BODY).color(k.c.text_muted));
    }
    for d in &s.devices {
        let serial = d.serial.clone();
        let selected = s.config.device == d.serial;
        let running = s.running.get(&d.serial).copied().unwrap_or(false);
        let wifi = d.serial.contains('.');
        devices = devices.child(
            row()
                .key(format!("device-{}", d.serial))
                .h(38.0)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .radius(tok::radius::SM)
                .fill(if selected { k.c.layer_02 } else { k.c.layer_01 })
                .on_press(move |s: &mut Ui| s.select_device(serial.clone()))
                .child(el().key("tile").size(8.0, 8.0).radius(4.0)
                    .fill(if running { tok::accent(3, s.dark) } else { k.c.text_faint }))
                .child(col().gap(1.0).grow(1.0)
                    .child(text(&d.model).font(type_::BODY).color(k.c.text_base))
                    .child(text(&d.serial).font(type_::MONO_CAPTION).color(k.c.text_muted)))
                .child(text(if running { tr("sidebar.live") } else { tr("sidebar.ready") })
                    .font(type_::LABEL).color(k.c.text_muted))
                .child(text(if wifi { tr("sidebar.wifi") } else { tr("sidebar.usb") })
                    .font(type_::LABEL).color(k.c.text_muted)),
        );
    }

    let tabs = row().key("transport").gap(4.0).child(
        Button::new(tr("sidebar.usb")).variant(ButtonVariant::Ghost).build(k),
    ).child(
        Button::new(tr("sidebar.wifi")).variant(ButtonVariant::Ghost).build(k),
    );

    let ip = TextInput::new(&s.ip)
        .label("IP")
        .placeholder(tr("sidebar.ipPlaceholder"))
        .width(Some(176.0))
        .on_edit(|s: &mut Ui| &mut s.ip)
        .build(k);
    let pair_ip = TextInput::new(&s.pair_ip)
        .label("Pair IP")
        .placeholder(tr("sidebar.ipPlaceholder"))
        .width(Some(112.0))
        .on_edit(|s: &mut Ui| &mut s.pair_ip)
        .build(k);
    let pair_code = TextInput::new(&s.pair_code)
        .label(tr("sidebar.pairCode"))
        .placeholder("123456")
        .width(Some(112.0))
        .on_edit(|s: &mut Ui| &mut s.pair_code)
        .build(k);

    let mut mdns = col().key("mdns").gap(2.0);
    for service in &s.mdns {
        if !service.service.contains("_adb-tls") { continue; }
        let addr = service.address.clone();
        mdns = mdns.child(
            row().key(format!("mdns-{}", service.name)).h(30.0).px(8.0).center_y()
                .radius(tok::radius::SM).fill(k.c.layer_01)
                .on_press(move |s: &mut Ui| s.ip.replace_text(&addr))
                .child(text(&service.name).font(type_::BODY).color(k.c.text_base).grow(1.0).truncate())
                .child(text(&service.address).font(type_::MONO_CAPTION).color(k.c.text_muted)),
        );
    }

    col()
        .key("sidebar")
        .w(300.0)
        .min_h(0.0)
        .gap(10.0)
        .padding(12.0)
        .fill(k.c.layer_01)
        .radius(tok::radius::LG)
        .child(row().key("hub-title").h(26.0).center_y()
            .child(text(tr("sidebar.hub")).font(type_::SECTION).color(k.c.text_base).grow(1.0))
            .child(Button::new(tr("sidebar.killAdb")).variant(ButtonVariant::Ghost)
                .on_press(|s: &mut Ui| s.kill_adb()).build(k))
            .child(Button::new(tr("sidebar.refresh")).variant(ButtonVariant::Ghost)
                .on_press(|s: &mut Ui| { s.refresh_devices(); s.refresh_mdns(); }).build(k)))
        .child(devices.scroll(ScrollSpec { stick_bottom: false }).max_h(220.0).min_h(60.0).shrink())
        .child(tabs)
        .child(col().gap(6.0)
            .child(row().gap(6.0).center_y().child(ip)
                .child(Button::new(tr("sidebar.connect")).variant(ButtonVariant::Contrast)
                    .on_press(|s: &mut Ui| { let ip = s.ip.text.to_string(); s.connect(ip); }).build(k)))
            .child(text(tr("sidebar.discovered")).font(type_::LABEL).color(k.c.text_muted))
            .child(mdns.max_h(100.0).scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0))
            .child(row().gap(6.0).center_y().child(pair_ip).child(pair_code))
            .child(Button::new(tr("sidebar.startPairing")).variant(ButtonVariant::Outline)
                .on_press(|s: &mut Ui| {
                    let ip = s.pair_ip.text.to_string();
                    let code = s.pair_code.text.to_string();
                    s.pair(ip, code);
                }).build(k)))
        .child(
            col().key("push").gap(4.0).padding(12.0).center()
                .fill(k.c.layer_02)
                .radius(tok::radius::MD)
                .on_press_tx(|s: &mut Ui, tx: &mut rune::Tx| {
                    let opts = rune::FileDialogOptions { title: tr_push_title(s), ..Default::default() }
                        .filter("Android App", &["apk"])
                        .filter("All files", &["*"]);
                    tx.open_file(opts, |s: &mut Ui, result: rune::file_dialog::FileDialogResult, _| {
                        if let Ok(Some(path)) = result { s.handle_file(path); }
                    });
                })
                .child(text(tr("sidebar.push")).font(type_::BODY).color(tok::accent(s.accent, s.dark)))
                .child(text(tr("sidebar.pushSub")).font(type_::LABEL).color(k.c.text_muted)),
        )
}

fn tr_push_title(s: &Ui) -> String {
    t(&s.locale, "sidebar.push").to_string()
}

fn center<F: Fn(&str) -> &'static str>(s: &Ui, k: &Kit, layer: &mut Vec<El>, tr: &F) -> El {
    let mut source = row().key("source").gap(6.0);
    for mode in [SessionKind::Mirror, SessionKind::Camera, SessionKind::Desktop] {
        let active = s.tab == mode;
        source = source.child(
            Button::new(mode.label())
                .variant(if active { ButtonVariant::Contrast } else { ButtonVariant::Outline })
                .on_press(move |s: &mut Ui| s.tab = mode).build(k),
        );
    }

    let running = s.running.get(&s.config.device).copied().unwrap_or(false);
    let start_label = match s.tab {
        SessionKind::Mirror => tr("control.start"),
        SessionKind::Camera => tr("control.startCamera"),
        SessionKind::Desktop => tr("control.startDesktop"),
    };

    col()
        .key("center")
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .gap(10.0)
        .child(section(k, tr("control.capture"), source))
        .child(
            col().key("engine-card").gap(10.0).padding(12.0).fill(k.c.layer_01).radius(tok::radius::LG)
                .child(row().gap(8.0).center_y()
                    .child(text(tr("control.engine")).font(type_::SECTION).color(k.c.text_base).grow(1.0))
                    .child(text(if running { tr("control.active") } else { tr("control.ready") })
                        .font(type_::LABEL).color(if running { tok::accent(3, s.dark) } else { k.c.text_muted })))
                .child(config_body(s, k, layer, tr))
                .child(
                    Button::new(if running { tr("control.stop") } else { start_label })
                        .variant(if running { ButtonVariant::Danger } else { ButtonVariant::Submit })
                        .on_press(move |s: &mut Ui| {
                            let device = s.config.device.clone();
                            if !running { s.start_session(); } else { s.stop_session(&device); }
                        }).build(k),
                ),
        )
        .child(log_panel(s, k, tr))
}

fn config_body<F: Fn(&str) -> &'static str>(s: &Ui, k: &Kit, layer: &mut Vec<El>, tr: &F) -> El {
    match s.tab {
        SessionKind::Mirror => mirror_config(s, k, layer, tr),
        SessionKind::Camera => camera_config(s, k, layer, tr),
        SessionKind::Desktop => desktop_config(s, k, layer, tr),
    }
}

fn mirror_config<F: Fn(&str) -> &'static str>(s: &Ui, k: &Kit, layer: &mut Vec<El>, tr: &F) -> El {
    col().key("mirror-config").gap(10.0)
        .child(text(tr("control.inputEnhancements")).font(type_::LABEL).color(k.c.text_muted))
        .child(row().gap(12.0)
            .child(switch(k, tr("control.hidKeyboard"), s.config.hid_keyboard.unwrap_or(false), |s| {
                flip(&mut s.config.hid_keyboard, false)
            }))
            .child(switch(k, tr("control.hidMouse"), s.config.hid_mouse.unwrap_or(false), |s| {
                flip(&mut s.config.hid_mouse, false)
            }))
            .child(switch(k, tr("control.pureHid"), s.config.otg_pure.unwrap_or(false), |s| {
                flip(&mut s.config.otg_pure, false)
            })))
        .child(perf_grid(s, k, layer, tr, true))
        .child(stepper(k, tr("control.bitrate"), s.config.bitrate.unwrap_or(8), "bitrate", 1, 50, 1, tr))
}

fn camera_config<F: Fn(&str) -> &'static str>(s: &Ui, k: &Kit, layer: &mut Vec<El>, tr: &F) -> El {
    col().key("camera-config").gap(10.0)
        .child(row().gap(8.0).center_y()
            .child(text(tr("control.lens")).font(type_::BODY).color(k.c.text_base).grow(1.0))
            .child(select(s, k, layer, "lens", LENSES,
                s.config.camera_id.as_deref().unwrap_or("Auto Select"), tr("control.lens")))
            .child(Button::new(tr("control.refreshLenses")).variant(ButtonVariant::Ghost).build(k)))
        .child(row().gap(8.0).center_y()
            .child(text(tr("control.aspect")).font(type_::BODY).color(k.c.text_base).grow(1.0))
            .child(select(s, k, layer, "aspect", ASPECT,
                s.config.camera_ar.as_deref().unwrap_or("0"), tr("control.aspect"))))
        .child(row().gap(12.0)
            .child(switch(k, tr("control.torch"), s.config.camera_torch.unwrap_or(false), |s| {
                flip(&mut s.config.camera_torch, false)
            }))
            .child(stepper(k, tr("control.zoom"), (s.config.camera_zoom.unwrap_or(1.0) * 10.0) as u32,
                "zoom", 10, 50, 1, tr)))
        .child(perf_grid(s, k, layer, tr, false))
        .child(stepper(k, tr("control.bitrate"), s.config.bitrate.unwrap_or(8), "bitrate", 1, 50, 1, tr))
}

fn desktop_config<F: Fn(&str) -> &'static str>(s: &Ui, k: &Kit, layer: &mut Vec<El>, tr: &F) -> El {
    col().key("desktop-config").gap(10.0)
        .child(row().gap(8.0).center_y()
            .child(text(tr("control.virtualDisplay")).font(type_::LABEL).color(k.c.text_muted).grow(1.0))
            .child(switch(k, tr("control.flex"), s.config.flex_display.unwrap_or(false), |s| {
                flip(&mut s.config.flex_display, false)
            })))
        .child(row().gap(8.0).center_y()
            .child(stepper(k, tr("control.width"), s.config.vd_width.unwrap_or(1920), "vdWidth", 480, 3840, 160, tr))
            .child(stepper(k, tr("control.height"), s.config.vd_height.unwrap_or(1080), "vdHeight", 360, 2160, 90, tr))
            .child(stepper(k, tr("control.dpi"), s.config.vd_dpi.unwrap_or(420), "vdDpi", 120, 640, 20, tr))
            .child(select(s, k, layer, "preset", PRESETS, "Custom", tr("control.width"))))
        .child(perf_grid(s, k, layer, tr, false))
        .child(stepper(k, tr("control.bitrate"), s.config.bitrate.unwrap_or(8), "bitrate", 1, 50, 1, tr))
        .child(row().gap(8.0).center_y()
            .child(text(tr("control.bgColor")).font(type_::BODY).color(k.c.text_base).grow(1.0))
            .child(TextInput::new(&s.background_color).label("Background color").placeholder("#000000")
                .width(Some(120.0)).on_edit(|s: &mut Ui| &mut s.background_color).build(k))
            .child(Button::new(tr("control.default")).variant(ButtonVariant::Ghost)
                .on_press(|s: &mut Ui| { s.config.background_color = None; s.background_color.replace_text(""); }).build(k)))
}

fn perf_grid<F: Fn(&str) -> &'static str>(s: &Ui, k: &Kit, layer: &mut Vec<El>, tr: &F, with_res: bool) -> El {
    let mut grid = col().key("perf").gap(6.0);
    if with_res {
        grid = grid.child(grid_row(k, tr("control.resolution"),
            select(s, k, layer, "res", RES, s.config.res.as_deref().unwrap_or("0"), tr("control.resolution"))));
    }
    grid = grid
        .child(grid_row(k, tr("control.fps"),
            select(s, k, layer, "fps", FPS,
                s.config.fps.map(|v| if v == 0 { "Auto" } else { "60" }).unwrap_or("Auto"), tr("control.fps"))))
        .child(grid_row(k, tr("control.rotation"),
            select(s, k, layer, "rotation", ROT, s.config.rotation.as_deref().unwrap_or("0"), tr("control.rotation"))))
        .child(grid_row(k, tr("control.codec"),
            select(s, k, layer, "codec", CODEC, s.config.codec.as_deref().unwrap_or("h264"), tr("control.codec"))))
        .child(grid_row(k, tr("control.renderer"),
            select(s, k, layer, "renderer", RENDERERS, s.config.render_driver.as_deref().unwrap_or("Auto"), tr("control.renderer"))))
        .child(grid_row(k, tr("control.hwdec"),
            select(s, k, layer, "hwdec", HWDEC, s.config.hwdec.as_deref().unwrap_or("auto"), tr("control.hwdec"))))
        .child(row().gap(12.0)
            .child(switch(k, tr("control.vsync"), s.config.vsync.unwrap_or(true), |s| {
                flip(&mut s.config.vsync, true)
            }))
            .child(switch(k, tr("control.ignoreConstraints"),
                s.config.ignore_video_encoder_constraints.unwrap_or(false), |s| {
                    flip(&mut s.config.ignore_video_encoder_constraints, false)
                })));
    grid
}

fn grid_row(k: &Kit, label: &str, control: El) -> El {
    row().h(30.0).px(4.0).gap(8.0).center_y()
        .child(text(label).font(type_::BODY).color(k.c.text_muted).grow(1.0))
        .child(control)
}

fn right<F: Fn(&str) -> &'static str>(s: &Ui, k: &Kit, layer: &mut Vec<El>, tr: &F) -> El {
    let toggles = col().key("behavior").gap(2.0)
        .child(toggle_row(k, tr("session.awake"), s.config.stay_awake.unwrap_or(false), |s| flip(&mut s.config.stay_awake, false)))
        .child(toggle_row(k, tr("session.keepActive"), s.config.keep_active.unwrap_or(false), |s| flip(&mut s.config.keep_active, false)))
        .child(toggle_row(k, tr("session.screenOff"), s.config.turn_off.unwrap_or(false), |s| flip(&mut s.config.turn_off, false)))
        .child(toggle_row(k, tr("session.audio"), s.config.audio_enabled.unwrap_or(true), |s| flip(&mut s.config.audio_enabled, true)))
        .child(row().h(30.0).px(4.0).center_y()
            .child(text(tr("session.audioCodec")).font(type_::BODY).color(k.c.text_muted).grow(1.0))
            .child(select(s, k, layer, "audio", AUDIO_CODEC,
                s.config.audio_codec.as_deref().unwrap_or("auto"), tr("session.audioCodec"))))
        .child(toggle_row(k, tr("session.alwaysOnTop"), s.config.always_on_top.unwrap_or(false), |s| flip(&mut s.config.always_on_top, false)))
        .child(toggle_row(k, tr("session.fullscreen"), s.config.fullscreen.unwrap_or(false), |s| flip(&mut s.config.fullscreen, false)))
        .child(toggle_row(k, tr("session.borderless"), s.config.borderless.unwrap_or(false), |s| flip(&mut s.config.borderless, false)))
        .child(toggle_row(k, tr("session.remember"), true, |_| {}))
        .child(toggle_row(k, tr("session.record"), s.config.record.unwrap_or(false), |s| flip(&mut s.config.record, false)))
        .child(row().h(30.0).px(4.0).gap(8.0).center_y()
            .child(text(tr("session.recordPath")).font(type_::BODY).color(k.c.text_muted).grow(1.0).truncate())
            .child(Button::new(tr("session.change")).variant(ButtonVariant::Ghost)
                .on_press_tx(|s: &mut Ui, tx: &mut rune::Tx| {
                    let opts = rune::FileDialogOptions { title: "Recording folder".into(), ..Default::default() };
                    tx.open_folder(opts, |s: &mut Ui, result: rune::file_dialog::FileDialogResult, _| {
                        if let Ok(Some(path)) = result {
                            s.set_record_path(path.to_string_lossy().to_string());
                        }
                    });
                }).build(k)))
        .child(text(s.config.record_path.as_deref().unwrap_or(tr("session.defaultVideos")))
            .font(type_::MONO_CAPTION).color(k.c.text_faint).truncate());

    let shortcuts = col().key("shortcuts").gap(4.0)
        .child(text(tr("shortcuts.title")).font(type_::SECTION).color(k.c.text_base))
        .child(row().gap(4.0)
            .child(shortcut_chip(k, "Alt+F", tr("session.fullscreen")))
            .child(shortcut_chip(k, "Alt+H", "Home"))
            .child(shortcut_chip(k, "Alt+B", "Back"))
            .child(shortcut_chip(k, "Alt+S", "Recents")))
        .child(row().gap(4.0)
            .child(shortcut_chip(k, "Alt+P", "Power"))
            .child(shortcut_chip(k, "Alt+R", "Rotate"))
            .child(shortcut_chip(k, "Alt+V", "Paste"))
            .child(shortcut_chip(k, "Alt+O", "Off")))
        .child(text("Ctrl+Alt+Shift+C  Recenter  ·  Ctrl+Alt+Shift+W  Drag")
            .font(type_::MONO_CAPTION).color(k.c.text_faint).wrap().w_full());

    col().key("right").w(290.0).min_h(0.0).gap(10.0)
        .child(section(k, tr("session.title"), toggles))
        .child(section(k, "Info", shortcuts))
}

fn log_panel<F: Fn(&str) -> &'static str>(s: &Ui, k: &Kit, tr: &F) -> El {
    let mut lines = col().key("log-lines").gap(1.0);
    if s.logs.is_empty() {
        lines = lines.child(text(tr("log.empty")).font(type_::MONO_CAPTION).color(k.c.text_faint));
    }
    for (i, line) in s.logs.iter().enumerate() {
        lines = lines.child(text(line.clone()).key(format!("log-{i}"))
            .font(type_::MONO_CAPTION).color(k.c.text_base).wrap());
    }
    col().key("log").h(220.0).padding(10.0).gap(6.0)
        .fill(k.c.bg_deep)
        .radius(tok::radius::MD)
        .child(row().key("log-bar").h(24.0).center_y()
            .child(text(tr("log.console")).font(type_::LABEL).color(k.c.text_base).grow(1.0))
            .child(Button::new(tr("log.clear")).variant(ButtonVariant::Ghost)
                .on_press(|s: &mut Ui| s.logs.clear()).build(k)))
        .child(lines.scroll(ScrollSpec { stick_bottom: true }).grow(1.0).shrink().min_h(0.0))
        .child(row().key("prompt").gap(6.0).center_y()
            .child(text("$").font(type_::MONO_CAPTION).color(tok::accent(s.accent, s.dark)))
            .child(TextInput::new(&s.command).label("Command").placeholder("adb shell ...")
                .width(None).on_edit(|s: &mut Ui| &mut s.command)
                .on_submit(|s: &mut Ui, _| { let cmd = s.command.text.to_string(); s.run_command(cmd); s.command.replace_text(""); })
                .build(k)))
}

fn footer<F: Fn(&str) -> &'static str>(k: &Kit, tr: &F) -> El {
    row().key("footer").h(38.0).px(14.0).gap(10.0).center_y().fill(k.c.bg_base)
        .border(1.0, k.c.border_muted)
        .child(text(tr("footer.about")).font(type_::LABEL).color(k.c.text_muted).grow(1.0))
        .child(text("Core: scrcpy  ·  Client: Rune  ·  UI: Rust").font(type_::LABEL).color(k.c.text_muted))
        .child(text("v4.1.0").font(type_::MONO_CAPTION).color(k.c.text_faint))
}

fn onboarding<F: Fn(&str) -> &'static str>(s: &Ui, k: &Kit, tr: &F) -> El {
    col().key("onboarding-scrim").absolute(0.0, 0.0).w_full().h_full().center()
        .fill(k.c.black.alpha(0.62))
        .child(col().key("onboarding-card").w(520.0).gap(14.0).padding(24.0)
            .fill(k.c.layer_01).radius(tok::radius::LG)
            .child(text(tr("onboarding.title")).font(type_::TITLE).color(k.c.text_base))
            .child(text(tr("onboarding.manual")).font(type_::BODY).color(k.c.text_muted).wrap())
            .child(Button::new(tr("onboarding.download")).variant(ButtonVariant::Submit)
                .disabled(s.download.active)
                .on_press(|s: &mut Ui| s.download_scrcpy()).build(k))
            .child(row().center_y().gap(10.0)
                .child(Button::new(tr("onboarding.continue")).variant(ButtonVariant::Contrast)
                    .disabled(s.scrcpy_ok != Some(true))
                    .on_press(|s: &mut Ui| s.onboarding_open = false).build(k))
                .child(Button::new(tr("common.close")).variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut Ui| s.onboarding_open = false).build(k))))
}

fn alert(k: &Kit, message: &str) -> El {
    col().key("alert-scrim").absolute(0.0, 0.0).w_full().h_full().center().fill(k.c.black.alpha(0.62))
        .child(col().key("alert-card").w(380.0).gap(12.0).padding(20.0)
            .fill(k.c.layer_01).radius(tok::radius::LG)
            .child(text(message).font(type_::BODY).color(k.c.text_base).wrap())
            .child(Button::new("OK").variant(ButtonVariant::Contrast)
                .on_press(|s: &mut Ui| s.alert = None).build(k)))
}

fn section(k: &Kit, title: &str, body: El) -> El {
    col().gap(6.0)
        .child(text(title).font(type_::SECTION).color(k.c.text_muted).pl(4.0))
        .child(body.fill(k.c.layer_01).radius(tok::radius::LG).padding(10.0))
}

fn switch<F: Fn(&mut Ui) + 'static>(k: &Kit, label: &str, on: bool, f: F) -> El {
    Switch::new(on).label(label).on_toggle(f).build(k)
}

fn toggle_row(k: &Kit, label: &str, on: bool, f: impl Fn(&mut Ui) + 'static) -> El {
    row().h(32.0).px(4.0).center_y().gap(8.0).label(label)
        .child(text(label).font(type_::BODY).color(k.c.text_base).grow(1.0))
        .child(Switch::new(on).label(label).on_toggle(f).build(k))
}

fn shortcut_chip(k: &Kit, key: &str, label: &str) -> El {
    col().key(key).w(64.0).h(44.0).center().gap(2.0)
        .fill(k.c.layer_02).radius(tok::radius::SM)
        .child(text(key).font(type_::LABEL).color(k.c.text_base))
        .child(text(label).font(type_::MONO_CAPTION).color(k.c.text_faint))
}

fn stepper<F: Fn(&str) -> &'static str>(k: &Kit, label: &str, value: u32, field: &'static str,
    min: u32, max: u32, step: u32, tr: &F) -> El {
    row().h(30.0).gap(4.0).center_y().key(format!("step-{field}"))
        .child(text(label).font(type_::BODY).color(k.c.text_muted).grow(1.0))
        .child(Button::new("-").variant(ButtonVariant::Ghost)
            .on_press(move |s: &mut Ui| step_config(s, field, -(step as i64))).build(k))
        .child(text(format!("{}", value)).font(type_::MONO_CAPTION).color(k.c.text_base))
        .child(Button::new("+").variant(ButtonVariant::Ghost)
            .on_press(move |s: &mut Ui| step_config(s, field, step as i64)).build(k))
        .child(text(format!("{}-{}", min, max)).font(type_::MONO_CAPTION).color(k.c.text_faint))
}

fn select(s: &Ui, k: &Kit, layer: &mut Vec<El>, key: &'static str, options: &[&str], current: &str, label: &str) -> El {
    Select::new(key, options)
        .width(110.0)
        .current(Some(current))
        .label(label)
        .open(s.open_select.as_deref() == Some(key))
        .on_toggle(move |s: &mut Ui| toggle_select(s, key))
        .on_close(|s: &mut Ui| s.open_select = None)
        .on_choose(move |s: &mut Ui, v: &str, _| apply_choice(s, key, v))
        .build(k, layer)
}

fn toggle_select(s: &mut Ui, key: &str) {
    s.open_select = if s.open_select.as_deref() == Some(key) { None } else { Some(key.to_string()) };
}

fn flip(field: &mut Option<bool>, fallback: bool) {
    *field = Some(!field.unwrap_or(fallback));
}

fn step_config(s: &mut Ui, field: &str, delta: i64) {
    let clamp = |v: u32, min: u32, max: u32| -> u32 { (v as i64 + delta).clamp(min as i64, max as i64) as u32 };
    match field {
        "bitrate" => s.config.bitrate = Some(clamp(s.config.bitrate.unwrap_or(8), 1, 50)),
        "zoom" => s.config.camera_zoom = Some(clamp((s.config.camera_zoom.unwrap_or(1.0) * 10.0) as u32, 10, 50) as f32 / 10.0),
        "vdWidth" => {
            let w = clamp(s.config.vd_width.unwrap_or(1920), 480, 3840);
            s.config.vd_width = Some(w);
            if s.config.flex_display.unwrap_or(false) {
                s.config.vd_height = Some(((w * 9) / 16).clamp(360, 2160));
            }
        }
        "vdHeight" => {
            let h = clamp(s.config.vd_height.unwrap_or(1080), 360, 2160);
            s.config.vd_height = Some(h);
            if s.config.flex_display.unwrap_or(false) {
                s.config.vd_width = Some(((h * 16) / 9).clamp(480, 3840));
            }
        }
        "vdDpi" => s.config.vd_dpi = Some(clamp(s.config.vd_dpi.unwrap_or(420), 120, 640)),
        _ => {}
    }
    s.save_config();
}

fn apply_choice(s: &mut Ui, key: &str, v: &str) {
    match key {
        "fps" => s.config.fps = if v == "Auto" { None } else { v.parse().ok() },
        "codec" => s.config.codec = Some(v.into()),
        "res" => s.config.res = Some(v.into()),
        "rotation" => s.config.rotation = Some(v.into()),
        "renderer" => s.config.render_driver = if v == "Auto" { None } else { Some(v.into()) },
        "hwdec" => s.config.hwdec = Some(v.into()),
        "aspect" => s.config.camera_ar = Some(v.into()),
        "audio" => s.config.audio_codec = Some(v.into()),
        "lens" => s.config.camera_id = if v == "Auto Select" { None } else { Some(v.into()) },
        "locale" => s.set_locale(v),
        "preset" => match v {
            "1080p" => { s.config.vd_width = Some(1920); s.config.vd_height = Some(1080); }
            "1440p" => { s.config.vd_width = Some(2560); s.config.vd_height = Some(1440); }
            "4K" => { s.config.vd_width = Some(3840); s.config.vd_height = Some(2160); }
            "21:9" => { s.config.vd_width = Some(3440); s.config.vd_height = Some(1440); }
            _ => {}
        },
        _ => {}
    }
    s.save_config();
}
