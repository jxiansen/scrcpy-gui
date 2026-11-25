//! Official rune-kit shell for ScrcpyGUI.
//!
//! The shell, sidebar, toolbar, cards and form controls come from rune-kit with
//! the OpenCode theme. The console, file drop zone and scrcpy-specific icons stay
//! custom because the kit has no equivalent.

use rune::{Color, Cx, El, LiveRegion, ScrollSpec, col, el, row, text};
use rune_kit::glyphs;
use rune_kit::{
    Button, ButtonVariant, Card, ControlSize, Dialog, Field, Kit, Menu, MenuItem, SegmentedControl,
    Select, Sidebar, SidebarHeader, SidebarRow, SidebarSection, Slider, Spinner, SplitView, Switch,
    TextInput, Toast, ToastRegion, Toolbar, root,
};
use rune_theme_opencode as theme;

use crate::i18n::{language_name, t, tv};
use crate::icons;
use crate::ui::Ui;
use crate::ui::state::{SessionKind, Transport};

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
const LOCALES: &[&str] = &["en", "fr", "pt-BR", "zh-CN", "zh-TW", "ru", "id", "ar"];

fn tr(s: &Ui, key: &str) -> &'static str {
    t(&s.locale, key)
}

fn flip(field: &mut Option<bool>, fallback: bool) {
    *field = Some(!field.unwrap_or(fallback));
}

fn preset_name(w: u32, h: u32) -> &'static str {
    match (w, h) {
        (1920, 1080) => "1080p",
        (2560, 1440) => "1440p",
        (3840, 2160) => "4K",
        (3440, 1440) => "21:9",
        _ => "Custom",
    }
}

fn valid_hex(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn display_option(locale: &str, key: &str, value: &str) -> String {
    let label = match (key, value) {
        ("fps" | "renderer", "Auto") | ("hwdec", "auto") => "controlPanel.rendererAuto",
        ("audio", "auto") => "sessionBehavior.audioCodecAuto",
        ("hwdec", "disabled") => "controlPanel.hardwareDecodingDisabled",
        ("res", "0") => "controlPanel.resolutionOriginal",
        ("aspect", "0") => "controlPanel.aspectDefault",
        ("lens", "Auto Select") => "controlPanel.autoSelect",
        ("lens", "Back") => "controlPanel.facingBack",
        ("lens", "Front") => "controlPanel.facingFront",
        ("lens", "External") => "controlPanel.facingExternal",
        ("preset", "Custom") => "controlPanel.custom",
        ("preset", "1080p") => "controlPanel.preset1080p",
        ("preset", "1440p") => "controlPanel.preset1440p",
        ("preset", "4K") => "controlPanel.preset4k",
        ("preset", "21:9") => "controlPanel.presetUltrawide",
        ("rotation", _) => return format!("{value}°"),
        _ => return value.to_string(),
    };
    t(locale, label).to_string()
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
        "lens" => {
            s.config.camera_id = if v == "Auto Select" {
                None
            } else {
                Some(v.into())
            }
        }
        "preset" => match v {
            "1080p" => {
                s.config.vd_width = Some(1920);
                s.config.vd_height = Some(1080);
            }
            "1440p" => {
                s.config.vd_width = Some(2560);
                s.config.vd_height = Some(1440);
            }
            "4K" => {
                s.config.vd_width = Some(3840);
                s.config.vd_height = Some(2160);
            }
            "21:9" => {
                s.config.vd_width = Some(3440);
                s.config.vd_height = Some(1440);
            }
            _ => {}
        },
        _ => {}
    }
    s.save_config();
}

fn select_control(
    s: &Ui,
    k: &Kit,
    layer: &mut Vec<El>,
    key: &'static str,
    options: &[&str],
    current: &str,
    label: &str,
    width: f32,
) -> El {
    let labels: Vec<String> = options
        .iter()
        .map(|v| display_option(&s.locale, key, v))
        .collect();
    let raw: Vec<String> = options.iter().map(|v| (*v).to_string()).collect();
    let current_label = display_option(&s.locale, key, current);
    Select::new(key, &labels)
        .width(width)
        .current(Some(&current_label))
        .label(label)
        .appearance(rune_kit::SelectAppearance::Field)
        .open(s.open_select.as_deref() == Some(key))
        .on_toggle(move |s: &mut Ui| {
            s.open_select = if s.open_select.as_deref() == Some(key) {
                None
            } else {
                Some(key.into())
            }
        })
        .on_close(|s: &mut Ui| s.open_select = None)
        .on_choose(move |s: &mut Ui, chosen: &str, _| {
            if let Some(i) = labels.iter().position(|l| l == chosen) {
                apply_choice(s, key, &raw[i]);
            }
        })
        .build(k, layer)
}

fn field(label: &str, control: El, k: &Kit) -> El {
    Field::new().label(label).control(control).build(k)
}

fn cell(content: El) -> El {
    col().grow(1.0).min_w(210.0).child(content)
}

/// A narrower cell for camera/desktop, whose five engine fields fit three per row.
fn cell_compact(content: El) -> El {
    col().grow(1.0).min_w(128.0).child(content)
}

fn grid(fields: Vec<El>) -> El {
    row().w_full().gap(12.0).flex_wrap().children(fields)
}

fn switch_row(s: &Ui, k: &Kit, label: &str, on: bool, f: impl Fn(&mut Ui) + 'static) -> El {
    let _ = s;
    Switch::new(on).label(label).on_toggle(f).build(k)
}

fn labeled_slider(k: &Kit, label: &str, value: String, slider: El) -> El {
    col()
        .w_full()
        .gap(4.0)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(text(label).font(k.ty.small).color(k.c.text_base))
                .child(row().grow(1.0))
                .child(text(value).font(k.ty.small).color(k.c.text_muted)),
        )
        .child(slider)
}

fn section_card(
    k: &Kit,
    key: &str,
    title: &str,
    icon: Option<&'static rune::Icon>,
    body: Vec<El>,
) -> El {
    let mut card = Card::new().title(title, icon);
    for child in body {
        card = card.child(child);
    }
    card.build(k).key(key)
}

