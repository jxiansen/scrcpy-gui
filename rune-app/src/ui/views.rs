//! Pixel-oriented port of the old React/Tailwind interface to Rune.
//! Geometry, colours and type scale follow the CSS at commit c4e8721.

use rune::{Color, Cursor, Cx, El, ScrollSpec, TextStyle, col, el, icon, row, text};
use rune_kit::{Kit, PaletteSeed, Select, SelectAppearance, TextInput};

use crate::i18n::{language_name, t, tv};
use crate::icons;
use crate::theme::{Palette, THEMES, hex};
use crate::ui::state::{SessionKind, Transport, Ui};

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

type ToggleRow = (&'static str, bool, fn(&mut Ui));

fn micro(size: f32, weight: f32, tracking: f32) -> TextStyle {
    TextStyle::new(size, weight)
        .tracking(tracking)
        .lh(size + 4.0)
}

fn mono(size: f32) -> TextStyle {
    micro(size, 400.0, 0.0).mono()
}

fn eyebrow(p: &Palette, value: &str, size: f32) -> El {
    text(value.to_uppercase())
        .font(micro(size, 900.0, size * 0.1))
        .color(p.muted)
}

fn kit(p: &Palette) -> Kit {
    Kit::from_palette(PaletteSeed {
        bg: p.elevated,
        fg: p.text,
        accent: p.primary,
    })
}

pub fn view(s: &Ui, cx: &mut Cx<Ui>) -> El {
    let p = Palette::new(&s.theme, s.dark);
    let k = kit(&p);
    // Make pressable controls Tab stops and give every control a visible ring.
    cx.web_focus();
    cx.focus_ring(2.0, 1.0, p.primary);
    let mut layer: Vec<El> = Vec::new();
    let tr = |key: &str| t(&s.locale, key);

    let root = col()
        .key("window")
        .w_full()
        .h_full()
        .fill(p.bg)
        .on_drop_files(|s: &mut Ui, paths: &[std::path::PathBuf], _| {
            if s.config.device.is_empty() {
                s.log(t(&s.locale, "logs.noDeviceForDragDrop").to_string());
                return;
            }
            for path in paths {
                s.handle_file(path.clone());
            }
        })
        .child(
            el().key("glow")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .radial_gradient(
                    [0.5, 0.5],
                    720.0,
                    [
                        (0.0, p.primary.alpha(0.18)),
                        (0.55, p.primary.alpha(0.05)),
                        (1.0, p.primary.alpha(0.0)),
                    ],
                )
                .a11y_hidden(),
        )
        .child(
            el().key("noise")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .pixels(&s.noise)
                .opacity(0.12)
                .a11y_hidden(),
        )
        .child(header(s, &p, &k, &mut layer, &tr))
        .child(
            row()
                .key("workspace")
                .grow(1.0)
                .min_h(0.0)
                .px(24.0)
                .pt(16.0)
                .pb(24.0)
                .gap(24.0)
                .child(sidebar(s, &p, &k, &tr))
                .child(center(s, &p, &k, &mut layer, &tr))
                .child(right(s, &p, &k, &mut layer, &tr)),
        )
        .child(footer(&p, &tr));

    if s.onboarding_open && s.scrcpy_ok == Some(false) {
        layer.push(onboarding(s, &p, &tr));
    }
    if let Some(info) = &s.update {
        layer.push(update_modal(s, &p, info, &tr));
    }
    if let Some(message) = &s.alert {
        layer.push(alert_modal(s, &p, message));
    }
    rune_kit::root(root, layer)
}

fn header<F: Fn(&str) -> &'static str>(
    s: &Ui,
    p: &Palette,
    k: &Kit,
    layer: &mut Vec<El>,
    tr: &F,
) -> El {
    let mut swatches = row().key("swatches").gap(8.0).center_y();
    for id in THEMES.iter() {
        let theme = *id;
        let preview = Palette::new(theme, s.dark);
        let selected = s.theme == theme;
        let name = tr(&format!("header.themes.{theme}"));
        let mut dot = el()
            .key(format!("swatch-{theme}"))
            .label(name)
            .size(16.0, 16.0)
            .radius(8.0)
            .fill(preview.primary)
            .border(1.0, p.border_subtle)
            .help(name)
            .on_press(move |s: &mut Ui| s.set_theme(theme));
        if selected {
            dot = dot.outline(2.0, 2.0, p.text);
        }
        swatches = swatches.child(dot);
    }

    let mut modes = row()
        .key("modes")
        .gap(2.0)
        .padding(2.0)
        .radius(6.0)
        .fill(p.glass)
        .border(1.0, p.border);
    let mode_labels = [tr("header.colorModes.light"), tr("header.colorModes.dark")];
    for (i, label) in mode_labels.iter().enumerate() {
        let active = (i == 0 && !s.dark) || (i == 1 && s.dark);
        modes = modes.child(
            el().key(format!("mode-{i}"))
                .h(18.0)
                .px(5.0)
                .help(*label)
                .center()
                .radius(4.0)
                .fill(if !active {
                    Color::TRANSPARENT
                } else if s.dark {
                    p.primary
                } else {
                    p.primary.alpha(0.14)
                })
                .on_press(move |s: &mut Ui| {
                    if i < 2 {
                        s.set_dark(i == 1);
                    }
                })
                .child(
                    text(label.to_uppercase())
                        .font(micro(8.0, 900.0, 0.5))
                        .color(if !active {
                            p.muted
                        } else if s.dark {
                            p.on_primary
                        } else {
                            p.primary
                        }),
                ),
        );
    }

    let lang_options: Vec<String> = LOCALES
        .iter()
        .map(|code| language_name(&s.locale, code).to_string())
        .collect();
    let lang = Select::new("locale", &lang_options)
        .width(110.0)
        .current(Some(language_name(&s.locale, &s.locale)))
        .label(tr("header.languageLabel"))
        .appearance(SelectAppearance::Inline)
        .open(s.open_select.as_deref() == Some("locale"))
        .on_toggle(|s: &mut Ui| toggle_select(s, "locale"))
        .on_close(|s: &mut Ui| s.open_select = None)
        .on_choose(|s: &mut Ui, _label: &str, _| {
            let idx = LOCALES
                .iter()
                .position(|code| language_name(&s.locale, code) == _label)
                .unwrap_or(0);
            s.set_locale(LOCALES[idx]);
        })
        .build(k, layer);

    let dot = match s.scrcpy_ok {
        Some(true) => p.success,
        Some(false) => p.danger,
        None => p.warning,
    };
    let status = if s.download.active {
        tv(
            &s.locale,
            "header.syncingComponents",
            &[("progress", &s.download.percent.to_string())],
        )
    } else {
        match s.scrcpy_ok {
            Some(true) => tr("header.scrcpyReady").to_string(),
            Some(false) => s.scrcpy_message.clone(),
            None => tr("common.loading").to_string(),
        }
    };

    let mut engine = row()
        .key("engine")
        .gap(16.0)
        .padding(8.0)
        .radius(12.0)
        .fill(p.glass)
        .border(1.0, p.glass_border)
        .child(
            col()
                .key("engine-status")
                .min_w(110.0)
                .gap(4.0)
                .child(
                    row()
                        .gap(6.0)
                        .center_y()
                        .child(eyebrow(p, tr("header.scrcpyEngine"), 9.0))
                        .child(el().size(6.0, 6.0).radius(3.0).fill(dot)),
                )
                .child(
                    text(status.to_uppercase())
                        .font(micro(12.0, 900.0, -0.6))
                        .color(match s.scrcpy_ok {
                            Some(true) => p.success,
                            Some(false) => p.danger,
                            None => p.muted,
                        }),
                ),
        );

    if s.download.active {
        engine = engine.child(
            el().key("download-track")
                .w(70.0)
                .h(4.0)
                .radius(2.0)
                .fill(p.border)
                .child(
                    el().w(70.0 * s.download.percent as f32 / 100.0)
                        .h(4.0)
                        .radius(2.0)
                        .fill(p.success),
                ),
        );
    }
    engine = engine
        .child(
            el().key("engine-divider")
                .w(1.0)
                .h(28.0)
                .fill(p.border)
                .a11y_hidden(),
        )
        .child(folder_button(p, tr("header.selectFolder"), |s, path| {
            s.set_scrcpy_path(Some(path.to_string_lossy().to_string()));
        }))
        .child(if s.scrcpy_ok == Some(true) {
            text_button(p, tr("header.installCore"), p.primary, 9.0, |s: &mut Ui| {
                s.download_scrcpy();
            })
        } else {
            let label = if s.download.active {
                format!("{} {}%", tr("header.syncingComponents"), s.download.percent)
            } else {
                tr("header.installCore").to_string()
            };
            solid_button(p, &label, p.success, p.bg, 9.0, |s: &mut Ui| {
                s.download_scrcpy();
            })
        });

    row()
        .key("header")
        .cursor(Cursor::Pointer)
        .h(56.0)
        .px(8.0)
        .py(8.0)
        .gap(16.0)
        .center_y()
        .child(
            row()
                .key("header-left")
                .grow(1.0)
                .gap(20.0)
                .center_y()
                .child(eyebrow(p, tr("header.themeLabel"), 9.0))
                .child(swatches)
                .child(modes)
                .child(lang),
        )
        .child(
            row()
                .key("branding")
                .grow(1.0)
                .center()
                .gap(8.0)
                .child(
                    text("SCRCPY")
                        .font(TextStyle::new(28.0, 900.0).tracking(-1.4).italic())
                        .color(p.text),
                )
                .child(
                    text("GUI")
                        .font(TextStyle::new(28.0, 900.0).tracking(-1.4))
                        .color(p.primary),
                )
                .child(
                    row()
                        .gap(6.0)
                        .center_y()
                        .padding(3.0)
                        .radius(4.0)
                        .fill(p.input)
                        .child(
                            text(format!("V{}", env!("CARGO_PKG_VERSION")))
                                .font(micro(10.0, 900.0, 0.6))
                                .color(p.muted),
                        )
                        .child(el().size(6.0, 6.0).radius(3.0).fill(p.success)),
                ),
        )
        .child(
            row()
                .key("header-right")
                .grow(1.0)
                .gap(12.0)
                .center()
                .child(engine),
        )
}

