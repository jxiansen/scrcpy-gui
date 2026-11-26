//! Official rune-kit shell for ScrcpyGUI.
//!
//! The shell, sidebar, toolbar, cards and form controls come from rune-kit with
//! the OpenCode theme. The console, file drop zone and scrcpy-specific icons stay
//! custom because the kit has no equivalent.

use rune::{Cx, El, LiveRegion, ScrollSpec, col, el, row, text};
use rune_kit::glyphs;
use rune_kit::{
    Button, ButtonVariant, ControlSize, Dialog, Field, IconButton, IconButtonVariant, Keybind, Kit,
    Placement, SegmentedControl, Select, Sidebar, SidebarHeader, SidebarRow, SidebarSection,
    Slider, Spinner, Switch, TextInput, Toast, ToastRegion, root,
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
        // Left-aligned so anchored_fit has room for the option labels; the
        // default BottomEnd collapses to a sliver next to the right edge.
        .placement(Placement::BottomStart)
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
    col().grow(1.0).min_w(300.0).child(content)
}

fn grid(fields: Vec<El>) -> El {
    row().w_full().gap(16.0).flex_wrap().children(fields)
}

/// A settings row: 13/20 label with the 36x16 switch at the trailing edge.
fn switch_row(k: &Kit, label: &str, on: bool, f: impl Fn(&mut Ui) + 'static) -> El {
    row()
        .w_full()
        .h(28.0)
        .gap(8.0)
        .center_y()
        .child(
            text(label.to_string())
                .font(k.ty.ui)
                .color(k.c.text_base)
                .grow(1.0)
                .shrink()
                .truncate(),
        )
        .child(
            Switch::new(on)
                .label(label)
                .hide_label()
                .on_toggle(f)
                .build(k),
        )
}

fn labeled_slider(k: &Kit, label: &str, value: String, slider: El) -> El {
    col()
        .w_full()
        .gap(8.0)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(text(label).font(k.ty.ui_medium).color(k.c.text_base))
                .child(row().grow(1.0))
                .child(text(value).font(k.ty.small).color(k.c.text_muted)),
        )
        .child(slider)
}

const SELECT_W: f32 = 340.0;
const SLIDER_W: f32 = 300.0;

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

fn flat_card(k: &Kit, title: &str, icon: &'static rune::Icon, sub: &str, body: El) -> El {
    let head = row()
        .key("title")
        .w_full()
        .gap(8.0)
        .center_y()
        .child(rune::icon(icon, 14.0).color(k.c.icon_base))
        .child(
            text(title.to_string())
                .font(k.ty.body_medium)
                .color(k.c.text_base),
        );
    let inner = col()
        .grow(1.0)
        .shrink()
        .pt(10.0)
        .pb(10.0)
        .pl(8.0)
        .pr(12.0)
        .child(head)
        .child(
            text(sub.to_string())
                .font(k.ty.body)
                .color(k.c.text_muted)
                .wrap(),
        )
        .child(col().w_full().mt(10.0).gap(12.0).grow(1.0).child(body));
    row()
        .key("settings-card")
        // Switching tabs mounts different Select/Slider nodes. Without this,
        // their default presence fade overlaps the old ones on the same rect
        // for ~0.25 s and the text looks blurry. Tab indicator lives outside
        // this card, so it still animates.
        .instant_changes()
        .w_full()
        .grow(1.0)
        .items_stretch()
        .mt(16.0)
        .child(el().w(2.0).mt(8.0).mb(8.0).radius(2.0).fill(k.c.icon_base))
        .child(inner)
}

fn section_heading(k: &Kit, title: &str) -> El {
    text(title.to_string())
        .font(k.ty.small_medium)
        .color(k.c.text_faint)
}

fn select_field(
    s: &Ui,
    k: &Kit,
    layer: &mut Vec<El>,
    key: &'static str,
    options: &[&str],
    current: &str,
    label: &'static str,
) -> El {
    field(
        tr(s, label),
        select_control(s, k, layer, key, options, current, tr(s, label), SELECT_W),
        k,
    )
}