fn folder_button(
    _s: &Ui,
    k: &Kit,
    label: &str,
    apply: impl Fn(&mut Ui, std::path::PathBuf) + Clone + 'static,
) -> El {
    let title = label.to_string();
    Button::new(label)
        .variant(ButtonVariant::Outline)
        .control_size(ControlSize::Small)
        .on_press_tx(move |_ui: &mut Ui, tx: &mut rune::Tx| {
            let opts = rune::FileDialogOptions {
                title: title.clone(),
                ..Default::default()
            };
            let apply = apply.clone();
            tx.open_folder(
                opts,
                move |s: &mut Ui, result: rune::file_dialog::FileDialogResult, _| {
                    if let Ok(Some(path)) = result {
                        apply(s, path);
                    }
                },
            );
        })
        .build(k)
}

fn engine_fields(s: &Ui, k: &Kit, layer: &mut Vec<El>, with_res: bool, compact: bool) -> Vec<El> {
    let sel_w = if compact { 128.0 } else { 210.0 };
    let wrap = |content: El| {
        if compact {
            cell_compact(content)
        } else {
            cell(content)
        }
    };
    let fps = s
        .config
        .fps
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Auto".into());
    let mut fields = Vec::new();
    if with_res {
        fields.push(wrap(field(
            tr(s, "controlPanel.resolution"),
            select_control(
                s,
                k,
                layer,
                "res",
                RES,
                s.config.res.as_deref().unwrap_or("0"),
                tr(s, "controlPanel.resolution"),
                sel_w,
            ),
            k,
        )));
    }
    fields.push(wrap(field(
        tr(s, "controlPanel.fps"),
        select_control(
            s,
            k,
            layer,
            "fps",
            FPS,
            &fps,
            tr(s, "controlPanel.fps"),
            sel_w,
        ),
        k,
    )));
    fields.push(wrap(field(
        tr(s, "controlPanel.rotation"),
        select_control(
            s,
            k,
            layer,
            "rotation",
            ROT,
            s.config.rotation.as_deref().unwrap_or("0"),
            tr(s, "controlPanel.rotation"),
            sel_w,
        ),
        k,
    )));
    fields.push(wrap(field(
        tr(s, "controlPanel.codec"),
        select_control(
            s,
            k,
            layer,
            "codec",
            CODEC,
            s.config.codec.as_deref().unwrap_or("h264"),
            tr(s, "controlPanel.codec"),
            sel_w,
        ),
        k,
    )));
    fields.push(wrap(field(
        tr(s, "controlPanel.graphicsRenderer"),
        select_control(
            s,
            k,
            layer,
            "renderer",
            RENDERERS,
            s.config.render_driver.as_deref().unwrap_or("Auto"),
            tr(s, "controlPanel.graphicsRenderer"),
            sel_w,
        ),
        k,
    )));
    fields.push(wrap(field(
        tr(s, "controlPanel.hardwareDecoding"),
        select_control(
            s,
            k,
            layer,
            "hwdec",
            HWDEC,
            s.config.hwdec.as_deref().unwrap_or("auto"),
            tr(s, "controlPanel.hardwareDecoding"),
            sel_w,
        ),
        k,
    )));
    fields
}

fn bitrate(s: &Ui, k: &Kit) -> El {
    let slider = Slider::new("bitrate", s.config.bitrate.unwrap_or(8) as f32)
        .range(1.0..=50.0)
        .step(1.0)
        .label(tr(s, "controlPanel.bitrate"))
        .hide_label()
        .width(420.0)
        .on_change(|s: &mut Ui, v, _| {
            s.config.bitrate = Some(v.round() as u32);
            s.save_config();
        })
        .build(k);
    labeled_slider(
        k,
        tr(s, "controlPanel.bitrate"),
        format!("{} Mbps", s.config.bitrate.unwrap_or(8)),
        slider,
    )
}

fn background_color(s: &Ui, k: &Kit, tr: &dyn Fn(&str) -> &'static str) -> El {
    let value = s.config.background_color.clone().unwrap_or_default();
    row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            Field::new()
                .label(tr("controlPanel.backgroundColor"))
                .control(
                    TextInput::new(&s.background_color)
                        .label(tr("controlPanel.backgroundColor"))
                        .placeholder("#000000")
                        .width(None)
                        .on_edit(|s: &mut Ui| &mut s.background_color)
                        .on_submit(|s: &mut Ui, _| {
                            let v = s.background_color.text.trim().to_string();
                            if valid_hex(&v) {
                                s.config.background_color = Some(v);
                                s.save_config();
                            }
                        })
                        .build(k),
                )
                .build(k)
                .grow(1.0),
        )
        .child(
            Button::new(tr("controlPanel.backgroundColorNone"))
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .disabled(value.is_empty())
                .on_press(|s: &mut Ui| {
                    s.config.background_color = None;
                    s.background_color.replace_text("");
                    s.save_config();
                })
                .build(k),
        )
}