fn sidebar<F: Fn(&str) -> &'static str>(s: &Ui, p: &Palette, k: &Kit, tr: &F) -> El {
    let mut device_rows = col()
        .key("devices")
        .gap(6.0)
        .max_h(220.0)
        .min_h(40.0)
        .shrink();
    if s.devices.is_empty() {
        device_rows = device_rows.child(
            col()
                .h(64.0)
                .center()
                .radius(8.0)
                .fill(p.elevated.alpha(0.2))
                .border(1.0, p.border.alpha(0.5))
                .child(
                    text(tr("sidebar.noDevicesDetected"))
                        .font(micro(10.0, 400.0, 0.0).italic())
                        .color(p.subtle),
                )
                .child(text_button(
                    p,
                    tr("sidebar.refresh"),
                    p.primary,
                    9.0,
                    |s: &mut Ui| s.refresh_devices(),
                )),
        );
    }
    for device in &s.devices {
        let serial = device.serial.clone();
        let selected = s.config.device == device.serial;
        let running = s.running.get(&device.serial).copied().unwrap_or(false);
        let wifi = device.serial.contains('.');
        device_rows =
            device_rows.child(
                row()
                    .key(format!("device-{}", device.serial))
                    .gap(10.0)
                    .padding(10.0)
                    .radius(8.0)
                    .fill(if selected {
                        p.primary.alpha(0.05)
                    } else {
                        p.elevated.alpha(0.2)
                    })
                    .border(
                        1.0,
                        if selected {
                            p.primary.alpha(0.3)
                        } else {
                            p.border.alpha(0.5)
                        },
                    )
                    .on_press(move |s: &mut Ui| s.select_device(serial.clone()))
                    .hover(p.primary.alpha(0.06))
                    .press(p.primary.alpha(0.12))
                    .child(
                        row()
                            .size(26.0, 26.0)
                            .radius(6.0)
                            .center()
                            .fill(if selected { p.primary } else { p.input })
                            .child(icon(&icons::PHONE, 14.0).color(if selected {
                                p.on_primary
                            } else {
                                p.subtle
                            })),
                    )
                    .child(
                        col()
                            .grow(1.0)
                            .gap(2.0)
                            .child(
                                text(device.label())
                                    .help(device.label())
                                    .font(micro(11.0, 700.0, -0.3))
                                    .truncate()
                                    .color(if selected { p.text } else { p.muted }),
                            )
                            .child(
                                row()
                                    .gap(8.0)
                                    .center_y()
                                    .child(el().size(4.0, 4.0).radius(2.0).fill(if running {
                                        p.success
                                    } else {
                                        p.subtle
                                    }))
                                    .child(
                                        text(
                                            if running {
                                                tr("sidebar.live")
                                            } else {
                                                tr("sidebar.ready")
                                            }
                                            .to_uppercase(),
                                        )
                                        .font(micro(8.0, 900.0, 0.8))
                                        .color(if running { p.success } else { p.subtle }),
                                    )
                                    .child(
                                        text(if wifi {
                                            tr("sidebar.wifi")
                                        } else {
                                            tr("sidebar.usb")
                                        })
                                        .font(micro(7.0, 900.0, 0.3))
                                        .color(if wifi {
                                            p.primary
                                        } else {
                                            p.muted
                                        }),
                                    ),
                            ),
                    ),
            );
    }

    let tabs = row()
        .key("transport")
        .gap(4.0)
        .padding(4.0)
        .radius(8.0)
        .fill(p.elevated.alpha(0.4))
        .border(1.0, p.border.alpha(0.5))
        .child(segment(
            p,
            tr("sidebar.usb"),
            s.transport == Transport::Usb,
            |s: &mut Ui| s.set_transport(Transport::Usb),
        ))
        .child(segment(
            p,
            tr("sidebar.wifi"),
            s.transport == Transport::Wifi,
            |s: &mut Ui| s.set_transport(Transport::Wifi),
        ));

    let ip_input = input(
        p,
        k,
        &s.ip,
        tr("sidebar.ipPlaceholder"),
        "IP",
        tr("ui.ipAddress"),
    );
    let pair_ip = input(
        p,
        k,
        &s.pair_ip,
        tr("sidebar.ipPlaceholder"),
        "Pair IP",
        tr("ui.ipAddress"),
    );
    let pair_code = input(
        p,
        k,
        &s.pair_code,
        tr("sidebar.pairingCodePlaceholder"),
        "Pairing code",
        tr("ui.pairingCode"),
    );
    let pair_button = solid_button(
        p,
        if s.pairing {
            tr("sidebar.synchronizing")
        } else {
            tr("sidebar.startPairing")
        },
        p.primary,
        p.on_primary,
        10.0,
        |s: &mut Ui| {
            let ip = s.pair_ip.text.clone();
            let code = s.pair_code.text.clone();
            s.pair(ip, code);
        },
    );

    let mut wifi_rows = col().key("wifi").gap(12.0);
    for service in &s.mdns {
        let pairing = service.service.contains("_adb-tls-pairing");
        if !pairing && !service.service.contains("_adb-tls-connect") {
            continue;
        }
        let address = service.address.clone();
        wifi_rows = wifi_rows.child(
            row()
                .key(format!("mdns-{}", service.name))
                .gap(8.0)
                .padding(8.0)
                .radius(8.0)
                .fill(p.input.alpha(0.2))
                .border(1.0, p.border.alpha(0.5))
                .help(tr("ui.useAddress"))
                .on_press(move |s: &mut Ui| {
                    if pairing {
                        s.pair_ip.replace_text(address.clone());
                    } else {
                        s.ip.replace_text(address.clone());
                    }
                })
                .hover(p.primary.alpha(0.06))
                .press(p.primary.alpha(0.12))
                .child(
                    text(&service.name)
                        .font(micro(10.0, 700.0, 0.0))
                        .color(p.muted)
                        .grow(1.0)
                        .truncate(),
                )
                .child(text(&service.address).font(mono(8.0)).color(p.primary)),
        );
    }

    let mut recent = col().key("recent").gap(4.0);
    if s.history.is_empty() {
        recent = recent.child(
            text(tr("ui.noRecords"))
                .font(micro(10.0, 400.0, 0.0))
                .color(p.subtle),
        );
    }
    for ip in &s.history {
        let target = ip.clone();
        recent = recent.child(
            row()
                .key(format!("recent-{ip}"))
                .gap(8.0)
                .padding(8.0)
                .radius(8.0)
                .fill(p.input.alpha(0.2))
                .border(1.0, p.border.alpha(0.5))
                .on_press(move |s: &mut Ui| {
                    let ip = target.clone();
                    s.ip.replace_text(ip.clone());
                    s.connect(ip);
                })
                .hover(p.primary.alpha(0.06))
                .press(p.primary.alpha(0.12))
                .child(
                    text(ip)
                        .font(micro(10.0, 700.0, 0.0))
                        .color(p.muted)
                        .grow(1.0)
                        .truncate(),
                )
                .child(
                    text(tr("sidebar.connect").to_uppercase())
                        .font(micro(8.0, 900.0, 0.8))
                        .color(p.primary),
                ),
        );
    }

    let hub = section(
        "hub",
        p,
        12.0,
        row()
            .gap(8.0)
            .center_y()
            .child(eyebrow(p, tr("sidebar.deviceHub"), 11.0).color(p.muted))
            .child(row().grow(1.0))
            .child(text_button(p, "⋯", p.subtle, 12.0, |s: &mut Ui| {
                s.toggle_adb_menu()
            }))
            .child(text_button(
                p,
                if s.is_refreshing {
                    tr("sidebar.syncing")
                } else {
                    tr("sidebar.refresh")
                },
                p.primary,
                9.0,
                |s: &mut Ui| s.refresh_devices(),
            )),
        col()
            .gap(10.0)
            .child(if s.adb_menu {
                row()
                    .gap(8.0)
                    .padding(8.0)
                    .radius(8.0)
                    .fill(p.elevated)
                    .child(
                        text(if s.confirm_kill_adb {
                            tr("ui.confirmEndAdb")
                        } else {
                            tr("ui.endAdb")
                        })
                        .font(micro(10.0, 500.0, 0.0))
                        .color(if s.confirm_kill_adb {
                            p.danger
                        } else {
                            p.muted
                        })
                        .grow(1.0),
                    )
                    .child(text_button(
                        p,
                        if s.confirm_kill_adb {
                            tr("common.confirm")
                        } else {
                            tr("ui.end")
                        },
                        if s.confirm_kill_adb {
                            p.danger
                        } else {
                            p.primary
                        },
                        9.0,
                        |s: &mut Ui| s.confirm_kill_adb(),
                    ))
                    .child(text_button(
                        p,
                        tr("common.cancel"),
                        p.subtle,
                        9.0,
                        |s: &mut Ui| s.toggle_adb_menu(),
                    ))
            } else {
                el().size(0.0, 0.0)
            })
            .child(device_rows.scroll(ScrollSpec {
                stick_bottom: false,
            }))
            .child(tabs)
            .child({
                let tip_text = if s.transport == Transport::Usb {
                    format!(
                        "{} {} {} {} {}",
                        tr("sidebar.usbSetupTipTextBefore"),
                        tr("sidebar.usbSetupTipDeveloperOptions"),
                        tr("sidebar.usbSetupTipAnd"),
                        tr("sidebar.usbSetupTipUsbDebugging"),
                        tr("sidebar.usbSetupTipTextAfter")
                    )
                } else {
                    format!(
                        "{} {} {} {} {}",
                        tr("sidebar.wirelessSetupTipTextBefore"),
                        tr("sidebar.wirelessSetupTipSameWifi"),
                        tr("sidebar.wirelessSetupTipAnd"),
                        tr("sidebar.wirelessSetupTipWirelessDebugging"),
                        tr("sidebar.wirelessSetupTipTextAfter")
                    )
                };
                col()
                    .gap(8.0)
                    .child(section_label(
                        p,
                        if s.transport == Transport::Usb {
                            tr("sidebar.usbSetupTip")
                        } else {
                            tr("sidebar.wirelessSetupTip")
                        },
                        false,
                    ))
                    .child(tip_card(p, &tip_text))
                    .child(section_label(
                        p,
                        &format!("1. {}", tr("sidebar.ipConnect")),
                        false,
                    ))
                    .child(
                        row()
                            .gap(6.0)
                            .center_y()
                            .child(ip_input)
                            .child(solid_button(
                                p,
                                if s.connecting {
                                    tr("sidebar.connecting")
                                } else {
                                    tr("sidebar.connect")
                                },
                                p.primary.alpha(0.14),
                                p.primary,
                                10.0,
                                |s: &mut Ui| {
                                    let ip = s.ip.text.clone();
                                    s.connect(ip);
                                },
                            )),
                    )
                    .child(section_label(p, tr("sidebar.discoveredDevices"), true))
                    .child(wifi_rows.max_h(100.0).scroll(ScrollSpec {
                        stick_bottom: false,
                    }))
                    .child(
                        row()
                            .center_y()
                            .child(eyebrow(p, tr("sidebar.recentDevices"), 9.0))
                            .child(row().grow(1.0))
                            .child(text_button(
                                p,
                                tr("sidebar.clear"),
                                p.primary,
                                8.0,
                                |s: &mut Ui| s.clear_history(),
                            )),
                    )
                    .child(recent.max_h(90.0).scroll(ScrollSpec {
                        stick_bottom: false,
                    }))
                    .child(section_label(p, tr("sidebar.pairDeviceTitle"), false))
                    .child(row().gap(8.0).child(pair_ip).child(pair_code))
                    .child(pair_button)
            }),
    );

    let drop_zone = col()
        .key("push")
        .gap(6.0)
        .padding(12.0)
        .center()
        .fill(if s.drop_hover {
            p.primary.alpha(0.08)
        } else {
            p.glass
        })
        .border(
            2.0,
            if s.drop_hover {
                p.primary.alpha(0.5)
            } else {
                p.border.alpha(0.5)
            },
        )
        .radius(12.0)
        .help(tr("sidebar.flashPushSubtitle"))
        .hover(p.primary.alpha(0.04))
        .on_drop_hover(|s: &mut Ui, over: bool, _| s.drop_hover = over)
        .on_press_tx(|s: &mut Ui, tx: &mut rune::Tx| {
            let opts = rune::FileDialogOptions {
                title: t(&s.locale, "sidebar.flashPushFiles").to_string(),
                ..Default::default()
            }
            .filter("Android App", &["apk"])
            .filter("All files", &["*"]);
            tx.open_file(
                opts,
                |s: &mut Ui, result: rune::file_dialog::FileDialogResult, _| {
                    if let Ok(Some(path)) = result {
                        s.handle_file(path);
                    }
                },
            );
        })
        .child(
            row()
                .size(40.0, 40.0)
                .radius(24.0)
                .center()
                .fill(p.primary.alpha(0.1))
                .child(text("↑").font(micro(24.0, 700.0, 0.0)).color(p.primary)),
        )
        .child(
            text(tr("sidebar.flashPushFiles").to_uppercase())
                .font(micro(11.0, 900.0, 1.1))
                .color(p.text),
        )
        .child(
            text(tr("sidebar.flashPushSubtitle").to_uppercase())
                .font(micro(9.0, 700.0, 0.3))
                .color(p.subtle),
        );

    col()
        .key("sidebar")
        .cursor(Cursor::Pointer)
        .w(270.0)
        .min_h(0.0)
        .gap(16.0)
        .shrink()
        .child(hub)
        .child(drop_zone)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
}