fn bitrate_field(s: &Ui, k: &Kit) -> El {
    let slider = Slider::new("bitrate", s.config.bitrate.unwrap_or(8) as f32)
        .range(1.0..=50.0)
        .step(1.0)
        .label(tr(s, "controlPanel.bitrate"))
        .hide_label()
        .width(SLIDER_W)
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

fn zoom_field(s: &Ui, k: &Kit) -> El {
    let value = s.config.camera_zoom.unwrap_or(1.0);
    let slider = Slider::new("zoom", value)
        .range(1.0..=5.0)
        .step(0.1)
        .label(tr(s, "controlPanel.cameraZoom"))
        .hide_label()
        .width(SLIDER_W)
        .on_change(|s: &mut Ui, v, _| {
            s.config.camera_zoom = Some(v);
            s.save_config();
        })
        .build(k);
    labeled_slider(
        k,
        tr(s, "controlPanel.cameraZoom"),
        format!("{value:.1}x"),
        slider,
    )
}

fn dimension_slider(
    s: &Ui,
    k: &Kit,
    key: &'static str,
    label: &'static str,
    value: u32,
    range: std::ops::RangeInclusive<f32>,
    unit: &'static str,
    set: fn(&mut Ui, u32),
) -> El {
    let slider = Slider::new(key, value as f32)
        .range(range)
        .step(10.0)
        .label(tr(s, label))
        .hide_label()
        .width(SLIDER_W)
        .on_change(move |s: &mut Ui, v, _| {
            set(s, v.round() as u32);
            s.save_config();
        })
        .build(k);
    labeled_slider(k, tr(s, label), format!("{value}{unit}"), slider)
}

fn essential_fields(s: &Ui, k: &Kit, layer: &mut Vec<El>) -> Vec<El> {
    match s.tab {
        SessionKind::Mirror => vec![
            cell(select_field(
                s,
                k,
                layer,
                "res",
                RES,
                s.config.res.as_deref().unwrap_or("0"),
                "controlPanel.resolution",
            )),
            cell(select_field(
                s,
                k,
                layer,
                "fps",
                FPS,
                &s.config
                    .fps
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "Auto".into()),
                "controlPanel.fps",
            )),
            cell(bitrate_field(s, k)),
            cell(select_field(
                s,
                k,
                layer,
                "codec",
                CODEC,
                s.config.codec.as_deref().unwrap_or("h264"),
                "controlPanel.codec",
            )),
        ],
        SessionKind::Camera => vec![
            cell(select_field(
                s,
                k,
                layer,
                "lens",
                LENSES,
                s.config.camera_id.as_deref().unwrap_or("Auto Select"),
                "controlPanel.cameraDevice",
            )),
            cell(select_field(
                s,
                k,
                layer,
                "aspect",
                ASPECT,
                s.config.camera_ar.as_deref().unwrap_or("0"),
                "controlPanel.aspect",
            )),
            cell(zoom_field(s, k)),
            cell(select_field(
                s,
                k,
                layer,
                "fps",
                FPS,
                &s.config
                    .fps
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "Auto".into()),
                "controlPanel.fps",
            )),
        ],
        SessionKind::Desktop => vec![
            cell(dimension_slider(
                s,
                k,
                "vdWidth",
                "controlPanel.width",
                s.config.vd_width.unwrap_or(1920),
                480.0..=3840.0,
                " px",
                |s, v| s.config.vd_width = Some(v),
            )),
            cell(dimension_slider(
                s,
                k,
                "vdHeight",
                "controlPanel.height",
                s.config.vd_height.unwrap_or(1080),
                360.0..=2160.0,
                " px",
                |s, v| s.config.vd_height = Some(v),
            )),
            cell(dimension_slider(
                s,
                k,
                "vdDpi",
                "controlPanel.uiScaling",
                s.config.vd_dpi.unwrap_or(420),
                120.0..=640.0,
                "",
                |s, v| s.config.vd_dpi = Some(v),
            )),
            cell(select_field(
                s,
                k,
                layer,
                "preset",
                PRESETS,
                preset_name(
                    s.config.vd_width.unwrap_or(1920),
                    s.config.vd_height.unwrap_or(1080),
                ),
                "controlPanel.quickPresets",
            )),
        ],
    }
}

fn advanced_selects(s: &Ui, k: &Kit, layer: &mut Vec<El>) -> Vec<El> {
    let mut fields = Vec::new();
    if matches!(s.tab, SessionKind::Desktop) {
        fields.push(cell(select_field(
            s,
            k,
            layer,
            "fps",
            FPS,
            &s.config
                .fps
                .map(|v| v.to_string())
                .unwrap_or_else(|| "Auto".into()),
            "controlPanel.fps",
        )));
    }
    fields.push(cell(select_field(
        s,
        k,
        layer,
        "rotation",
        ROT,
        s.config.rotation.as_deref().unwrap_or("0"),
        "controlPanel.rotation",
    )));
    fields.push(cell(select_field(
        s,
        k,
        layer,
        "renderer",
        RENDERERS,
        s.config.render_driver.as_deref().unwrap_or("Auto"),
        "controlPanel.graphicsRenderer",
    )));
    fields.push(cell(select_field(
        s,
        k,
        layer,
        "hwdec",
        HWDEC,
        s.config.hwdec.as_deref().unwrap_or("auto"),
        "controlPanel.hardwareDecoding",
    )));
    fields
}

fn advanced_switches(s: &Ui, k: &Kit) -> El {
    let mut cells = Vec::new();
    cells.push(col().grow(1.0).min_w(0.0).child(switch_row(
        k,
        tr(s, "controlPanel.vsync"),
        s.config.vsync.unwrap_or(true),
        |s| {
            flip(&mut s.config.vsync, true);
            s.save_config();
        },
    )));
    cells.push(col().grow(1.0).min_w(0.0).child(switch_row(
        k,
        tr(s, "controlPanel.ignoreEncoderConstraints"),
        s.config.ignore_video_encoder_constraints.unwrap_or(false),
        |s| {
            flip(&mut s.config.ignore_video_encoder_constraints, false);
            s.save_config();
        },
    )));
    if matches!(s.tab, SessionKind::Desktop) {
        cells.push(col().grow(1.0).min_w(0.0).child(switch_row(
            k,
            tr(s, "controlPanel.flexDisplay"),
            s.config.flex_display.unwrap_or(false),
            |s| {
                flip(&mut s.config.flex_display, false);
                s.save_config();
            },
        )));
    }
    row().w_full().gap(16.0).flex_wrap().children(cells)
}

fn advanced_content(s: &Ui, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut content = col()
        .w_full()
        .gap(12.0)
        .child(grid(advanced_selects(s, k, layer)));
    if matches!(s.tab, SessionKind::Desktop) {
        content = content
            .child(bitrate_field(s, k))
            .child(background_color(s, k, &|key| tr(s, key)));
    } else if matches!(s.tab, SessionKind::Camera) {
        content = content.child(bitrate_field(s, k));
    }
    content.child(advanced_switches(s, k))
}

fn advanced_count(s: &Ui) -> usize {
    // The badge mirrors the advanced body per tab: `advanced_selects` builds the
    // selector row, `advanced_content` adds bitrate/background rows and
    // `advanced_switches` the toggles. Update this total together with them.
    match s.tab {
        SessionKind::Mirror => 5,
        SessionKind::Camera => 7,
        SessionKind::Desktop => 10,
    }
}