fn mirror_config(s: &Ui, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut body = Vec::new();
    body.push(
        col()
            .gap(8.0)
            .child(
                text(tr(s, "controlPanel.inputEnhancements"))
                    .font(k.ty.small_medium)
                    .color(k.c.text_muted),
            )
            .child(
                text(tr(s, "controlPanel.inputEnhancementsDescription"))
                    .font(k.ty.small)
                    .color(k.c.text_faint)
                    .wrap(),
            )
            .child(row().gap(16.0).flex_wrap().children([
                switch_row(
                    s,
                    k,
                    tr(s, "controlPanel.hidKeyboard"),
                    s.config.hid_keyboard.unwrap_or(false),
                    |s| {
                        flip(&mut s.config.hid_keyboard, false);
                        s.save_config();
                    },
                ),
                switch_row(
                    s,
                    k,
                    tr(s, "controlPanel.hidMouse"),
                    s.config.hid_mouse.unwrap_or(false),
                    |s| {
                        flip(&mut s.config.hid_mouse, false);
                        s.save_config();
                    },
                ),
                switch_row(
                    s,
                    k,
                    tr(s, "controlPanel.pureHid"),
                    s.config.otg_pure.unwrap_or(false),
                    |s| {
                        flip(&mut s.config.otg_pure, false);
                        s.save_config();
                    },
                ),
            ])),
    );
    body.push(grid(engine_fields(s, k, layer, true, false)));
    body.push(row().gap(24.0).flex_wrap().children([
        switch_row(
            s,
            k,
            tr(s, "controlPanel.vsync"),
            s.config.vsync.unwrap_or(true),
            |s| {
                flip(&mut s.config.vsync, true);
                s.save_config();
            },
        ),
        switch_row(
            s,
            k,
            tr(s, "controlPanel.ignoreEncoderConstraints"),
            s.config.ignore_video_encoder_constraints.unwrap_or(false),
            |s| {
                flip(&mut s.config.ignore_video_encoder_constraints, false);
                s.save_config();
            },
        ),
    ]));
    body.push(bitrate(s, k));
    body.into_iter()
        .fold(col().w_full().gap(12.0), |c, child| c.child(child))
}

fn camera_config(s: &Ui, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut body = Vec::new();
    body.push(
        col()
            .gap(4.0)
            .padding(12.0)
            .radius(k.radius.md)
            .fill(k.c.layer_01)
            .child(
                text(tr(s, "controlPanel.webcamProTip"))
                    .font(k.ty.small_medium)
                    .color(k.c.text_accent),
            )
            .child(
                text(format!(
                    "{} {} {} {} {}",
                    tr(s, "controlPanel.webcamProTipTextBefore"),
                    tr(s, "controlPanel.webcamProTipObs"),
                    tr(s, "controlPanel.webcamProTipAndStart"),
                    tr(s, "controlPanel.webcamProTipVirtualCamera"),
                    tr(s, "controlPanel.webcamProTipTextAfter")
                ))
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap(),
            ),
    );
    body.push(grid(vec![
        cell(field(
            tr(s, "controlPanel.cameraDevice"),
            select_control(
                s,
                k,
                layer,
                "lens",
                LENSES,
                s.config.camera_id.as_deref().unwrap_or("Auto Select"),
                tr(s, "controlPanel.cameraDevice"),
                200.0,
            ),
            k,
        )),
        cell(field(
            tr(s, "controlPanel.aspect"),
            select_control(
                s,
                k,
                layer,
                "aspect",
                ASPECT,
                s.config.camera_ar.as_deref().unwrap_or("0"),
                tr(s, "controlPanel.aspect"),
                160.0,
            ),
            k,
        )),
    ]));
    body.push(
        row().gap(20.0).flex_wrap().children([
            switch_row(
                s,
                k,
                tr(s, "controlPanel.cameraTorch"),
                s.config.camera_torch.unwrap_or(false),
                |s| {
                    flip(&mut s.config.camera_torch, false);
                    s.save_config();
                },
            ),
            labeled_slider(
                k,
                tr(s, "controlPanel.cameraZoom"),
                format!("{:.1}x", s.config.camera_zoom.unwrap_or(1.0)),
                Slider::new("zoom", s.config.camera_zoom.unwrap_or(1.0))
                    .range(1.0..=5.0)
                    .step(0.1)
                    .label(tr(s, "controlPanel.cameraZoom"))
                    .hide_label()
                    .width(300.0)
                    .on_change(|s: &mut Ui, v, _| {
                        s.config.camera_zoom = Some(v);
                        s.save_config();
                    })
                    .build(k),
            ),
        ]),
    );
    body.push(grid(engine_fields(s, k, layer, false, true)));
    body.push(bitrate(s, k));
    body.into_iter()
        .fold(col().w_full().gap(12.0), |c, child| c.child(child))
}

fn desktop_config(s: &Ui, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut body = Vec::new();
    body.push(grid(vec![
        cell(labeled_slider(
            k,
            tr(s, "controlPanel.width"),
            s.config.vd_width.unwrap_or(1920).to_string(),
            Slider::new("vdWidth", s.config.vd_width.unwrap_or(1920) as f32)
                .range(480.0..=3840.0)
                .step(10.0)
                .label(tr(s, "controlPanel.width"))
                .hide_label()
                .width(200.0)
                .on_change(|s: &mut Ui, v, _| {
                    s.config.vd_width = Some(v.round() as u32);
                    s.save_config();
                })
                .build(k),
        )),
        cell(labeled_slider(
            k,
            tr(s, "controlPanel.height"),
            s.config.vd_height.unwrap_or(1080).to_string(),
            Slider::new("vdHeight", s.config.vd_height.unwrap_or(1080) as f32)
                .range(360.0..=2160.0)
                .step(10.0)
                .label(tr(s, "controlPanel.height"))
                .hide_label()
                .width(200.0)
                .on_change(|s: &mut Ui, v, _| {
                    s.config.vd_height = Some(v.round() as u32);
                    s.save_config();
                })
                .build(k),
        )),
    ]));
    body.push(grid(vec![
        cell(labeled_slider(
            k,
            tr(s, "controlPanel.uiScaling"),
            s.config.vd_dpi.unwrap_or(420).to_string(),
            Slider::new("vdDpi", s.config.vd_dpi.unwrap_or(420) as f32)
                .range(120.0..=640.0)
                .step(10.0)
                .label(tr(s, "controlPanel.uiScaling"))
                .hide_label()
                .width(200.0)
                .on_change(|s: &mut Ui, v, _| {
                    s.config.vd_dpi = Some(v.round() as u32);
                    s.save_config();
                })
                .build(k),
        )),
        cell(field(
            tr(s, "controlPanel.quickPresets"),
            select_control(
                s,
                k,
                layer,
                "preset",
                PRESETS,
                preset_name(
                    s.config.vd_width.unwrap_or(0),
                    s.config.vd_height.unwrap_or(0),
                ),
                tr(s, "controlPanel.quickPresets"),
                200.0,
            ),
            k,
        )),
    ]));
    body.push(grid(engine_fields(s, k, layer, false, true)));
    body.push(switch_row(
        s,
        k,
        tr(s, "controlPanel.flexDisplay"),
        s.config.flex_display.unwrap_or(false),
        |s| {
            flip(&mut s.config.flex_display, false);
            s.save_config();
        },
    ));
    body.push(bitrate(s, k));
    body.push(background_color(s, k, &|key| tr(s, key)));
    body.into_iter()
        .fold(col().w_full().gap(12.0), |c, child| c.child(child))
}