fn center<F: Fn(&str) -> &'static str>(
    s: &Ui,
    p: &Palette,
    k: &Kit,
    layer: &mut Vec<El>,
    tr: &F,
) -> El {
    let mut source = row()
        .key("source")
        .gap(6.0)
        .padding(4.0)
        .radius(12.0)
        .fill(p.elevated.alpha(0.5))
        .border(1.0, p.border);
    for mode in [
        SessionKind::Mirror,
        SessionKind::Camera,
        SessionKind::Desktop,
    ] {
        let label = match mode {
            SessionKind::Mirror => tr("controlPanel.screen"),
            SessionKind::Camera => tr("controlPanel.camera"),
            SessionKind::Desktop => tr("controlPanel.desktop"),
        };
        let active = s.tab == mode;
        let ico: &rune::Icon = match mode {
            SessionKind::Mirror => &icons::MONITOR,
            SessionKind::Camera => &icons::CAMERA,
            SessionKind::Desktop => &icons::GRID,
        };
        source = source.child(
            row()
                .key(format!("source-{:?}", mode))
                .grow(1.0)
                .h(36.0)
                .center()
                .gap(8.0)
                .radius(9.0)
                .fill(if active {
                    p.primary
                } else {
                    Color::TRANSPARENT
                })
                .on_press_tx(move |s: &mut Ui, tx: &mut rune::Tx| {
                    // Avoid spring-driven text rasterisation (blur + shift) on tab change.
                    s.tab = mode;
                    tx.instant();
                })
                .child(icon(ico, 14.0).color(if active { p.on_primary } else { p.muted }))
                .child(
                    text(label.to_uppercase())
                        .font(TextStyle::new(12.0, 600.0).lh(16.0))
                        .color(if active { p.on_primary } else { p.muted }),
                ),
        );
    }

    let capture = section(
        "capture",
        p,
        10.0,
        text(tr("controlPanel.captureSource").to_uppercase())
            .font(micro(10.0, 900.0, 1.0))
            .color(p.muted),
        source,
    );

    let running = s.running.get(&s.config.device).copied().unwrap_or(false);
    let start_label = if running {
        tr("controlPanel.stopSession")
    } else {
        match s.tab {
            SessionKind::Mirror => tr("controlPanel.startMission"),
            SessionKind::Camera => tr("controlPanel.engageCamera"),
            SessionKind::Desktop => tr("controlPanel.ejectToDesktop"),
        }
    };
    let transport = if s.config.device.contains(':') {
        tr("sidebar.wifi")
    } else {
        tr("sidebar.usb")
    };
    let target_line = row().key("session-target").gap(8.0).center_y().child(
        text(if s.config.device.is_empty() {
            tr("ui.noDevice").to_string()
        } else {
            format!("{} · {}", s.config.device, transport)
        })
        .font(micro(12.0, 500.0, 0.0))
        .color(p.muted),
    );
    let action = el()
        .key("start-stop")
        .h(52.0)
        .radius(16.0)
        .fill(if running { hex("991b1b") } else { p.primary })
        .border(
            1.0,
            if running {
                p.danger.alpha(0.5)
            } else {
                p.primary
            },
        )
        .center()
        .on_press(move |s: &mut Ui| {
            let device = s.config.device.clone();
            if running {
                s.stop_session(&device);
            } else {
                s.start_session();
            }
        })
        .child(
            text(start_label.to_uppercase())
                .font(micro(16.0, 900.0, 3.2))
                .color(if running { Color::WHITE } else { p.on_primary }),
        );

    let config = match s.tab {
        SessionKind::Mirror => mirror_config(s, p, k, layer, tr),
        SessionKind::Camera => camera_config(s, p, k, layer, tr),
        SessionKind::Desktop => desktop_config(s, p, k, layer, tr),
    };

    col()
        .key("center")
        .cursor(Cursor::Pointer)
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .gap(16.0)
        .child(capture)
        .child(
            section(
                "engine",
                p,
                10.0,
                {
                    let mut head = row()
                        .gap(8.0)
                        .center_y()
                        .child(
                            text(tr("controlPanel.engineConfiguration").to_uppercase())
                                .font(micro(11.0, 900.0, 1.1))
                                .color(p.muted),
                        )
                        .child(row().grow(1.0));
                    if running {
                        head = head.child(badge(p, tr("controlPanel.active"), p.success));
                    }
                    let otg = s.config.otg_pure.unwrap_or(false)
                        && (s.config.hid_keyboard.unwrap_or(false)
                            || s.config.hid_mouse.unwrap_or(false));
                    if otg {
                        head = head.child(badge(p, tr("controlPanel.otgOnly"), p.danger));
                    }
                    head
                },
                col()
                    .gap(6.0)
                    .grow(1.0)
                    .min_h(0.0)
                    .child(target_line)
                    .child(
                        config
                            .scroll(ScrollSpec {
                                stick_bottom: false,
                            })
                            .min_h(0.0)
                            .grow(1.0)
                            .shrink(),
                    ),
            )
            .grow(1.0)
            .min_h(0.0),
        )
        .child(action)
        .child(log_panel(s, p, tr))
}