fn input_switches(s: &Ui, k: &Kit) -> El {
    let mut cells = Vec::new();
    match s.tab {
        SessionKind::Mirror => {
            cells.push(col().grow(1.0).min_w(0.0).child(switch_row(
                k,
                tr(s, "controlPanel.hidKeyboard"),
                s.config.hid_keyboard.unwrap_or(false),
                |s| {
                    flip(&mut s.config.hid_keyboard, false);
                    s.save_config();
                },
            )));
            cells.push(col().grow(1.0).min_w(0.0).child(switch_row(
                k,
                tr(s, "controlPanel.hidMouse"),
                s.config.hid_mouse.unwrap_or(false),
                |s| {
                    flip(&mut s.config.hid_mouse, false);
                    s.save_config();
                },
            )));
            cells.push(col().grow(1.0).min_w(0.0).child(switch_row(
                k,
                tr(s, "controlPanel.pureHid"),
                s.config.otg_pure.unwrap_or(false),
                |s| {
                    flip(&mut s.config.otg_pure, false);
                    s.save_config();
                },
            )));
        }
        SessionKind::Camera => {
            cells.push(col().grow(1.0).min_w(0.0).child(switch_row(
                k,
                tr(s, "controlPanel.cameraTorch"),
                s.config.camera_torch.unwrap_or(false),
                |s| {
                    flip(&mut s.config.camera_torch, false);
                    s.save_config();
                },
            )));
            cells.push(col().grow(1.0).min_w(0.0).child(switch_row(
                k,
                tr(s, "sessionBehavior.forwardAudio"),
                s.config.audio_enabled.unwrap_or(true),
                |s| {
                    flip(&mut s.config.audio_enabled, true);
                    s.save_config();
                },
            )));
            cells.push(col().grow(1.0).min_w(0.0).child(switch_row(
                k,
                tr(s, "sessionBehavior.stayAwake"),
                s.config.stay_awake.unwrap_or(false),
                |s| {
                    flip(&mut s.config.stay_awake, false);
                    s.save_config();
                },
            )));
        }
        SessionKind::Desktop => {
            cells.push(col().grow(1.0).min_w(0.0).child(switch_row(
                k,
                tr(s, "controlPanel.hidKeyboard"),
                s.config.hid_keyboard.unwrap_or(false),
                |s| {
                    flip(&mut s.config.hid_keyboard, false);
                    s.save_config();
                },
            )));
            cells.push(col().grow(1.0).min_w(0.0).child(switch_row(
                k,
                tr(s, "controlPanel.hidMouse"),
                s.config.hid_mouse.unwrap_or(false),
                |s| {
                    flip(&mut s.config.hid_mouse, false);
                    s.save_config();
                },
            )));
            cells.push(col().grow(1.0).min_w(0.0).child(switch_row(
                k,
                tr(s, "sessionBehavior.forwardAudio"),
                s.config.audio_enabled.unwrap_or(true),
                |s| {
                    flip(&mut s.config.audio_enabled, true);
                    s.save_config();
                },
            )));
        }
    }
    row().w_full().gap(16.0).flex_wrap().children(cells)
}

fn settings_card(s: &Ui, k: &Kit, layer: &mut Vec<El>) -> El {
    let icon = match s.tab {
        SessionKind::Mirror => &icons::SLIDERS,
        SessionKind::Camera => &icons::CAMERA,
        SessionKind::Desktop => &icons::MONITOR,
    };
    let sub = match s.tab {
        SessionKind::Mirror => tr(s, "settings.cardSubMirror"),
        SessionKind::Camera => tr(s, "settings.cardSubCamera"),
        SessionKind::Desktop => tr(s, "settings.cardSubDesktop"),
    };
    // Keep the actual controls compact at the top of the card and pin the
    // advanced entry to the bottom; the space between is intentional breathing
    // room, not a stretched-out grid of fields.
    let fields = col()
        .w_full()
        .gap(18.0)
        .child(grid(essential_fields(s, k, layer)))
        .child(
            col()
                .w_full()
                .gap(10.0)
                .child(section_heading(k, tr(s, "settings.inputGroup")))
                .child(input_switches(s, k)),
        )
        .child(el().h(1.0).w_full().fill(k.c.border_muted));
    let mut body = col().w_full().grow(1.0).justify_start().child(fields);
    if s.advanced_open {
        body = body.child(advanced_content(s, k, layer));
    }
    body = body.child(
        Button::new(format!(
            "{} · {}",
            tr(s, "advanced.title"),
            advanced_count(s)
        ))
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Regular)
        .icon(&icons::SLIDERS)
        .expanded(s.advanced_open)
        .on_press(|s: &mut Ui| s.advanced_open = !s.advanced_open)
        .build(k)
        .justify_start()
        .key("advanced-toggle"),
    );
    flat_card(
        k,
        tr(s, "controlPanel.engineConfiguration"),
        icon,
        sub,
        body,
    )
}

fn mode_row(s: &Ui, k: &Kit) -> El {
    let selected = s.tab as usize;
    let segmented = SegmentedControl::new(
        &[
            tr(s, "controlPanel.screen"),
            tr(s, "controlPanel.camera"),
            tr(s, "controlPanel.desktop"),
        ],
        Some(selected),
    )
    .label(tr(s, "controlPanel.captureSource"))
    .width(Some(360.0))
    .on_pick(|s: &mut Ui, i, _| {
        s.tab = match i {
            0 => SessionKind::Mirror,
            1 => SessionKind::Camera,
            _ => SessionKind::Desktop,
        };
        s.advanced_open = false;
    })
    .build(k);
    let mut bar = row()
        .key("mode-row")
        .w_full()
        .gap(16.0)
        .center_y()
        .mb(16.0)
        .child(segmented)
        .child(row().grow(1.0));
    if s.running.get(&s.config.device).copied().unwrap_or(false) {
        bar = bar.child(
            text(tr(s, "device.modeNote"))
                .font(k.ty.small)
                .color(k.c.text_faint)
                .truncate()
                .help(tr(s, "device.modeNote")),
        );
    }
    bar
}

fn has_device(s: &Ui) -> bool {
    !s.config.device.is_empty()
        && (s.devices.iter().any(|d| d.serial == s.config.device)
            || s.running.contains_key(&s.config.device))
}