fn console(s: &Ui, k: &Kit) -> El {
    let mut logs = col().key("console-lines").gap(2.0);
    for (i, (time, line)) in s.logs.iter().enumerate() {
        let mut entry = row().key(format!("log-{i}")).gap(12.0);
        if i + 1 == s.logs.len() {
            entry = entry.live_region(LiveRegion::Polite);
        }
        logs = logs.child(
            entry
                .child(text(time.clone()).font(k.ty.mono).color(k.c.text_faint))
                .child(
                    text(line.clone())
                        .font(k.ty.mono)
                        .color(k.c.text_muted)
                        .wrap(),
                ),
        );
    }
    if s.logs.is_empty() {
        logs = logs.child(
            text(tr(s, "logPanel.waitingForSequence"))
                .font(k.ty.mono)
                .color(k.c.text_faint),
        );
    }
    Card::new()
        .title(tr(s, "logPanel.systemConsole"), Some(&glyphs::FILE_CODE))
        .child(
            col()
                .key("console-body")
                .gap(8.0)
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .child(row().grow(1.0))
                        .child(
                            Button::new(tr(s, "logPanel.report"))
                                .variant(ButtonVariant::GhostMuted)
                                .control_size(ControlSize::Small)
                                .on_press(|s: &mut Ui| s.export_report())
                                .build(k)
                                .key("console-export"),
                        )
                        .child(
                            Button::new(tr(s, "logPanel.clear"))
                                .variant(ButtonVariant::GhostMuted)
                                .control_size(ControlSize::Small)
                                .on_press(|s: &mut Ui| s.logs.clear())
                                .build(k)
                                .key("console-clear"),
                        ),
                )
                .child(
                    logs.scroll(ScrollSpec { stick_bottom: true })
                        .h(120.0)
                        .min_h(0.0),
                )
                .child(
                    TextInput::new(&s.command)
                        .plain()
                        .label(tr(s, "logPanel.terminalPlaceholder"))
                        .placeholder(tr(s, "logPanel.terminalPlaceholder"))
                        .width(None)
                        .on_edit(|s: &mut Ui| &mut s.command)
                        .on_submit(|s: &mut Ui, _| {
                            let cmd = s.command.text.clone();
                            s.run_command(cmd);
                            s.command.replace_text("");
                        })
                        .build(k),
                ),
        )
        .build(k)
        .key("console")
}

fn main_view(s: &Ui, k: &Kit, layer: &mut Vec<El>) -> El {
    let running = s.running.get(&s.config.device).copied().unwrap_or(false);
    let label = if running {
        tr(s, "controlPanel.stopSession")
    } else {
        match s.tab {
            SessionKind::Mirror => tr(s, "controlPanel.startMission"),
            SessionKind::Camera => tr(s, "controlPanel.engageCamera"),
            SessionKind::Desktop => tr(s, "controlPanel.ejectToDesktop"),
        }
    };
    let start = Button::new(label)
        .variant(if running {
            ButtonVariant::Danger
        } else {
            ButtonVariant::Submit
        })
        .control_size(ControlSize::ExtraLarge)
        .disabled(s.config.device.is_empty())
        .on_press(move |s: &mut Ui| {
            if running {
                let device = s.config.device.clone();
                s.stop_session(&device);
            } else {
                s.start_session();
            }
        })
        .build(k)
        .key("start-stop");
    let target = if s.config.device.is_empty() {
        tr(s, "ui.noDevice").to_string()
    } else {
        let transport = if s.config.device.contains(':') {
            tr(s, "sidebar.wifi")
        } else {
            tr(s, "sidebar.usb")
        };
        format!("{} · {}", s.config.device, transport)
    };
    let config = match s.tab {
        SessionKind::Mirror => mirror_config(s, k, layer),
        SessionKind::Camera => camera_config(s, k, layer),
        SessionKind::Desktop => desktop_config(s, k, layer),
    };
    col()
        .key("main-content")
        .w_full()
        .gap(16.0)
        .padding(16.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            SegmentedControl::new(
                &[
                    tr(s, "controlPanel.screen"),
                    tr(s, "controlPanel.camera"),
                    tr(s, "controlPanel.desktop"),
                ],
                Some(s.tab as usize),
            )
            .label(tr(s, "controlPanel.captureSource"))
            .width(None)
            .on_pick(|s: &mut Ui, i, _| {
                s.tab = match i {
                    0 => SessionKind::Mirror,
                    1 => SessionKind::Camera,
                    _ => SessionKind::Desktop,
                };
            })
            .build(k),
        )
        .child(section_card(
            k,
            "engine",
            tr(s, "controlPanel.engineConfiguration"),
            Some(&icons::GEAR),
            vec![text(target).font(k.ty.small).color(k.c.text_muted), config],
        ))
        .child(start)
        .child(console(s, k))
}