fn mirror_config<F: Fn(&str) -> &'static str>(
    s: &Ui,
    p: &Palette,
    k: &Kit,
    layer: &mut Vec<El>,
    tr: &F,
) -> El {
    col()
        .key("mirror")
        .gap(10.0)
        .child(
            col()
                .gap(8.0)
                .padding(8.0)
                .radius(12.0)
                .fill(p.elevated.alpha(0.2))
                .border(1.0, p.border)
                .child(
                    text(tr("controlPanel.inputEnhancements").to_uppercase())
                        .font(micro(9.0, 900.0, 1.0))
                        .color(p.muted),
                )
                .child(
                    text(tr("controlPanel.inputEnhancementsDescription"))
                        .font(micro(8.0, 400.0, 0.0))
                        .color(p.subtle),
                )
                .child(
                    row()
                        .gap(12.0)
                        .child(checkbox(
                            p,
                            tr("controlPanel.hidKeyboard"),
                            s.config.hid_keyboard.unwrap_or(false),
                            |s| {
                                flip(&mut s.config.hid_keyboard, false);
                                s.save_config();
                            },
                        ))
                        .child(checkbox(
                            p,
                            tr("controlPanel.hidMouse"),
                            s.config.hid_mouse.unwrap_or(false),
                            |s| {
                                flip(&mut s.config.hid_mouse, false);
                                s.save_config();
                            },
                        ))
                        .child(checkbox(
                            p,
                            tr("controlPanel.pureHid"),
                            s.config.otg_pure.unwrap_or(false),
                            |s| {
                                flip(&mut s.config.otg_pure, false);
                                s.save_config();
                            },
                        )),
                ),
        )
        .child(perf_grid(s, p, k, layer, tr, true, false))
        .child(bitrate(s, p, tr))
}

fn camera_config<F: Fn(&str) -> &'static str>(
    s: &Ui,
    p: &Palette,
    k: &Kit,
    layer: &mut Vec<El>,
    tr: &F,
) -> El {
    col()
        .key("camera")
        .gap(10.0)
        .child(
            col()
                .gap(6.0)
                .padding(8.0)
                .radius(12.0)
                .fill(p.primary.alpha(0.05))
                .border(1.0, p.primary.alpha(0.2))
                .child(
                    text(tr("controlPanel.webcamProTip").to_uppercase())
                        .font(micro(10.0, 900.0, 1.0))
                        .color(p.primary),
                )
                .child(
                    text(format!(
                        "{} {} {} {} {}",
                        tr("controlPanel.webcamProTipTextBefore"),
                        tr("controlPanel.webcamProTipObs"),
                        tr("controlPanel.webcamProTipAndStart"),
                        tr("controlPanel.webcamProTipVirtualCamera"),
                        tr("controlPanel.webcamProTipTextAfter")
                    ))
                    .font(micro(10.0, 400.0, 0.0))
                    .color(p.muted)
                    .wrap(),
                ),
        )
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    text(tr("controlPanel.cameraDevice").to_uppercase())
                        .font(micro(9.0, 900.0, 0.8))
                        .color(p.muted)
                        .grow(1.0),
                )
                .child(select(
                    s,
                    p,
                    k,
                    layer,
                    "lens",
                    LENSES,
                    s.config.camera_id.as_deref().unwrap_or("Auto Select"),
                    tr("controlPanel.cameraDevice"),
                    200.0,
                )),
        )
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    text(tr("controlPanel.aspect").to_uppercase())
                        .font(micro(9.0, 900.0, 0.8))
                        .color(p.muted)
                        .grow(1.0),
                )
                .child(select(
                    s,
                    p,
                    k,
                    layer,
                    "aspect",
                    ASPECT,
                    s.config.camera_ar.as_deref().unwrap_or("0"),
                    tr("controlPanel.aspect"),
                    120.0,
                )),
        )
        .child(
            row()
                .gap(16.0)
                .child(checkbox(
                    p,
                    tr("controlPanel.cameraTorch"),
                    s.config.camera_torch.unwrap_or(false),
                    |s| {
                        flip(&mut s.config.camera_torch, false);
                        s.save_config();
                    },
                ))
                .child(slider_row(
                    p,
                    tr("controlPanel.cameraZoom"),
                    &format!("{:.1}x", s.config.camera_zoom.unwrap_or(1.0)),
                    s.config.camera_zoom.unwrap_or(1.0) / 5.0,
                    "zoom",
                    s,
                )),
        )
        .child(perf_grid(s, p, k, layer, tr, false, true))
        .child(bitrate(s, p, tr))
}