fn device_badge(k: &Kit, label: &str) -> El {
    row()
        .h(20.0)
        .px(7.0)
        .gap(5.0)
        .center_y()
        .radius(4.0)
        .fill(k.c.success.bg)
        .border(0.5, k.c.success.border)
        .child(el().size(5.0, 5.0).radius(2.5).fill(k.c.success.fg))
        .child(
            text(label.to_string())
                .font(k.ty.small_medium)
                .color(k.c.success.fg),
        )
}

fn device_head(s: &Ui, k: &Kit) -> El {
    let connected = has_device(s);
    let running = s.running.get(&s.config.device).copied().unwrap_or(false);
    let avatar = el()
        .size(36.0, 36.0)
        .radius(8.0)
        .fill(k.c.layer_02)
        .center()
        .child(rune::icon(&icons::PHONE, 18.0).color(k.c.icon_base));
    let row = if !connected {
        row()
            .w_full()
            .h(36.0)
            .gap(12.0)
            .center_y()
            .child(avatar)
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(
                        text(tr(s, "device.emptyTitle"))
                            .font(k.ty.body_medium)
                            .color(k.c.text_base),
                    )
                    .child(
                        text(tr(s, "device.emptyBody"))
                            .font(k.ty.small)
                            .color(k.c.text_muted)
                            .truncate(),
                    ),
            )
            .child(
                Button::new(tr(s, "sidebar.connect"))
                    .variant(ButtonVariant::Submit)
                    .control_size(ControlSize::Regular)
                    .on_press(|s: &mut Ui| s.connect_open = true)
                    .build(k)
                    .key("device-connect"),
            )
    } else {
        let name = s
            .devices
            .iter()
            .find(|d| d.serial == s.config.device)
            .map(|d| d.label().to_string())
            .unwrap_or_else(|| s.config.device.clone());
        let transport = if s.config.device.contains(':') {
            tr(s, "sidebar.wifi")
        } else {
            tr(s, "sidebar.usb")
        };
        row()
            .w_full()
            .h(36.0)
            .gap(12.0)
            .center_y()
            .child(avatar)
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(text(name).font(k.ty.body_medium).color(k.c.text_base))
                    .child(
                        text(format!("{} · {transport}", s.config.device))
                            .font(k.ty.small)
                            .color(k.c.text_faint)
                            .truncate(),
                    ),
            )
            .child(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(device_badge(
                        k,
                        if running {
                            tr(s, "device.live")
                        } else {
                            tr(s, "sidebar.ready")
                        },
                    ))
                    .child(
                        Button::new(tr(s, "device.disconnect"))
                            .variant(ButtonVariant::GhostMuted)
                            .control_size(ControlSize::Regular)
                            .on_press(|s: &mut Ui| s.request_disconnect())
                            .build(k)
                            .key("device-disconnect"),
                    ),
            )
    };
    col()
        .key("device-head")
        .w_full()
        .gap(14.0)
        .child(row)
        .child(el().h(1.0).w_full().fill(k.c.border_muted))
}

fn action_summary(s: &Ui, k: &Kit) -> El {
    let empty = !has_device(s);
    let running = s.running.get(&s.config.device).copied().unwrap_or(false);
    let name = if empty {
        tr(s, "ui.noDevice").to_string()
    } else {
        s.devices
            .iter()
            .find(|d| d.serial == s.config.device)
            .map(|d| d.label().to_string())
            .unwrap_or_else(|| s.config.device.clone())
    };
    let mut summary = row()
        .grow(1.0)
        .min_w(0.0)
        .gap(6.0)
        .center_y()
        .child(rune::icon(&icons::PHONE, 12.0).color(k.c.icon_muted))
        .child(
            text(name)
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .truncate(),
        );
    if !empty {
        let transport = if s.config.device.contains(':') {
            tr(s, "sidebar.wifi")
        } else {
            tr(s, "sidebar.usb")
        };
        summary = summary.child(text(transport).font(k.ty.small).color(k.c.text_faint));
        let chips: Vec<String> = match s.tab {
            SessionKind::Mirror => vec![
                display_option(&s.locale, "res", s.config.res.as_deref().unwrap_or("0")),
                display_option(
                    &s.locale,
                    "fps",
                    &s.config
                        .fps
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "Auto".into()),
                ),
                s.config.codec.clone().unwrap_or_else(|| "h264".into()),
            ],
            SessionKind::Camera => vec![
                display_option(
                    &s.locale,
                    "lens",
                    s.config.camera_id.as_deref().unwrap_or("Auto Select"),
                ),
                display_option(
                    &s.locale,
                    "aspect",
                    s.config.camera_ar.as_deref().unwrap_or("0"),
                ),
                s.config.codec.clone().unwrap_or_else(|| "h264".into()),
            ],
            SessionKind::Desktop => vec![
                format!(
                    "{}×{}",
                    s.config.vd_width.unwrap_or(1920),
                    s.config.vd_height.unwrap_or(1080)
                ),
                format!("{} DPI", s.config.vd_dpi.unwrap_or(420)),
                s.config.codec.clone().unwrap_or_else(|| "h264".into()),
            ],
        };
        for chip in chips {
            summary = summary
                .child(text("·").font(k.ty.small).color(k.c.text_faint))
                .child(text(chip).font(k.ty.small).color(k.c.text_muted));
        }
    }
    if running {
        summary = summary.child(
            row()
                .gap(5.0)
                .center_y()
                .child(el().size(5.0, 5.0).radius(2.5).fill(k.c.success.fg))
                .child(
                    text(tr(s, "device.live"))
                        .font(k.ty.small_medium)
                        .color(k.c.success.fg),
                ),
        );
    }
    summary
}