fn appearance_card(s: &Ui, k: &Kit, layer: &mut Vec<El>, system_dark: bool) -> El {
    let selected = match s.color_mode.as_str() {
        "light" => 0,
        "dark" => 1,
        _ => 2,
    };
    let language_labels: Vec<String> = LOCALES
        .iter()
        .map(|code| language_name(&s.locale, code).to_string())
        .collect();
    let language = Select::new("locale", &language_labels)
        .width(180.0)
        .current(Some(language_name(&s.locale, &s.locale)))
        .label(tr(s, "header.languageLabel"))
        .open(s.open_select.as_deref() == Some("locale"))
        .on_toggle(|s: &mut Ui| {
            s.open_select = if s.open_select.as_deref() == Some("locale") {
                None
            } else {
                Some("locale".into())
            }
        })
        .on_close(|s: &mut Ui| s.open_select = None)
        .on_choose(|s: &mut Ui, label: &str, _| {
            if let Some(i) = LOCALES
                .iter()
                .position(|code| language_name(&s.locale, code) == label)
            {
                s.set_locale(LOCALES[i]);
            }
        })
        .build(k, layer);
    section_card(
        k,
        "appearance",
        tr(s, "header.themeLabel"),
        None,
        vec![
            SegmentedControl::new(
                &[
                    tr(s, "header.colorModes.light"),
                    tr(s, "header.colorModes.dark"),
                    tr(s, "header.colorModes.system"),
                ],
                Some(selected),
            )
            .label(tr(s, "header.colorModes.light"))
            .width(None)
            .on_pick(move |s: &mut Ui, i, _| {
                s.set_color_mode(
                    match i {
                        0 => "light",
                        1 => "dark",
                        _ => "system",
                    },
                    system_dark,
                );
            })
            .build(k),
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    text(tr(s, "header.languageLabel"))
                        .font(k.ty.small)
                        .color(k.c.text_muted),
                )
                .child(language),
            switch_row(s, k, tr(s, "header.panelOnTop"), s.panel_on_top, |s| {
                s.toggle_panel_on_top()
            }),
        ],
    )
}

fn behavior_card(s: &Ui, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut rows = col().w_full().gap(4.0);
    for (label, value, key, fallback) in [
        (
            "sessionBehavior.stayAwake",
            s.config.stay_awake,
            "stay_awake",
            false,
        ),
        (
            "sessionBehavior.keepActive",
            s.config.keep_active,
            "keep_active",
            false,
        ),
        (
            "sessionBehavior.screenOff",
            s.config.turn_off,
            "turn_off",
            false,
        ),
        (
            "sessionBehavior.forwardAudio",
            s.config.audio_enabled,
            "audio_enabled",
            true,
        ),
        (
            "sessionBehavior.alwaysOnTop",
            s.config.always_on_top,
            "always_on_top",
            false,
        ),
        (
            "sessionBehavior.fullScreen",
            s.config.fullscreen,
            "fullscreen",
            false,
        ),
        (
            "sessionBehavior.borderless",
            s.config.borderless,
            "borderless",
            false,
        ),
    ] {
        rows = rows.child(switch_row(
            s,
            k,
            tr(s, label),
            value.unwrap_or(fallback),
            move |s: &mut Ui| {
                match key {
                    "stay_awake" => flip(&mut s.config.stay_awake, false),
                    "keep_active" => flip(&mut s.config.keep_active, false),
                    "turn_off" => flip(&mut s.config.turn_off, false),
                    "audio_enabled" => flip(&mut s.config.audio_enabled, true),
                    "always_on_top" => flip(&mut s.config.always_on_top, false),
                    "fullscreen" => flip(&mut s.config.fullscreen, false),
                    "borderless" => flip(&mut s.config.borderless, false),
                    _ => {}
                }
                s.save_config();
            },
        ));
    }
    rows = rows.child(switch_row(
        s,
        k,
        tr(s, "sessionBehavior.rememberWindowPosition"),
        s.remember_position,
        |s| s.toggle_remember_position(),
    ));
    rows = rows.child(switch_row(
        s,
        k,
        tr(s, "sessionBehavior.recordFeed"),
        s.config.record.unwrap_or(false),
        |s| {
            flip(&mut s.config.record, false);
            s.save_config();
        },
    ));
    if s.config.record.unwrap_or(false) {
        rows = rows.child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    text(
                        s.config
                            .record_path
                            .as_deref()
                            .unwrap_or_else(|| tr(s, "sessionBehavior.defaultVideosFolder")),
                    )
                    .font(k.ty.small)
                    .color(k.c.text_faint)
                    .truncate()
                    .grow(1.0),
                )
                .child(folder_button(
                    s,
                    k,
                    tr(s, "sessionBehavior.change"),
                    |s, path| {
                        s.set_record_path(path.to_string_lossy().to_string());
                    },
                )),
        );
    }
    rows = rows.child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(
                text(tr(s, "sessionBehavior.audioCodec"))
                    .font(k.ty.small)
                    .color(k.c.text_muted),
            )
            .child(select_control(
                s,
                k,
                layer,
                "audio",
                AUDIO_CODEC,
                s.config.audio_codec.as_deref().unwrap_or("auto"),
                tr(s, "sessionBehavior.audioCodec"),
                140.0,
            )),
    );
    section_card(
        k,
        "behavior",
        tr(s, "sessionBehavior.title"),
        Some(&icons::GEAR),
        vec![rows],
    )
}

fn shortcuts_card(s: &Ui, k: &Kit) -> El {
    let keys: [(&str, &str); 8] = [
        ("F", "shortcuts.full"),
        ("H", "shortcuts.home"),
        ("B", "shortcuts.back"),
        ("S", "shortcuts.recents"),
        ("P", "shortcuts.power"),
        ("R", "shortcuts.rotate"),
        ("V", "shortcuts.paste"),
        ("O", "shortcuts.off"),
    ];
    let mut rows = col().w_full().gap(4.0);
    for (key, label) in keys {
        rows = rows.child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    text(tr(s, label))
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .grow(1.0),
                )
                .child(rune_kit::Keybind::new(&["Alt", key]).build(k)),
        );
    }
    section_card(
        k,
        "shortcuts",
        tr(s, "shortcuts.title"),
        Some(&glyphs::KEYBOARD),
        vec![rows],
    )
}