fn desktop_config<F: Fn(&str) -> &'static str>(
    s: &Ui,
    p: &Palette,
    k: &Kit,
    layer: &mut Vec<El>,
    tr: &F,
) -> El {
    col()
        .key("desktop")
        .gap(10.0)
        .child(
            col()
                .gap(8.0)
                .padding(10.0)
                .radius(12.0)
                .fill(p.elevated.alpha(0.4))
                .border(1.0, p.border)
                .child(
                    row()
                        .gap(8.0)
                        .center_y()
                        .child(
                            text(tr("controlPanel.virtualDisplayEngine").to_uppercase())
                                .font(micro(10.0, 900.0, 1.0))
                                .color(p.muted)
                                .grow(1.0),
                        )
                        .child(badge(p, tr("controlPanel.flexDisplay"), p.primary)),
                )
                .child(
                    row()
                        .gap(16.0)
                        .child(slider_row(
                            p,
                            tr("controlPanel.width"),
                            &s.config.vd_width.unwrap_or(1920).to_string(),
                            s.config.vd_width.unwrap_or(1920) as f32 / 3840.0,
                            "vdWidth",
                            s,
                        ))
                        .child(slider_row(
                            p,
                            tr("controlPanel.height"),
                            &s.config.vd_height.unwrap_or(1080).to_string(),
                            s.config.vd_height.unwrap_or(1080) as f32 / 2160.0,
                            "vdHeight",
                            s,
                        )),
                )
                .child(
                    row()
                        .gap(16.0)
                        .child(slider_row(
                            p,
                            tr("controlPanel.uiScaling"),
                            &s.config.vd_dpi.unwrap_or(420).to_string(),
                            s.config.vd_dpi.unwrap_or(420) as f32 / 640.0,
                            "vdDpi",
                            s,
                        ))
                        .child(
                            row()
                                .gap(8.0)
                                .center_y()
                                .child(
                                    text(tr("controlPanel.quickPresets").to_uppercase())
                                        .font(micro(8.0, 900.0, 0.6))
                                        .color(p.muted),
                                )
                                .child(select(
                                    s,
                                    p,
                                    k,
                                    layer,
                                    "preset",
                                    PRESETS,
                                    preset_name(
                                        s.config.vd_width.unwrap_or(0),
                                        s.config.vd_height.unwrap_or(0),
                                    ),
                                    tr("controlPanel.quickPresets"),
                                    140.0,
                                )),
                        ),
                ),
        )
        .child(perf_grid(s, p, k, layer, tr, false, true))
        .child(bitrate(s, p, tr))
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    text(tr("controlPanel.backgroundColor").to_uppercase())
                        .font(micro(9.0, 900.0, 0.8))
                        .color(p.muted)
                        .grow(1.0),
                )
                .child(el().size(20.0, 20.0).radius(4.0).fill(safe_hex(
                    s.config.background_color.as_deref().unwrap_or("#000000"),
                )))
                .child(
                    TextInput::new(&s.background_color)
                        .plain()
                        .label("Background color")
                        .width(Some(100.0))
                        .on_edit(|s: &mut Ui| &mut s.background_color)
                        .on_submit(|s: &mut Ui, _| {
                            let value = s.background_color.text.clone();
                            if valid_hex(&value) {
                                s.config.background_color = Some(value);
                                s.save_config();
                            }
                        })
                        .build(k),
                )
                .child(text_button(
                    p,
                    tr("controlPanel.backgroundColorNone"),
                    p.subtle,
                    8.0,
                    |s: &mut Ui| {
                        s.config.background_color = None;
                        s.background_color.replace_text("");
                    },
                )),
        )
}

fn perf_grid<F: Fn(&str) -> &'static str>(
    s: &Ui,
    p: &Palette,
    k: &Kit,
    layer: &mut Vec<El>,
    tr: &F,
    with_res: bool,
    two_cols: bool,
) -> El {
    let sel_w = if two_cols { 268.0 } else { 176.0 };
    let mut grid = row().key("perf").gap(8.0);
    if two_cols {
        grid = grid.w_full();
    }
    let mut fields: Vec<El> = Vec::new();
    let fps_current = s
        .config
        .fps
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Auto".to_string());
    if with_res {
        fields.push(field(
            p,
            tr("controlPanel.resolution"),
            select(
                s,
                p,
                k,
                layer,
                "res",
                RES,
                s.config.res.as_deref().unwrap_or("0"),
                tr("controlPanel.resolution"),
                sel_w,
            ),
        ));
    }
    fields.push(field(
        p,
        tr("controlPanel.fps"),
        select(
            s,
            p,
            k,
            layer,
            "fps",
            FPS,
            &fps_current,
            tr("controlPanel.fps"),
            sel_w,
        ),
    ));
    fields.push(field(
        p,
        tr("controlPanel.rotation"),
        select(
            s,
            p,
            k,
            layer,
            "rotation",
            ROT,
            s.config.rotation.as_deref().unwrap_or("0"),
            tr("controlPanel.rotation"),
            sel_w,
        ),
    ));
    fields.push(field(
        p,
        tr("controlPanel.codec"),
        select(
            s,
            p,
            k,
            layer,
            "codec",
            CODEC,
            s.config.codec.as_deref().unwrap_or("h264"),
            tr("controlPanel.codec"),
            sel_w,
        ),
    ));
    fields.push(field(
        p,
        tr("controlPanel.graphicsRenderer"),
        select(
            s,
            p,
            k,
            layer,
            "renderer",
            RENDERERS,
            s.config.render_driver.as_deref().unwrap_or("Auto"),
            tr("controlPanel.graphicsRenderer"),
            sel_w,
        ),
    ));
    fields.push(field(
        p,
        tr("controlPanel.hardwareDecoding"),
        select(
            s,
            p,
            k,
            layer,
            "hwdec",
            HWDEC,
            s.config.hwdec.as_deref().unwrap_or("auto"),
            tr("controlPanel.hardwareDecoding"),
            sel_w,
        ),
    ));

    let _ = grid;
    let mut rows = col().key("perf").gap(8.0);
    for chunk in fields.chunks(if two_cols { 2 } else { 3 }) {
        let mut r = row().gap(8.0);
        for f in chunk {
            r = r.child(f.clone());
        }
        rows = rows.child(r);
    }
    rows.child(
        row()
            .gap(8.0)
            .center_y()
            .child(checkbox(
                p,
                tr("controlPanel.vsync"),
                s.config.vsync.unwrap_or(true),
                |s| {
                    flip(&mut s.config.vsync, true);
                    s.save_config();
                },
            ))
            .child(checkbox(
                p,
                tr("controlPanel.ignoreEncoderConstraints"),
                s.config.ignore_video_encoder_constraints.unwrap_or(false),
                |s| {
                    flip(&mut s.config.ignore_video_encoder_constraints, false);
                    s.save_config();
                },
            )),
    )
}

fn right<F: Fn(&str) -> &'static str>(
    s: &Ui,
    p: &Palette,
    k: &Kit,
    layer: &mut Vec<El>,
    tr: &F,
) -> El {
    let mut toggles = col().key("behavior-list").gap(2.0);
    let rows: [ToggleRow; 9] = [
        (
            "sessionBehavior.stayAwake",
            s.config.stay_awake.unwrap_or(false),
            |s| {
                flip(&mut s.config.stay_awake, false);
                s.save_config();
            },
        ),
        (
            "sessionBehavior.keepActive",
            s.config.keep_active.unwrap_or(false),
            |s| {
                flip(&mut s.config.keep_active, false);
                s.save_config();
            },
        ),
        (
            "sessionBehavior.screenOff",
            s.config.turn_off.unwrap_or(false),
            |s| {
                flip(&mut s.config.turn_off, false);
                s.save_config();
            },
        ),
        (
            "sessionBehavior.forwardAudio",
            s.config.audio_enabled.unwrap_or(true),
            |s| {
                flip(&mut s.config.audio_enabled, true);
                s.save_config();
            },
        ),
        (
            "sessionBehavior.alwaysOnTop",
            s.config.always_on_top.unwrap_or(false),
            |s| {
                flip(&mut s.config.always_on_top, false);
                s.save_config();
            },
        ),
        (
            "sessionBehavior.fullScreen",
            s.config.fullscreen.unwrap_or(false),
            |s| {
                flip(&mut s.config.fullscreen, false);
                s.save_config();
            },
        ),
        (
            "sessionBehavior.borderless",
            s.config.borderless.unwrap_or(false),
            |s| {
                flip(&mut s.config.borderless, false);
                s.save_config();
            },
        ),
        (
            "sessionBehavior.rememberWindowPosition",
            s.remember_position,
            |s: &mut Ui| s.toggle_remember_position(),
        ),
        (
            "sessionBehavior.recordFeed",
            s.config.record.unwrap_or(false),
            |s| {
                flip(&mut s.config.record, false);
                s.save_config();
            },
        ),
    ];
    for (key, on, handler) in rows {
        toggles = toggles.child(toggle_row(
            p,
            tr(key),
            on,
            key == "sessionBehavior.recordFeed",
            handler,
        ));
    }
    toggles = toggles.child(
        row()
            .gap(8.0)
            .center_y()
            .child(eyebrow(p, tr("sessionBehavior.audioCodec"), 8.0))
            .child(select(
                s,
                p,
                k,
                layer,
                "audio",
                AUDIO_CODEC,
                s.config.audio_codec.as_deref().unwrap_or("auto"),
                tr("sessionBehavior.audioCodec"),
                140.0,
            )),
    );

    let record_path = s.config.record_path.as_deref().unwrap_or("");
    if s.config.record.unwrap_or(false) {
        toggles = toggles.child(
            col()
                .gap(4.0)
                .pt(8.0)
                .child(
                    row()
                        .gap(8.0)
                        .center_y()
                        .child(icon(&rune_kit::glyphs::FOLDER, 12.0).color(p.subtle))
                        .child(eyebrow(p, tr("sessionBehavior.recordPath"), 9.0))
                        .child(row().grow(1.0))
                        .child(folder_button(p, tr("sessionBehavior.change"), |s, path| {
                            s.set_record_path(path.to_string_lossy().to_string());
                        })),
                )
                .child(
                    row()
                        .padding(8.0)
                        .radius(8.0)
                        .fill(p.elevated.alpha(0.4))
                        .border(1.0, p.border.alpha(0.5))
                        .child(
                            text(if record_path.is_empty() {
                                tr("sessionBehavior.defaultVideosFolder")
                            } else {
                                record_path
                            })
                            .font(mono(9.0))
                            .color(p.subtle)
                            .truncate(),
                        ),
                ),
        );
    }

    let mut shortcuts = col().key("shortcuts-grid").gap(4.0);
    let keys: [(&str, &str, &rune::Icon); 8] = [
        ("F", tr("shortcuts.full"), &icons::FULLSCREEN),
        ("H", tr("shortcuts.home"), &icons::HOME),
        ("B", tr("shortcuts.back"), &icons::BACK),
        ("S", tr("shortcuts.recents"), &icons::RECENTS),
        ("P", tr("shortcuts.power"), &icons::POWER),
        ("R", tr("shortcuts.rotate"), &icons::ROTATE),
        ("V", tr("shortcuts.paste"), &icons::PASTE),
        ("O", tr("shortcuts.off"), &icons::OFF),
    ];
    for pair in keys.chunks(2) {
        let mut r = row().gap(8.0);
        for (key, label, ico) in pair {
            r = r.child(
                row()
                    .key(*key)
                    .help(format!("Alt+{key}: {label}"))
                    .h(36.0)
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(8.0)
                    .px(8.0)
                    .center_y()
                    .fill(p.elevated.alpha(0.3))
                    .radius(8.0)
                    .child(
                        row()
                            .size(24.0, 24.0)
                            .center()
                            .radius(6.0)
                            .fill(p.input)
                            .child(icon(ico, 14.0).color(p.muted)),
                    )
                    .child(
                        text(*label)
                            .font(micro(12.0, 500.0, 0.0))
                            .color(p.muted)
                            .grow(1.0)
                            .truncate(),
                    )
                    .child(text(*key).font(micro(10.0, 500.0, 0.0)).color(p.subtle)),
            );
        }
        shortcuts = shortcuts.child(r);
    }

    col()
        .key("right")
        .cursor(Cursor::Pointer)
        .w(270.0)
        .min_h(0.0)
        .gap(16.0)
        .shrink()
        .child(section(
            "behavior",
            p,
            16.0,
            row()
                .gap(8.0)
                .center_y()
                .child(icon(&icons::GEAR, 12.0).color(p.subtle))
                .child(eyebrow(p, tr("sessionBehavior.title"), 10.0)),
            toggles,
        ))
        .child(section(
            "shortcuts",
            p,
            16.0,
            row()
                .gap(8.0)
                .center_y()
                .child(icon(&rune_kit::glyphs::KEYBOARD, 14.0).color(p.subtle))
                .child(eyebrow(p, tr("shortcuts.title"), 13.0).color(p.text)),
            shortcuts,
        ))
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
}