fn actionbar(s: &Ui, k: &Kit) -> El {
    let connected = has_device(s);
    let running = s.running.get(&s.config.device).copied().unwrap_or(false);
    let empty = !connected;
    let label = if empty {
        tr(s, "device.select")
    } else if running {
        tr(s, "controlPanel.stopSession")
    } else {
        match s.tab {
            SessionKind::Mirror => tr(s, "controlPanel.startMission"),
            SessionKind::Camera => tr(s, "controlPanel.engageCamera"),
            SessionKind::Desktop => tr(s, "controlPanel.ejectToDesktop"),
        }
    };
    let device = s.config.device.clone();
    let button = Button::new(label)
        .variant(if running {
            ButtonVariant::Danger
        } else {
            ButtonVariant::Submit
        })
        .control_size(ControlSize::ExtraLarge)
        .disabled(empty)
        .on_press(move |s: &mut Ui| {
            if running {
                s.stop_session(&device);
            } else {
                s.start_session();
            }
        })
        .build(k)
        .min_w(120.0)
        .key("start-stop");
    col()
        .key("actionbar")
        .w_full()
        .child(el().h(1.0).w_full().fill(k.c.border_muted))
        .child(
            row()
                .w_full()
                .h(60.0)
                .px(32.0)
                .justify_center()
                .center_y()
                .child(
                    row()
                        .w(720.0)
                        .gap(12.0)
                        .center_y()
                        .child(action_summary(s, k))
                        .child(button),
                ),
        )
}