fn sidebar(s: &Ui, k: &Kit) -> El {
    let mut sidebar = Sidebar::new().header(
        SidebarHeader::new(tr(s, "sidebar.deviceHub"))
            .action(
                Button::new(tr(s, "sidebar.refresh"))
                    .variant(ButtonVariant::GhostMuted)
                    .control_size(ControlSize::Small)
                    .disabled(s.is_refreshing)
                    .on_press(|s: &mut Ui| s.refresh_devices())
                    .build(k),
            )
            .build(k),
    );
    let mut devices = Vec::new();
    if s.devices.is_empty() {
        devices.push(
            SidebarRow::new(tr(s, "sidebar.noDevicesDetected"))
                .subtitle(tr(s, "sidebar.refresh"))
                .on_select(|s: &mut Ui, _| s.refresh_devices())
                .build(k)
                .key("no-devices"),
        );
    }
    for device in &s.devices {
        let serial = device.serial.clone();
        let transport = if serial.contains(':') {
            tr(s, "sidebar.wifi")
        } else {
            tr(s, "sidebar.usb")
        };
        let detail = format!("{} · {}", serial, transport);
        devices.push(
            SidebarRow::new(device.label())
                .subtitle(detail)
                .icon(&icons::PHONE)
                .selected(s.config.device == serial)
                .on_select(move |s: &mut Ui, _| s.select_device(serial.clone()))
                .build(k)
                .key(format!("device-{}", device.serial)),
        );
    }
    sidebar = sidebar.section(
        SidebarSection::new(tr(s, "sidebar.devices"))
            .count(s.devices.len())
            .rows(devices),
    );
    let mut recent = Vec::new();
    for ip in &s.history {
        let target = ip.clone();
        recent.push(
            SidebarRow::new(ip)
                .detail(tr(s, "sidebar.connect"))
                .on_select(move |s: &mut Ui, _| {
                    s.ip.replace_text(target.clone());
                    s.connect(target.clone());
                })
                .build(k)
                .key(format!("recent-{ip}")),
        );
    }
    if !recent.is_empty() {
        sidebar = sidebar.section(
            SidebarSection::new(tr(s, "sidebar.recentDevices"))
                .count(s.history.len())
                .rows(recent),
        );
    }
    sidebar = sidebar.footer(
        col()
            .w_full()
            .gap(2.0)
            .child(
                SidebarRow::new(tr(s, "sidebar.connect"))
                    .icon(&icons::MONITOR)
                    .on_select(|s: &mut Ui, _| s.connect_open = true)
                    .build(k)
                    .key("open-connect"),
            )
            .child(
                SidebarRow::new(tr(s, "sidebar.flashPushFiles"))
                    .icon(&icons::PASTE)
                    .on_select(|s: &mut Ui, tx: &mut rune::Tx| {
                        let opts = rune::FileDialogOptions {
                            title: tr(s, "sidebar.flashPushFiles").into(),
                            ..Default::default()
                        };
                        tx.open_file(opts, |s: &mut Ui, result, _| {
                            if let Ok(Some(path)) = result {
                                s.handle_file(path);
                            }
                        });
                    })
                    .build(k)
                    .key("push-files"),
            )
            .child(
                SidebarRow::new(tr(s, "footer.aboutScrcpyGui"))
                    .icon(&glyphs::FILE)
                    .on_select(|s: &mut Ui, _| s.about_open = true)
                    .build(k)
                    .key("about"),
            ),
    );
    sidebar.build(k)
}

fn toolbar(s: &Ui, k: &Kit, insets: rune::ChromeInsets, trigger: El) -> Toolbar {
    let dot = match s.scrcpy_ok {
        Some(true) => k.c.success.fg,
        Some(false) => k.c.danger.fg,
        None => k.c.warning.fg,
    };
    let status = if s.download.active {
        tv(
            &s.locale,
            "header.syncingComponents",
            &[("progress", &s.download.percent.to_string())],
        )
    } else if s.scrcpy_ok == Some(true) {
        tr(s, "header.scrcpyReady").to_string()
    } else {
        s.scrcpy_message.clone()
    };
    let mut trailing = row().gap(8.0).center_y();
    trailing = trailing.child(
        Button::new(tr(s, "sidebar.connect"))
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
            .on_press(|s: &mut Ui| s.connect_open = true)
            .build(k),
    );
    if s.scrcpy_ok == Some(false) {
        trailing = trailing.child(
            Button::new(tr(s, "header.installCore"))
                .variant(ButtonVariant::Contrast)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut Ui| s.download_scrcpy())
                .build(k),
        );
    } else {
        trailing = trailing.child(
            Button::new(tr(s, "header.selectFolder"))
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press_tx(move |s: &mut Ui, tx: &mut rune::Tx| {
                    let opts = rune::FileDialogOptions {
                        title: tr(s, "header.selectFolder").into(),
                        ..Default::default()
                    };
                    tx.open_folder(opts, |s: &mut Ui, result, _| {
                        if let Ok(Some(path)) = result {
                            s.set_scrcpy_path(Some(path.to_string_lossy().to_string()));
                        }
                    });
                })
                .build(k),
        );
    }
    let trigger = row()
        .mr(insets.trailing)
        .gap(8.0)
        .center_y()
        .child(
            row()
                .gap(6.0)
                .center_y()
                .child(el().size(7.0, 7.0).radius(3.5).fill(dot))
                .child({
                    let mut line = row().gap(6.0).center_y();
                    if s.download.active {
                        line = line.child(
                            el().key("sync-spinner")
                                .size(12.0, 12.0)
                                .center()
                                .child(Spinner::new().size(12.0).color(k.c.success.fg).build(k)),
                        );
                    }
                    line.child(
                        text(status.clone())
                            .font(k.ty.small)
                            .color(k.c.text_muted)
                            .truncate()
                            .help(status),
                    )
                }),
        )
        .child(trailing)
        .child(trigger);
    Toolbar::new()
        .leading(text("ScrcpyGUI").font(k.ty.ui_medium).color(k.c.text_base))
        .trailing(trigger)
}