fn log_panel<F: Fn(&str) -> &'static str>(s: &Ui, p: &Palette, tr: &F) -> El {
    // The console is force-dark in both themes, so all of its text must use a
    // dark-theme Kit regardless of the surrounding palette.
    let console_kit = kit(&Palette::new("ultraviolet", true));
    let mut lines = col().key("logs").gap(1.0);
    if s.logs.is_empty() {
        lines = lines.child(
            text(tr("logPanel.waitingForSequence").to_uppercase())
                .font(micro(10.0, 700.0, 1.6))
                .color(hex("a1a1aa")),
        );
    }
    for (i, (time, line)) in s.logs.iter().enumerate() {
        lines = lines.child(
            row()
                .key(format!("log-{i}"))
                .gap(12.0)
                .pl(12.0)
                .child(text(time.clone()).font(mono(11.0)).color(hex("a1a1aa")))
                .child(
                    text(line.clone())
                        .font(mono(11.0))
                        .color(hex("d4d4d8"))
                        .wrap(),
                ),
        );
    }
    col()
        .key("console")
        .h(220.0)
        .radius(16.0)
        .padding(10.0)
        .gap(6.0)
        .fill(hex("09090b").alpha(0.94))
        .radial_gradient(
            [0.0, 0.0],
            420.0,
            [(0.0, p.primary.alpha(0.03)), (1.0, Color::TRANSPARENT)],
        )
        .border(1.0, hex("ffffff").alpha(0.07))
        .child(
            row()
                .h(28.0)
                .gap(8.0)
                .center_y()
                .child(text("_").font(mono(12.0)).color(p.primary))
                .child(
                    text(tr("logPanel.systemConsole").to_uppercase())
                        .font(micro(9.0, 900.0, 1.8))
                        .color(hex("a1a1aa"))
                        .grow(1.0),
                )
                .child(text_button(
                    p,
                    tr("logPanel.report"),
                    p.primary,
                    9.0,
                    |s: &mut Ui| s.export_report(),
                ))
                .child(text_button(
                    p,
                    tr("logPanel.clear"),
                    hex("a1a1aa"),
                    9.0,
                    |s: &mut Ui| s.logs.clear(),
                )),
        )
        .child(
            lines
                .scroll(ScrollSpec { stick_bottom: true })
                .grow(1.0)
                .shrink()
                .min_h(0.0),
        )
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(text("$").font(mono(11.0)).color(p.primary))
                .child(
                    TextInput::new(&s.command)
                        .plain()
                        .label("Command")
                        .placeholder(tr("logPanel.terminalPlaceholder"))
                        .width(None)
                        .on_edit(|s: &mut Ui| &mut s.command)
                        .on_submit(|s: &mut Ui, _| {
                            let cmd = s.command.text.clone();
                            s.run_command(cmd);
                            s.command.replace_text("");
                        })
                        .build(&console_kit),
                ),
        )
}

fn footer<F: Fn(&str) -> &'static str>(p: &Palette, tr: &F) -> El {
    let link = |label: &str, url: &str, tint: Color| {
        let target = url.to_string();
        text(label.to_uppercase())
            .role("button")
            .label(label)
            .font(micro(11.0, 500.0, 0.0))
            .color(tint)
            .py(6.0)
            .hover(p.primary.alpha(0.08))
            .on_press_tx(move |_: &mut Ui, tx: &mut rune::Tx| {
                let _ = tx.open_url(&target);
            })
    };
    let version = format!(
        "v{} · {}",
        env!("CARGO_PKG_VERSION"),
        tr("footer.createdBy")
    );
    let meta = format!(
        "{}: SCRCPY  {}: RUNE  {}: RUST",
        tr("footer.core").to_uppercase(),
        tr("footer.client").to_uppercase(),
        tr("footer.ui").to_uppercase()
    );
    col()
        .key("footer")
        .w_full()
        .h(72.0)
        .px(24.0)
        .py(12.0)
        .gap(12.0)
        .fill(p.glass)
        .border(1.0, p.border)
        .child(
            row()
                .gap(24.0)
                .center_y()
                .child(
                    text(tr("footer.aboutScrcpyGui").to_uppercase())
                        .font(micro(13.0, 600.0, 0.0))
                        .color(p.muted),
                )
                .child(
                    row()
                        .grow(1.0)
                        .gap(28.0)
                        .center()
                        .child(link(
                            tr("footer.github"),
                            "https://github.com/kil0bit-kb",
                            p.subtle,
                        ))
                        .child(link(
                            tr("footer.youtube"),
                            "https://www.youtube.com/@kilObit",
                            p.subtle,
                        ))
                        .child(link(
                            tr("footer.website"),
                            "https://kil0bit.blogspot.com/",
                            p.subtle,
                        ))
                        .child(link(
                            tr("footer.support"),
                            "https://www.patreon.com/cw/KB_kilObit",
                            hex("6d28d9"),
                        )),
                ),
        )
        .child(
            row()
                .gap(24.0)
                .center_y()
                .child(text(version).font(micro(11.0, 400.0, 0.0)).color(p.subtle))
                .child(
                    row()
                        .grow(1.0)
                        .center()
                        .child(text(meta).font(micro(11.0, 400.0, 0.0)).color(p.subtle)),
                ),
        )
}