fn console_strip(s: &Ui, k: &Kit) -> El {
    let preview = s
        .logs
        .last()
        .map(|(_, line)| line.clone())
        .unwrap_or_default();
    let preview_text = text(preview)
        .font(k.ty.mono)
        .color(k.c.text_faint)
        .truncate()
        .grow(1.0);
    let preview_text = if s.console_open {
        preview_text
    } else {
        preview_text.live_region(LiveRegion::Polite)
    };
    let mut header = row()
        .key("console-toggle")
        .w_full()
        .h(44.0)
        .px(32.0)
        .center_y()
        .label(tr(s, "logPanel.systemConsole"))
        .expanded(s.console_open)
        .focusable(true)
        .role("button")
        .on_press(|s: &mut Ui| s.console_open = !s.console_open);
    header = k.interactive(header);
    let header = header
        .child(row().grow(1.0))
        .child(
            row()
                .w(720.0)
                .gap(8.0)
                .center_y()
                .child(rune::icon(&glyphs::FILE_CODE, 14.0).color(k.c.icon_muted))
                .child(
                    text(tr(s, "logPanel.systemConsole"))
                        .font(k.ty.ui_medium)
                        .color(k.c.text_muted),
                )
                .child(preview_text)
                .child(
                    rune::icon(
                        if s.console_open {
                            k.glyph.chevron_up
                        } else {
                            k.glyph.chevron_down
                        },
                        16.0,
                    )
                    .color(k.c.icon_muted),
                ),
        )
        .child(row().grow(1.0));
    let mut strip = col()
        .key("console")
        .w_full()
        .fill(k.c.bg_deep)
        .child(el().h(1.0).w_full().fill(k.c.border_muted))
        .child(header);
    if s.console_open {
        let mut logs = col().key("console-lines").w_full().gap(2.0);
        for (i, (time, line)) in s.logs.iter().enumerate() {
            let mut entry = row().key(format!("log-{i}")).gap(12.0);
            if i + 1 == s.logs.len() {
                entry = entry.live_region(LiveRegion::Polite);
            }
            entry = entry
                .child(text(time.clone()).font(k.ty.mono).color(k.c.text_faint))
                .child(
                    text(line.clone())
                        .font(k.ty.mono)
                        .color(k.c.text_muted)
                        .truncate(),
                );
            logs = logs.child(entry);
        }
        let logs = logs
            .scroll(ScrollSpec { stick_bottom: true })
            .h(120.0)
            .min_h(0.0);
        let body = col()
            .w(720.0)
            .self_center()
            .gap(4.0)
            .child(
                row()
                    .w_full()
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
            .child(logs)
            .child(
                TextInput::new(&s.command)
                    .label(tr(s, "logPanel.terminalPlaceholder"))
                    .placeholder(tr(s, "logPanel.terminalPlaceholder"))
                    .width(None)
                    .on_edit(|s: &mut Ui| &mut s.command)
                    .on_submit(|s: &mut Ui, _| {
                        let cmd = s.command.text.clone();
                        s.run_command(cmd);
                        s.command.replace_text("");
                    })
                    .build(k)
                    .key("console-input"),
            );
        strip = strip.child(col().w_full().px(32.0).pb(10.0).child(body));
    }
    strip
}

fn main_view(s: &Ui, k: &Kit, layer: &mut Vec<El>) -> El {
    let scroll = col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .px(32.0)
        .pt(24.0)
        .pb(16.0)
        .child(
            col()
                .w_full()
                .max_w(720.0)
                .self_center()
                .grow(1.0)
                .child(mode_row(s, k))
                .child(device_head(s, k))
                .child(settings_card(s, k, layer)),
        );
    col()
        .key("main")
        .grow(1.0)
        .h_full()
        .min_w(0.0)
        .fill(k.c.bg_base)
        .child(scroll)
        .child(actionbar(s, k))
        .child(console_strip(s, k))
}

fn topbar(s: &Ui, k: &Kit, insets: rune::ChromeInsets) -> El {
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
    let mut status_row = row()
        .max_w(320.0)
        .gap(6.0)
        .center_y()
        .child(el().size(6.0, 6.0).radius(3.0).fill(dot));
    if s.download.active {
        status_row = status_row.child(
            el().key("sync-spinner")
                .size(12.0, 12.0)
                .center()
                .child(Spinner::new().size(12.0).color(k.c.success.fg).build(k)),
        );
    }
    status_row = status_row.child(
        text(status.clone())
            .font(k.ty.small)
            .color(k.c.text_muted)
            .truncate()
            .help(status),
    );
    let brand = row()
        .gap(8.0)
        .center_y()
        .child(rune::icon(&icons::BRAND, 22.0))
        .child(text("ScrcpyGUI").font(k.ty.ui_medium).color(k.c.text_base));
    let settings = IconButton::new(&icons::GEAR)
        .variant(IconButtonVariant::GhostMuted)
        .control_size(ControlSize::Regular)
        .label(tr(s, "settings.title"))
        .on_press(|s: &mut Ui| s.settings_open = !s.settings_open)
        .build(k)
        .key("open-settings");
    col()
        .key("topbar")
        .w_full()
        .child(
            row()
                .w_full()
                .h(47.0)
                .pl(16.0 + insets.leading)
                .pr(insets.trailing + 8.0)
                .gap(8.0)
                .center_y()
                .window_drag()
                .child(brand)
                .child(row().grow(1.0))
                .child(status_row)
                .child(settings),
        )
        .child(el().h(1.0).w_full().fill(k.c.border_muted))
}

fn rail(s: &Ui, k: &Kit) -> El {
    let refresh = Button::new(tr(s, "sidebar.refresh"))
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .disabled(s.is_refreshing)
        .on_press(|s: &mut Ui| s.refresh_devices())
        .build(k);
    let mut sidebar = Sidebar::new().header(
        SidebarHeader::new(tr(s, "sidebar.deviceHub"))
            .action(refresh)
            .build(k),
    );
    let mut devices = Vec::new();
    if s.devices.is_empty() {
        devices.push(
            SidebarRow::new(tr(s, "sidebar.noDevicesDetected"))
                .subtitle(tr(s, "sidebar.noDevicesHint"))
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
        let selected = s.config.device == serial;
        let row = SidebarRow::new(device.label())
            .subtitle(format!("{} · {}", serial, transport))
            .icon(&icons::PHONE)
            .selected(selected)
            .on_select(move |s: &mut Ui, _| s.select_device(serial.clone()));
        devices.push(row.build(k).key(format!("device-{}", device.serial)));
    }
    sidebar = sidebar.section(
        SidebarSection::new(tr(s, "sidebar.devices"))
            .count(s.devices.len())
            .rows(devices),
    );
    if !s.history.is_empty() {
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
                SidebarRow::new(tr(s, "sidebar.wirelessConnect"))
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
    col()
        .key("rail")
        .w(264.0)
        .h_full()
        .fill(k.c.bg_deep)
        .child(sidebar.build(k))
}

fn pair_rows(items: Vec<El>) -> El {
    let mut rows = col().w_full().gap(0.0);
    let mut iter = items.into_iter();
    while let Some(first) = iter.next() {
        let second = iter.next();
        let mut row = row()
            .w_full()
            .gap(16.0)
            .child(col().grow(1.0).min_w(0.0).child(first));
        row = match second {
            Some(second) => row.child(col().grow(1.0).min_w(0.0).child(second)),
            None => row.child(col().grow(1.0).min_w(0.0)),
        };
        rows = rows.child(row);
    }
    rows
}

fn drawer_section(k: &Kit, title: &str, content: El) -> El {
    col()
        .w_full()
        .gap(8.0)
        .child(section_heading(k, title))
        .child(content)
}

fn drawer_appearance(s: &Ui, k: &Kit, _layer: &mut Vec<El>, system_dark: bool) -> El {
    let selected = match s.color_mode.as_str() {
        "light" => 0,
        "dark" => 1,
        _ => 2,
    };
    // rune-kit's Select popup is clipped to 50 px inside the drawer by
    // anchored_fit, so language selection opens a full Dialog instead.
    let language = Button::new(language_name(&s.locale, &s.locale))
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Regular)
        .on_press(|s: &mut Ui| s.open_select = Some("locale".into()))
        .build(k)
        .key("locale-trigger");
    let themes = SegmentedControl::new(
        &[
            tr(s, "header.colorModes.light"),
            tr(s, "header.colorModes.dark"),
            tr(s, "header.colorModes.system"),
        ],
        Some(selected),
    )
    .label(tr(s, "settings.appearance"))
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
    .build(k);
    let content = col()
        .w_full()
        .gap(8.0)
        .child(themes)
        // Wrapping in Field gives the trigger a real width; a bare Select in a
        // drawer shrinks to its text and anchored_fit then clips the options.
        .child(field(tr(s, "header.languageLabel"), language, k))
        .child(switch_row(
            k,
            tr(s, "header.panelOnTop"),
            s.panel_on_top,
            |s| s.toggle_panel_on_top(),
        ));
    drawer_section(k, tr(s, "settings.appearance"), content)
}

fn drawer_behavior(s: &Ui, k: &Kit, layer: &mut Vec<El>) -> El {
    let switches = vec![
        switch_row(
            k,
            tr(s, "sessionBehavior.stayAwake"),
            s.config.stay_awake.unwrap_or(false),
            |s| {
                flip(&mut s.config.stay_awake, false);
                s.save_config();
            },
        ),
        switch_row(
            k,
            tr(s, "sessionBehavior.keepActive"),
            s.config.keep_active.unwrap_or(false),
            |s| {
                flip(&mut s.config.keep_active, false);
                s.save_config();
            },
        ),
        switch_row(
            k,
            tr(s, "sessionBehavior.screenOff"),
            s.config.turn_off.unwrap_or(false),
            |s| {
                flip(&mut s.config.turn_off, false);
                s.save_config();
            },
        ),
        switch_row(
            k,
            tr(s, "sessionBehavior.alwaysOnTop"),
            s.config.always_on_top.unwrap_or(false),
            |s| {
                flip(&mut s.config.always_on_top, false);
                s.save_config();
            },
        ),
        switch_row(
            k,
            tr(s, "sessionBehavior.fullScreen"),
            s.config.fullscreen.unwrap_or(false),
            |s| {
                flip(&mut s.config.fullscreen, false);
                s.save_config();
            },
        ),
        switch_row(
            k,
            tr(s, "sessionBehavior.borderless"),
            s.config.borderless.unwrap_or(false),
            |s| {
                flip(&mut s.config.borderless, false);
                s.save_config();
            },
        ),
        switch_row(
            k,
            tr(s, "sessionBehavior.rememberWindowPosition"),
            s.remember_position,
            |s| s.toggle_remember_position(),
        ),
        switch_row(
            k,
            tr(s, "sessionBehavior.recordFeed"),
            s.config.record.unwrap_or(false),
            |s| {
                flip(&mut s.config.record, false);
                s.save_config();
            },
        ),
    ];
    let mut content = col()
        .w_full()
        .gap(8.0)
        .child(pair_rows(switches))
        .child(select_control(
            s,
            k,
            layer,
            "audio",
            AUDIO_CODEC,
            s.config.audio_codec.as_deref().unwrap_or("auto"),
            tr(s, "sessionBehavior.audioCodec"),
            170.0,
        ));
    if s.config.record.unwrap_or(false) {
        content = content.child(
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
    drawer_section(k, tr(s, "sessionBehavior.title"), content)
}

fn shortcut_row(k: &Kit, label: &str, keys: &[&str]) -> El {
    row()
        .w_full()
        .h(28.0)
        .gap(8.0)
        .center_y()
        .child(
            text(label.to_string())
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .grow(1.0),
        )
        .child(Keybind::new(keys).build(k))
}

fn drawer_shortcuts(s: &Ui, k: &Kit) -> El {
    let mut session = Vec::new();
    for (key, label) in [
        ("F", "shortcuts.full"),
        ("H", "shortcuts.home"),
        ("B", "shortcuts.back"),
        ("S", "shortcuts.recents"),
        ("P", "shortcuts.power"),
        ("R", "shortcuts.rotate"),
        ("V", "shortcuts.paste"),
        ("O", "shortcuts.off"),
    ] {
        session.push(shortcut_row(k, tr(s, label), &["Alt", key]));
    }
    let mut app = Vec::new();
    for (key, label) in [("C", "shortcuts.recenter"), ("W", "shortcuts.drag")] {
        app.push(shortcut_row(
            k,
            tr(s, label),
            &["Ctrl", "Alt", "Shift", key],
        ));
    }
    drawer_section(
        k,
        tr(s, "shortcuts.title"),
        col()
            .w_full()
            .gap(8.0)
            .child(section_heading(k, tr(s, "shortcuts.sessionTitle")))
            .child(pair_rows(session))
            .child(section_heading(k, tr(s, "shortcuts.appTitle")))
            .child(pair_rows(app)),
    )
}

fn drawer_core(s: &Ui, k: &Kit) -> El {
    let path = s
        .config
        .scrcpy_path
        .clone()
        .unwrap_or_else(|| "scrcpy · Rust".into());
    let mut buttons = row().w_full().gap(8.0).child(folder_button(
        s,
        k,
        tr(s, "header.selectFolder"),
        |s, path| {
            s.set_scrcpy_path(Some(path.to_string_lossy().to_string()));
        },
    ));
    if s.scrcpy_ok == Some(false) {
        buttons = buttons.child(
            Button::new(tr(s, "header.installCore"))
                .variant(ButtonVariant::Contrast)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut Ui| s.download_scrcpy())
                .build(k),
        );
    }
    buttons = buttons.child(
        Button::new(tr(s, "ui.endAdb"))
            .variant(ButtonVariant::Danger)
            .control_size(ControlSize::Small)
            .on_press(|s: &mut Ui| {
                s.settings_open = false;
                s.confirm_kill_adb = true;
            })
            .build(k),
    );
    drawer_section(
        k,
        tr(s, "footer.core"),
        col()
            .w_full()
            .gap(8.0)
            .child(
                col()
                    .w_full()
                    .gap(1.0)
                    .child(text("scrcpy").font(k.ty.ui_medium).color(k.c.text_base))
                    .child(text(path).font(k.ty.small).color(k.c.text_faint).truncate()),
            )
            .child(buttons),
    )
}

fn settings_drawer(s: &Ui, k: &Kit, layer: &mut Vec<El>, system_dark: bool) {
    if !s.settings_open {
        return;
    }
    let mut popups = Vec::new();
    let body = col()
        .w_full()
        .gap(20.0)
        .child(drawer_appearance(s, k, &mut popups, system_dark))
        .child(drawer_behavior(s, k, &mut popups))
        .child(drawer_core(s, k));
    let header = row()
        .w_full()
        .h(50.0)
        .px(16.0)
        .center_y()
        .child(
            text(tr(s, "settings.title"))
                .font(k.ty.body_medium)
                .color(k.c.text_base)
                .grow(1.0),
        )
        .child(
            IconButton::new(k.glyph.close)
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Regular)
                .label(tr(s, "common.close"))
                .on_press(|s: &mut Ui| s.settings_open = false)
                .build(k)
                .key("close-settings"),
        );
    let panel = col()
        .key("settings-drawer")
        .pin(None, Some(48.0), Some(0.0), Some(0.0))
        .w(380.0)
        .fill(k.c.layer_01)
        .child(header)
        .child(el().h(1.0).w_full().fill(k.c.border_muted))
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(col().w_full().padding(16.0).gap(20.0).child(body)),
        );
    let panel = k
        .elevate(panel, &k.elevation.overlay)
        .modal(true, |s: &mut Ui| s.settings_open = false);
    layer.push(
        el().key("settings-scrim")
            .pin(Some(0.0), Some(48.0), Some(0.0), Some(0.0))
            .fill(k.c.scrim)
            .on_press(|s: &mut Ui| s.settings_open = false),
    );
    layer.push(panel);
    layer.extend(popups);
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
        let label = format!("{} · {}", service.name, address);
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
    let transport = SegmentedControl::new(
        &[tr(s, "sidebar.usb"), tr(s, "sidebar.wifi")],
        Some(if s.transport == Transport::Usb { 0 } else { 1 }),
    )
    .label(tr(s, "sidebar.deviceHub"))
    .width(None)
    .on_pick(|s: &mut Ui, i, _| {
        s.set_transport(if i == 0 {
            Transport::Usb
        } else {
            Transport::Wifi
        });
    })
    .build(k);
    let mut body = col().w_full().padding(16.0).gap(12.0).child(transport);
    match s.transport {
        Transport::Usb => {
            let mut actions = row().w_full().gap(8.0).items_center().child(
                Button::new(tr(s, "connect.refreshDevices"))
                    .variant(ButtonVariant::Neutral)
                    .control_size(ControlSize::Regular)
                    .disabled(s.is_refreshing)
                    .on_press(|s: &mut Ui| s.refresh_devices())
                    .build(k)
                    .key("connect-refresh"),
            );
            if s.connecting {
                actions = actions.child(
                    Button::new(tr(s, "connect.cancel"))
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Regular)
                        .on_press(|s: &mut Ui| s.cancel_connect())
                        .build(k)
                        .key("connect-cancel"),
                );
            }
            body = body
                .child(
                    text(tr(s, "connect.usbTitle"))
                        .font(k.ty.body_medium)
                        .color(k.c.text_base),
                )
                .child(
                    text(tr(s, "connect.usbBody"))
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .wrap(),
                )
                .child(actions);
        }
        Transport::Wifi => {
            let mut submit = row()
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
                );
            if s.connecting {
                submit = submit.child(
                    Button::new(tr(s, "connect.cancel"))
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Large)
                        .on_press(|s: &mut Ui| s.cancel_connect())
                        .build(k)
                        .key("connect-cancel"),
                );
            }
            body = body
                .child(submit)
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
                        s.pair_ip.text.trim().is_empty()
                            || s.pair_code.text.trim().is_empty()
                            || s.pairing,
                    )
                    .on_press(|s: &mut Ui| {
                        let ip = s.pair_ip.text.clone();
                        let code = s.pair_code.text.clone();
                        s.pair(ip, code);
                    })
                    .build(k)
                    .key("pair-submit"),
                );
        }
    }
    Dialog::new("connect", tr(s, "sidebar.connect"))
        .open(s.connect_open)
        .width(520.0)
        .fit()
        .on_close(|s: &mut Ui| s.connect_open = false)
        .child(body)
        .build(k, layer);
}