fn connect_dialog(s: &Ui, k: &Kit, layer: &mut Vec<El>) {
    let ip = Field::new()
        .label(tr(s, "ui.ipAddress"))
        .control(
            TextInput::new(&s.ip)
                .label(tr(s, "ui.ipAddress"))
                .placeholder(tr(s, "sidebar.ipPlaceholder"))
                .width(None)
                .on_edit(|s: &mut Ui| &mut s.ip)
                .on_submit(|s: &mut Ui, _| {
                    let ip = s.ip.text.clone();
                    s.connect(ip);
                })
                .build(k)
                .key("connect-ip"),
        )
        .build(k);
    let mut mdns = col().w_full().gap(2.0).max_h(120.0).scroll(ScrollSpec {
        stick_bottom: false,
    });
    for service in &s.mdns {
        let address = service.address.clone();
        let label = format!("{}  ·  {}", service.name, address);
        mdns = mdns.child(
            Button::new(label)
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut Ui| {
                    s.ip.replace_text(address.clone());
                })
                .build(k),
        );
    }
    let pair_ip = Field::new()
        .label(tr(s, "ui.ipAddress"))
        .control(
            TextInput::new(&s.pair_ip)
                .label(tr(s, "ui.ipAddress"))
                .placeholder(tr(s, "sidebar.ipPlaceholder"))
                .width(None)
                .on_edit(|s: &mut Ui| &mut s.pair_ip)
                .build(k)
                .key("pair-ip"),
        )
        .build(k);
    let pair_code = Field::new()
        .label(tr(s, "ui.pairingCode"))
        .control(
            TextInput::new(&s.pair_code)
                .label(tr(s, "ui.pairingCode"))
                .placeholder(tr(s, "sidebar.pairingCodePlaceholder"))
                .width(None)
                .on_edit(|s: &mut Ui| &mut s.pair_code)
                .build(k)
                .key("pair-code"),
        )
        .build(k);
    let body = col()
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .child(
            SegmentedControl::new(
                &[tr(s, "sidebar.usb"), tr(s, "sidebar.wifi")],
                Some(if s.transport == Transport::Usb { 0 } else { 1 }),
            )
            .label(tr(s, "sidebar.deviceHub"))
            .width(None)
            .on_pick(|s: &mut Ui, i, _| {
                s.transport = if i == 0 {
                    Transport::Usb
                } else {
                    Transport::Wifi
                };
            })
            .build(k),
        )
        .child(
            row()
                .w_full()
                .gap(8.0)
                .items_end()
                .child(col().grow(1.0).min_w(0.0).child(ip))
                .child(
                    Button::new(if s.connecting {
                        tr(s, "sidebar.connecting")
                    } else {
                        tr(s, "sidebar.connect")
                    })
                    .variant(ButtonVariant::Contrast)
                    .control_size(ControlSize::Large)
                    .disabled(s.ip.text.trim().is_empty() || s.connecting)
                    .on_press(|s: &mut Ui| {
                        let ip = s.ip.text.clone();
                        s.connect(ip);
                    })
                    .build(k)
                    .key("connect-submit"),
                ),
        )
        .child(
            text(tr(s, "sidebar.discoveredDevices"))
                .font(k.ty.small_medium)
                .color(k.c.text_muted),
        )
        .child(mdns)
        .child(
            text(tr(s, "sidebar.pairDeviceTitle"))
                .font(k.ty.small_medium)
                .color(k.c.text_muted),
        )
        .child(row().w_full().gap(8.0).children([
            col().grow(1.0).min_w(0.0).child(pair_ip),
            col().grow(1.0).min_w(0.0).child(pair_code),
        ]))
        .child(
            Button::new(if s.pairing {
                tr(s, "sidebar.synchronizing")
            } else {
                tr(s, "sidebar.startPairing")
            })
            .variant(ButtonVariant::Neutral)
            .control_size(ControlSize::Regular)
            .disabled(
                s.pair_ip.text.trim().is_empty() || s.pair_code.text.trim().is_empty() || s.pairing,
            )
            .on_press(|s: &mut Ui| {
                let ip = s.pair_ip.text.clone();
                let code = s.pair_code.text.clone();
                s.pair(ip, code);
            })
            .build(k)
            .key("pair-submit"),
        );
    Dialog::new("connect", tr(s, "sidebar.connect"))
        .open(s.connect_open)
        .width(520.0)
        .fit()
        .on_close(|s: &mut Ui| s.connect_open = false)
        .child(body)
        .build(k, layer);
}

fn about_dialog(s: &Ui, k: &Kit, layer: &mut Vec<El>) {
    Dialog::new("about", tr(s, "footer.aboutScrcpyGui"))
        .open(s.about_open)
        .width(420.0)
        .fit()
        .on_close(|s: &mut Ui| s.about_open = false)
        .child(
            col()
                .w_full()
                .padding(16.0)
                .gap(8.0)
                .child(
                    text(format!("ScrcpyGUI v{}", env!("CARGO_PKG_VERSION")))
                        .font(k.ty.body_medium)
                        .color(k.c.text_base),
                )
                .child(
                    text("Core: scrcpy · UI: Rune · Rust")
                        .font(k.ty.small)
                        .color(k.c.text_muted),
                ),
        )
        .build(k, layer);
}

fn end_adb_dialog(s: &Ui, k: &Kit, layer: &mut Vec<El>) {
    Dialog::new("end-adb", tr(s, "ui.endAdb"))
        .open(s.confirm_kill_adb)
        .width(400.0)
        .fit()
        .on_close(|s: &mut Ui| s.confirm_kill_adb = false)
        .child(
            col().w_full().padding(16.0).child(
                Button::new(tr(s, "ui.end"))
                    .variant(ButtonVariant::Danger)
                    .control_size(ControlSize::Regular)
                    .on_press(|s: &mut Ui| {
                        s.confirm_kill_adb = false;
                        s.kill_adb();
                    })
                    .build(k),
            ),
        )
        .build(k, layer);
}