fn onboarding<F: Fn(&str) -> &'static str>(_s: &Ui, p: &Palette, tr: &F) -> El {
    col()
        .key("onboarding")
        .cursor(Cursor::Pointer)
        .modal(true, |s: &mut Ui| s.onboarding_open = false)
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .center()
        .fill(hex("000000").alpha(0.8))
        .child(
            row()
                .key("onboarding-panel")
                .w(672.0)
                .radius(24.0)
                .fill(p.elevated)
                .border(1.0, p.border)
                .child(
                    col()
                        .w(224.0)
                        .padding(32.0)
                        .gap(16.0)
                        .fill(p.primary.alpha(0.1))
                        .child(
                            text("SCRCPY GUI")
                                .font(TextStyle::new(30.0, 900.0).italic())
                                .color(p.text),
                        )
                        .child(
                            text(tr("onboarding.setupCoreComponents").to_uppercase())
                                .font(micro(10.0, 900.0, 1.0))
                                .color(p.primary),
                        )
                        .child(
                            text(tr("onboarding.introQuote"))
                                .font(micro(10.0, 400.0, 0.0))
                                .color(p.muted)
                                .wrap(),
                        ),
                )
                .child(
                    col()
                        .grow(1.0)
                        .padding(48.0)
                        .gap(16.0)
                        .child(
                            text(tr("onboarding.setupCoreComponents").to_uppercase())
                                .font(TextStyle::new(24.0, 900.0).italic())
                                .color(p.text),
                        )
                        .child(
                            text(tr("onboarding.setupCoreSubtitle"))
                                .font(micro(14.0, 400.0, 0.0))
                                .color(p.muted),
                        )
                        .child(
                            text(tr("onboarding.binariesDescription"))
                                .font(micro(12.0, 400.0, 0.0))
                                .color(p.muted)
                                .wrap(),
                        )
                        .child(
                            solid_button(
                                p,
                                tr("onboarding.downloadCoreBinaries"),
                                p.primary,
                                p.on_primary,
                                10.0,
                                |s: &mut Ui| s.download_scrcpy(),
                            )
                            .autofocus(),
                        )
                        .child(
                            text(tr("onboarding.manualSetupFallback"))
                                .font(micro(11.0, 400.0, 0.0))
                                .color(p.subtle)
                                .wrap(),
                        )
                        .child(row().center().gap(12.0).child(solid_button(
                            p,
                            tr("onboarding.continueToApp"),
                            p.input,
                            p.subtle,
                            10.0,
                            |s: &mut Ui| s.onboarding_open = false,
                        ))),
                ),
        )
}

fn update_modal<F: Fn(&str) -> &'static str>(
    s: &Ui,
    p: &Palette,
    info: &crate::ui::state::UpdateInfo,
    tr: &F,
) -> El {
    let message = tv(
        &s.locale,
        "alerts.updateAvailableMessage",
        &[("local", &info.local), ("latest", &info.latest)],
    );
    col()
        .key("update-modal")
        .cursor(Cursor::Pointer)
        .modal(true, |s: &mut Ui| s.update = None)
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .center()
        .fill(hex("000000").alpha(0.6))
        .child(
            col()
                .key("update-panel")
                .w(448.0)
                .padding(32.0)
                .gap(16.0)
                .center()
                .radius(24.0)
                .fill(p.elevated)
                .border(1.0, p.primary.alpha(0.3))
                .child(el().h(2.0).w(200.0).fill(p.primary))
                .child(
                    text(tr("alerts.updateAvailableTitle").to_uppercase())
                        .font(TextStyle::new(20.0, 900.0).italic())
                        .color(p.text),
                )
                .child(
                    text(message)
                        .font(micro(14.0, 400.0, 0.0))
                        .color(p.muted)
                        .wrap()
                        .center(),
                )
                .child(
                    row()
                        .gap(12.0)
                        .child(solid_button(
                            p,
                            tr("alerts.cancelBtn"),
                            p.input,
                            p.muted,
                            10.0,
                            |s: &mut Ui| s.update = None,
                        ))
                        .child(
                            solid_button(
                                p,
                                tr("alerts.updateBtn"),
                                p.primary,
                                p.on_primary,
                                10.0,
                                |s: &mut Ui| s.update_now(),
                            )
                            .autofocus(),
                        ),
                ),
        )
}

fn alert_modal(_s: &Ui, p: &Palette, message: &str) -> El {
    col()
        .key("alert-modal")
        .cursor(Cursor::Pointer)
        .modal(true, |s: &mut Ui| s.alert = None)
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .center()
        .fill(hex("000000").alpha(0.6))
        .child(
            col()
                .key("alert-panel")
                .w(380.0)
                .padding(32.0)
                .gap(16.0)
                .center()
                .radius(24.0)
                .fill(p.elevated)
                .border(1.0, p.primary.alpha(0.3))
                .child(
                    text(message)
                        .font(micro(14.0, 400.0, 0.0))
                        .color(p.muted)
                        .wrap()
                        .center(),
                )
                .child(
                    solid_button(p, "OK", p.primary, p.on_primary, 10.0, |s: &mut Ui| {
                        s.alert = None
                    })
                    .autofocus(),
                ),
        )
}

fn section(key: &str, p: &Palette, padding: f32, title: El, body: El) -> El {
    col()
        .key(key)
        .gap(8.0)
        .padding(padding)
        .fill(p.glass)
        .border(1.0, p.glass_border)
        .radius(12.0)
        .child(
            col()
                .gap(8.0)
                .child(title)
                .child(el().h(1.0).w_full().fill(p.border.alpha(0.5))),
        )
        .child(body)
}

fn section_label(p: &Palette, value: &str, accent: bool) -> El {
    col()
        .gap(6.0)
        .child(
            text(value.to_uppercase())
                .font(micro(9.0, 900.0, 0.9))
                .color(if accent {
                    p.primary.alpha(0.6)
                } else {
                    p.muted
                }),
        )
        .child(el().h(1.0).w_full().fill(p.border.alpha(0.5)))
}

fn tip_card(p: &Palette, text_value: &str) -> El {
    col()
        .gap(6.0)
        .padding(12.0)
        .radius(8.0)
        .fill(p.primary.alpha(0.05))
        .border(1.0, p.primary.alpha(0.2))
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(el().size(4.0, 4.0).radius(2.0).fill(p.primary))
                .child(eyebrow(p, text_value, 9.0).color(p.primary)),
        )
}

fn field(p: &Palette, label: &str, control: El) -> El {
    col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(
            text(label.to_uppercase())
                .font(micro(9.0, 900.0, 0.2))
                .color(p.subtle),
        )
        .child(control)
}

fn badge(_p: &Palette, label: &str, tint: Color) -> El {
    text(label.to_uppercase())
        .font(micro(8.0, 700.0, 0.4))
        .color(tint)
        .padding(3.0)
        .radius(4.0)
        .fill(tint.alpha(0.1))
        .border(1.0, tint.alpha(0.25))
}

fn text_button(
    _p: &Palette,
    label: &str,
    tint: Color,
    size: f32,
    f: impl Fn(&mut Ui) + 'static,
) -> El {
    text(label.to_uppercase())
        .role("button")
        .label(label)
        .font(micro(size, 900.0, 0.5))
        .color(tint)
        .py(8.0)
        .px(6.0)
        .radius(4.0)
        .help(label)
        .on_press(f)
        .hover(tint.alpha(0.08))
        .press(tint.alpha(0.16))
}

fn ghost(p: &Palette, label: &str, size: f32, f: impl Fn(&mut Ui) + 'static) -> El {
    text(label.to_uppercase())
        .role("button")
        .label(label)
        .font(micro(size, 900.0, 0.4))
        .color(p.subtle)
        .py(9.0)
        .px(6.0)
        .radius(6.0)
        .border(1.0, p.border)
        .on_press(f)
        .hover(p.primary.alpha(0.08))
        .press(p.primary.alpha(0.16))
}

fn folder_button(
    p: &Palette,
    label: &str,
    f: impl Fn(&mut Ui, std::path::PathBuf) + Clone + 'static,
) -> El {
    let title = label.to_string();
    text(label.to_uppercase())
        .role("button")
        .label(label)
        .font(micro(9.0, 900.0, 0.4))
        .color(p.subtle)
        .py(8.0)
        .px(6.0)
        .radius(6.0)
        .border(1.0, p.border)
        .hover(p.primary.alpha(0.08))
        .press(p.primary.alpha(0.16))
        .on_press_tx(move |_s: &mut Ui, tx: &mut rune::Tx| {
            let opts = rune::FileDialogOptions {
                title: title.clone(),
                ..Default::default()
            };
            let choose = f.clone();
            tx.open_folder(
                opts,
                move |s: &mut Ui, result: rune::file_dialog::FileDialogResult, _| {
                    if let Ok(Some(path)) = result {
                        choose(s, path);
                    }
                },
            );
        })
}

fn segment(p: &Palette, label: &str, active: bool, f: impl Fn(&mut Ui) + 'static) -> El {
    row()
        .grow(1.0)
        .h(28.0)
        .center()
        .gap(8.0)
        .radius(6.0)
        .fill(if active {
            p.primary
        } else {
            Color::TRANSPARENT
        })
        .on_press(f)
        .child(
            text(label.to_uppercase())
                .font(micro(9.0, 900.0, 0.9))
                .color(if active { p.on_primary } else { p.subtle }),
        )
}