fn locale_dialog(s: &Ui, k: &Kit, layer: &mut Vec<El>) {
    let mut body = col().w_full().gap(2.0);
    for code in LOCALES {
        let chosen = *code;
        body = body.child(
            Button::new(language_name(&s.locale, chosen))
                .variant(if s.locale == chosen {
                    ButtonVariant::Contrast
                } else {
                    ButtonVariant::Ghost
                })
                .control_size(ControlSize::Regular)
                .on_press(move |s: &mut Ui| {
                    s.set_locale(chosen);
                    s.open_select = None;
                })
                .build(k),
        );
    }
    Dialog::new("locale", tr(s, "header.languageLabel"))
        .open(s.open_select.as_deref() == Some("locale"))
        .width(360.0)
        .fit()
        .on_close(|s: &mut Ui| s.open_select = None)
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
                )
                .child(drawer_shortcuts(s, k)),
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

fn disconnect_dialog(s: &Ui, k: &Kit, layer: &mut Vec<El>) {
    Dialog::new("disconnect", tr(s, "device.disconnectConfirmTitle"))
        .open(s.disconnect_confirm)
        .width(420.0)
        .fit()
        .on_close(|s: &mut Ui| s.cancel_disconnect())
        .child(
            col()
                .w_full()
                .padding(16.0)
                .gap(12.0)
                .child(
                    text(tr(s, "device.disconnectConfirmBody"))
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .wrap(),
                )
                .child(
                    row()
                        .gap(8.0)
                        .child(
                            Button::new(tr(s, "device.disconnectConfirm"))
                                .variant(ButtonVariant::Danger)
                                .control_size(ControlSize::Regular)
                                .on_press(|s: &mut Ui| s.confirm_disconnect())
                                .build(k)
                                .key("disconnect-confirm"),
                        )
                        .child(
                            Button::new(tr(s, "common.cancel"))
                                .variant(ButtonVariant::Ghost)
                                .control_size(ControlSize::Regular)
                                .on_press(|s: &mut Ui| s.cancel_disconnect())
                                .build(k)
                                .key("disconnect-cancel"),
                        ),
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
    let k = theme::kit(dark);
    // The system's caption symbols follow the OS appearance; keep them readable on
    // the app's own light or dark titlebar instead.
    cx.caption_color(k.c.icon_muted);
    let insets = cx.chrome_insets();
    let mut layer = Vec::new();

    let main = main_view(s, k, &mut layer);
    let shell = col()
        .w_full()
        .h_full()
        .fill(k.c.bg_base)
        .child(topbar(s, k, insets))
        .child(
            row()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(rail(s, k))
                .child(el().w(1.0).h_full().fill(k.c.border_muted))
                .child(main),
        );

    settings_drawer(s, k, &mut layer, system_dark);
    connect_dialog(s, k, &mut layer);
    about_dialog(s, k, &mut layer);
    locale_dialog(s, k, &mut layer);
    end_adb_dialog(s, k, &mut layer);
    disconnect_dialog(s, k, &mut layer);
    update_dialog(s, k, &mut layer);
    onboarding_dialog(s, k, &mut layer);
    alert_dialog(s, k, &mut layer);
    if !s.toasts.is_empty() {
        let mut region = ToastRegion::new();
        for note in &s.toasts {
            let seq = note.seq;
            region = region.toast(
                Toast::new(format!("app-toast-{seq}"), note.title.clone())
                    .description(note.description.clone())
                    .on_close(move |s: &mut Ui| s.toasts.retain(|t| t.seq != seq))
                    .build(k),
            );
        }
        layer.push(region.build(k));
    }
    let page = root(shell, layer);
    page.dir(if s.locale == "ar" {
        rune::Direction::Rtl
    } else {
        rune::Direction::Ltr
    })
}