fn update_dialog(s: &Ui, k: &Kit, layer: &mut Vec<El>) {
    let Some(info) = &s.update else { return };
    let body = col()
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .child(
            text(tv(
                &s.locale,
                "alerts.updateAvailableMessage",
                &[("latest", &info.latest), ("local", &info.local)],
            ))
            .font(k.ty.small)
            .color(k.c.text_muted)
            .wrap(),
        )
        .child(
            row()
                .gap(8.0)
                .child(
                    Button::new(tr(s, "alerts.updateBtn"))
                        .variant(ButtonVariant::Contrast)
                        .on_press(|s: &mut Ui| s.update_now())
                        .build(k),
                )
                .child(
                    Button::new(tr(s, "alerts.cancelBtn"))
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut Ui| s.update = None)
                        .build(k),
                ),
        );
    Dialog::new("update", tr(s, "alerts.updateAvailableTitle"))
        .open(true)
        .width(460.0)
        .fit()
        .on_close(|s: &mut Ui| s.update = None)
        .child(body)
        .build(k, layer);
}

fn onboarding_dialog(s: &Ui, k: &Kit, layer: &mut Vec<El>) {
    if !(s.onboarding_open && s.scrcpy_ok == Some(false)) {
        return;
    }
    let body = col()
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .child(
            text(tr(s, "onboarding.binariesDescription"))
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap(),
        )
        .child(
            row()
                .gap(8.0)
                .child(
                    Button::new(tr(s, "onboarding.downloadCoreBinaries"))
                        .variant(ButtonVariant::Contrast)
                        .on_press(|s: &mut Ui| s.download_scrcpy())
                        .build(k),
                )
                .child(
                    Button::new(tr(s, "onboarding.continueToApp"))
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut Ui| s.onboarding_open = false)
                        .build(k),
                ),
        );
    Dialog::new("onboarding", tr(s, "onboarding.coreInitialization"))
        .open(true)
        .width(520.0)
        .fit()
        .on_close(|s: &mut Ui| s.onboarding_open = false)
        .child(body)
        .build(k, layer);
}

fn alert_dialog(s: &Ui, k: &Kit, layer: &mut Vec<El>) {
    let Some(message) = &s.alert else { return };
    Dialog::new("alert", tr(s, "common.ok"))
        .open(true)
        .width(420.0)
        .fit()
        .on_close(|s: &mut Ui| s.alert = None)
        .child(
            col()
                .w_full()
                .padding(16.0)
                .gap(8.0)
                .child(text(message).font(k.ty.small).color(k.c.text_muted).wrap())
                .child(
                    Button::new(tr(s, "common.ok"))
                        .variant(ButtonVariant::Contrast)
                        .on_press(|s: &mut Ui| s.alert = None)
                        .build(k),
                ),
        )
        .build(k, layer);
}

pub fn view(s: &Ui, cx: &mut Cx<Ui>) -> El {
    let system_dark = cx.system_dark();
    let dark = s.resolved_dark(system_dark);
    let mut kit = theme::kit(dark).clone();
    // Dialogs, menus and select popups use `layer_01`; keep it opaque so the
    // page behind doesn't read through the panel.
    kit.c.layer_01 = Color {
        a: 1.0,
        ..kit.c.layer_01
    };
    let k: &Kit = &kit;
    let insets = cx.chrome_insets();
    let mut layer: Vec<El> = Vec::new();

    let menu_trigger = Button::new("⋯")
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .on_press(|s: &mut Ui| s.app_menu = true)
        .build(k)
        .key("app-menu-trigger");
    let app_menu = Menu::new("app")
        .trigger(menu_trigger)
        .open(s.app_menu)
        .min_width(200.0)
        .item(
            MenuItem::new(tr(s, "sidebar.refresh")).on_select(|s: &mut Ui| {
                s.app_menu = false;
                s.refresh_devices();
            }),
        )
        .item(
            MenuItem::new(tr(s, "sidebar.connect")).on_select(|s: &mut Ui| {
                s.app_menu = false;
                s.connect_open = true;
            }),
        )
        .item(
            MenuItem::new(tr(s, "ui.endAdb"))
                .danger(true)
                .on_select(|s: &mut Ui| {
                    s.app_menu = false;
                    s.confirm_kill_adb = true;
                }),
        )
        .separator()
        .item(
            MenuItem::new(tr(s, "footer.aboutScrcpyGui")).on_select(|s: &mut Ui| {
                s.app_menu = false;
                s.about_open = true;
            }),
        )
        .on_close(|s: &mut Ui| s.app_menu = false)
        .build(k, &mut layer);

    let sidebar = sidebar(s, k);
    let main = main_view(s, k, &mut layer);
    let inspector = col()
        .key("inspector")
        .w_full()
        .gap(16.0)
        .padding(16.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(appearance_card(s, k, &mut layer, system_dark))
        .child(behavior_card(s, k, &mut layer))
        .child(shortcuts_card(s, k));

    let toolbar = toolbar(s, k, insets, app_menu);
    let shell = SplitView::new(main)
        .sidebar(&s.sidebar, |s: &mut Ui| &mut s.sidebar, sidebar)
        .inspector(&s.inspector, |s: &mut Ui| &mut s.inspector, inspector)
        .toolbar(toolbar)
        .titlebar_inset(cx.titlebar_inset())
        .build(k)
        .on_drop_files(|s: &mut Ui, paths: &[std::path::PathBuf], _| {
            if s.config.device.is_empty() {
                s.log(t(&s.locale, "logs.noDeviceForDragDrop").to_string());
                return;
            }
            for path in paths {
                s.handle_file(path.clone());
            }
        });

    connect_dialog(s, k, &mut layer);
    about_dialog(s, k, &mut layer);
    end_adb_dialog(s, k, &mut layer);
    update_dialog(s, k, &mut layer);
    onboarding_dialog(s, k, &mut layer);
    alert_dialog(s, k, &mut layer);
    if let Some(note) = &s.toast {
        layer.push(
            ToastRegion::new()
                .toast(
                    Toast::new("app-toast", note.title.clone())
                        .description(note.description.clone())
                        .on_close(|s: &mut Ui| s.toast = None)
                        .build(k),
                )
                .build(k),
        );
    }
    root(shell, layer)
}