fn solid_button(
    p: &Palette,
    label: &str,
    fill: Color,
    ink: Color,
    size: f32,
    f: impl Fn(&mut Ui) + 'static,
) -> El {
    row()
        .h(30.0)
        .padding(12.0)
        .center()
        .radius(8.0)
        .fill(fill)
        .on_press(f)
        .hover(fill.mix(p.text, 0.08))
        .press(fill.mix(p.text, 0.16))
        .child(
            text(label.to_uppercase())
                .font(micro(size, 900.0, 1.0))
                .color(ink),
        )
}

fn checkbox(p: &Palette, label: &str, checked: bool, f: impl Fn(&mut Ui) + 'static) -> El {
    row()
        .role("checkbox")
        .label(label)
        .checked(checked)
        .min_h(28.0)
        .gap(8.0)
        .center_y()
        .on_press(f)
        .hover(p.primary.alpha(0.06))
        .child(
            el().size(14.0, 14.0)
                .radius(4.0)
                .fill(if checked {
                    p.primary
                } else {
                    Color::TRANSPARENT
                })
                .border(1.0, if checked { p.primary } else { p.border_subtle })
                .child(if checked {
                    el().size(6.0, 6.0)
                        .radius(1.0)
                        .center()
                        .self_center()
                        .fill(p.on_primary)
                } else {
                    el().size(0.0, 0.0)
                }),
        )
        .child(
            text(label.to_uppercase())
                .font(micro(10.0, 700.0, 0.4))
                .color(if checked { p.text } else { p.muted }),
        )
}

fn toggle_row(
    p: &Palette,
    label: &str,
    on: bool,
    danger: bool,
    f: impl Fn(&mut Ui) + 'static,
) -> El {
    let tint = if danger { p.danger } else { p.primary };
    row()
        .role("switch")
        .label(label)
        .checked(on)
        .h(32.0)
        .gap(8.0)
        .px(8.0)
        .center_y()
        .radius(8.0)
        .border(1.0, Color::TRANSPARENT)
        .on_press(f)
        .hover(p.primary.alpha(0.06))
        .child(
            row()
                .size(20.0, 20.0)
                .radius(6.0)
                .center()
                .fill(if on {
                    tint.alpha(0.1)
                } else {
                    p.input.alpha(0.5)
                })
                .child(
                    el().size(8.0, 8.0)
                        .radius(4.0)
                        .fill(if on { tint } else { p.subtle }),
                ),
        )
        .child(
            text(label.to_uppercase())
                .font(micro(10.0, 700.0, -0.2))
                .truncate()
                .color(if on { p.text } else { p.muted })
                .grow(1.0),
        )
        .child(
            el().key(format!("switch-{label}"))
                .size(24.0, 14.0)
                .radius(7.0)
                .fill(if on { tint } else { p.input })
                .padding(2.0)
                .child(
                    el().size(10.0, 10.0)
                        .radius(5.0)
                        .absolute(if on { 12.0 } else { 2.0 }, 2.0)
                        .fill(if on { p.on_primary } else { Color::WHITE }),
                ),
        )
}

fn bitrate<F: Fn(&str) -> &'static str>(s: &Ui, p: &Palette, tr: &F) -> El {
    slider_row(
        p,
        tr("controlPanel.bitrate"),
        &format!("{}M", s.config.bitrate.unwrap_or(8)),
        s.config.bitrate.unwrap_or(8) as f32 / 50.0,
        "bitrate",
        s,
    )
}

fn slider_row(
    p: &Palette,
    label: &str,
    value: &str,
    fraction: f32,
    field: &'static str,
    _s: &Ui,
) -> El {
    col()
        .key(format!("slider-{field}"))
        .grow(1.0)
        .min_w(180.0)
        .gap(6.0)
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    text(label.to_uppercase())
                        .font(micro(9.0, 900.0, 0.9))
                        .color(p.subtle),
                )
                .child(text(value).font(micro(10.0, 900.0, 0.0)).color(p.primary))
                .child(row().grow(1.0)),
        )
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    el().key(format!("slider-track-{field}"))
                        .grow(1.0)
                        .h(4.0)
                        .radius(2.0)
                        .fill(p.input)
                        .child(
                            el().w_frac(fraction.clamp(0.0, 1.0))
                                .h(4.0)
                                .radius(2.0)
                                .fill(p.primary)
                                .justify_end()
                                .child(
                                    el().size(14.0, 14.0)
                                        .radius(7.0)
                                        .self_center()
                                        .fill(p.primary)
                                        .border(2.0, p.bg),
                                ),
                        ),
                )
                .child(step_button(p, "-", field, -1))
                .child(step_button(p, "+", field, 1)),
        )
}

fn valid_hex(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn safe_hex(value: &str) -> Color {
    if valid_hex(value) {
        hex(value)
    } else {
        hex("#000000")
    }
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

fn step_button(p: &Palette, label: &str, field: &'static str, delta: i32) -> El {
    row()
        .role("button")
        .label(label)
        .size(28.0, 28.0)
        .center()
        .radius(6.0)
        .fill(p.input)
        .on_press(move |s: &mut Ui| step_config(s, field, delta))
        .hover(p.primary.alpha(0.1))
        .press(p.primary.alpha(0.18))
        .child(text(label).font(micro(11.0, 700.0, 0.0)).color(p.muted))
}

fn input(
    p: &Palette,
    k: &Kit,
    edit: &rune::TextEdit,
    placeholder: &str,
    key: &str,
    display: &str,
) -> El {
    let field = key.to_string();
    col()
        .gap(4.0)
        .grow(1.0)
        .min_w(0.0)
        .child(text(display).font(micro(11.0, 500.0, 0.0)).color(p.muted))
        .child(
            row()
                .h(36.0)
                .grow(1.0)
                .min_w(0.0)
                .padding(12.0)
                .center_y()
                .radius(8.0)
                .fill(p.elevated.alpha(0.4))
                .border(1.0, p.border)
                .child(
                    TextInput::new(edit)
                        .plain()
                        .label(display)
                        .text_style(TextStyle::new(13.0, 400.0).lh(20.0))
                        .height(20.0)
                        .placeholder(placeholder)
                        .width(None)
                        .on_edit(move |s: &mut Ui| match field.as_str() {
                            "IP" => &mut s.ip,
                            "Pair IP" => &mut s.pair_ip,
                            _ => &mut s.pair_code,
                        })
                        .build(k),
                ),
        )
}

fn select(
    s: &Ui,
    _p: &Palette,
    k: &Kit,
    layer: &mut Vec<El>,
    key: &'static str,
    options: &[&str],
    current: &str,
    label: &str,
    width: f32,
) -> El {
    Select::new(key, options)
        .width(width)
        .current(Some(current))
        .label(label)
        .appearance(SelectAppearance::Field)
        .open(s.open_select.as_deref() == Some(key))
        .on_toggle(move |s: &mut Ui| toggle_select(s, key))
        .on_close(|s: &mut Ui| s.open_select = None)
        .on_choose(move |s: &mut Ui, v: &str, _| apply_choice(s, key, v))
        .build(k, layer)
        .grow(1.0)
        .min_w(0.0)
}

fn toggle_select(s: &mut Ui, key: &str) {
    s.open_select = if s.open_select.as_deref() == Some(key) {
        None
    } else {
        Some(key.to_string())
    };
}

fn flip(field: &mut Option<bool>, fallback: bool) {
    *field = Some(!field.unwrap_or(fallback));
}

fn step_config(s: &mut Ui, field: &str, delta: i32) {
    let clamp = |v: u32, min: u32, max: u32| -> u32 {
        (v as i32 + delta).clamp(min as i32, max as i32) as u32
    };
    match field {
        "bitrate" => s.config.bitrate = Some(clamp(s.config.bitrate.unwrap_or(8), 1, 50)),
        "zoom" => {
            s.config.camera_zoom = Some(
                clamp((s.config.camera_zoom.unwrap_or(1.0) * 10.0) as u32, 10, 50) as f32 / 10.0,
            )
        }
        "vdWidth" => s.config.vd_width = Some(clamp(s.config.vd_width.unwrap_or(1920), 480, 3840)),
        "vdHeight" => {
            s.config.vd_height = Some(clamp(s.config.vd_height.unwrap_or(1080), 360, 2160))
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
        "lens" => {
            s.config.camera_id = if v == "Auto Select" {
                None
            } else {
                Some(v.into())
            }
        }
        "locale" => s.set_locale(v),
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
