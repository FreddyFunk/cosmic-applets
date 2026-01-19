// Copyright 2025 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use crate::pipewire_monitor::{DeviceType, DeviceUsage, PipeWireEvent, check_camera_proc, pipewire_subscription};
use cosmic::cosmic_theme::palette::WithAlpha;
use cosmic::iced::{Background, Border, Subscription};
use cosmic::theme::{Container, Svg, Theme};
use cosmic::widget::container::Style as ContainerStyle;
use cosmic::widget::svg::Style as SvgStyle;
use cosmic::widget::{icon, layer_container, Column, Row};
use cosmic::{app, Application, Apply, Element, Task};
use cosmic_time::{anim, chain, Instant, Timeline};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use std::sync::LazyLock;
use std::time::Duration;

const APP_ID: &str = "com.system76.CosmicAppletPrivacy";

static REC_ICON: LazyLock<crate::rec_icon::Id> = LazyLock::new(crate::rec_icon::Id::unique);

pub fn run() -> cosmic::iced::Result {
    cosmic::applet::run::<PrivacyIndicator>(())
}

#[derive(Default)]
struct Shared {
    pub microphone: bool,
    pub screenshare: bool,
    pub camera: bool,
}

#[derive(Default)]
pub struct PrivacyIndicator {
    core: cosmic::app::Core,
    timeline: Timeline,
    shared: Shared,
    /// Active devices from PipeWire, keyed by node_id
    pipewire_devices: FxHashMap<u32, DeviceUsage>,
    /// Active cameras from /proc scanning, keyed by PID
    proc_cameras: FxHashMap<u32, DeviceUsage>,
}

impl PrivacyIndicator {
    fn has_device_type(&self, device_type: DeviceType) -> bool {
        self.pipewire_devices.values().any(|d| d.device_type == device_type)
            || self.proc_cameras.values().any(|d| d.device_type == device_type)
    }

    fn update_shared(&mut self) {
        let old_shared = (self.shared.camera, self.shared.microphone, self.shared.screenshare);
        self.shared = Shared {
            microphone: self.has_device_type(DeviceType::Microphone),
            screenshare: self.has_device_type(DeviceType::ScreenShare)
                || self.has_device_type(DeviceType::ScreenRecord),
            camera: self.has_device_type(DeviceType::Camera) || !self.proc_cameras.is_empty(),
        };
        let new_shared = (self.shared.camera, self.shared.microphone, self.shared.screenshare);
        if old_shared != new_shared {
            tracing::debug!(
                "Shared state changed: camera={}, mic={}, screen={}",
                self.shared.camera, self.shared.microphone, self.shared.screenshare
            );
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    RecTick(Instant),
    PipeWire(PipeWireEvent),
}

impl Application for PrivacyIndicator {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &cosmic::app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::app::Core {
        &mut self.core
    }

    fn init(core: cosmic::app::Core, _flags: Self::Flags) -> (Self, app::Task<Self::Message>) {
        tracing::debug!("Privacy applet initializing...");
        let mut timeline = Timeline::new();
        timeline.set_chain(chain![REC_ICON]).start();

        (
            Self {
                core,
                timeline,
                ..Default::default()
            },
            Task::none(),
        )
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let horizontal = self.core.applet.is_horizontal();
        let size = self.core.applet.suggested_size(true);
        let pad = self.core.applet.suggested_padding(true);

        let Shared {
            microphone,
            screenshare,
            camera,
        } = self.shared;

        // If nothing active, return empty (hides applet)
        if !screenshare && !microphone && !camera {
            return "".into();
        }

        let mut icons: Vec<Element<Self::Message>> = vec![];

        // Animated recording indicator
        icons.push(anim![REC_ICON, &self.timeline, size.0].into());

        // Icon styling to match theme
        let icon_style = Rc::new(|theme: &Theme| SvgStyle {
            color: Some(theme.cosmic().button_color().into()),
        });
        let indicator = |name: &str| {
            icon(icon::from_name(name).into())
                .class(Svg::Custom(icon_style.clone()))
                .size(size.0)
        };

        if camera {
            icons.push(indicator("camera-web-symbolic").into());
        }
        if microphone {
            icons.push(indicator("audio-input-microphone-symbolic").into());
        }
        if screenshare {
            icons.push(indicator("accessories-screenshot-symbolic").into());
        }

        // Container styling with semi-transparent background
        let container_style = |theme: &Theme| {
            let cosmic = theme.cosmic();
            ContainerStyle {
                background: Some(Background::Color(
                    cosmic.primary.base.with_alpha(0.5).into(),
                )),
                border: Border {
                    radius: cosmic.corner_radii.radius_xl.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        };

        let container = if horizontal {
            Row::with_children(icons)
                .spacing(pad.0)
                .apply(layer_container)
        } else {
            Column::with_children(icons)
                .spacing(pad.1)
                .apply(layer_container)
        }
        .padding([pad.1, pad.0])
        .class(Container::Custom(Box::new(container_style)));

        self.core.applet.autosize_window(container).into()
    }

    fn update(&mut self, message: Self::Message) -> app::Task<Self::Message> {
        match message {
            Message::Tick => {
                // Check for cameras using /proc (fallback for non-PipeWire camera access)
                let old_count = self.proc_cameras.len();
                self.proc_cameras = check_camera_proc();
                let new_count = self.proc_cameras.len();
                if new_count != old_count {
                    tracing::debug!("Camera count changed: {} -> {}", old_count, new_count);
                    for (pid, usage) in &self.proc_cameras {
                        tracing::debug!("  Camera: {} (PID {}) using {}", usage.app_name, pid, usage.device_name);
                    }
                }
                self.update_shared();
            }
            Message::RecTick(now) => {
                self.timeline.now(now);
            }
            Message::PipeWire(event) => match event {
                PipeWireEvent::DeviceAdded(usage) => {
                    tracing::debug!(
                        "Device added: {} using {}",
                        usage.app_name,
                        usage.device_name
                    );
                    self.pipewire_devices.insert(usage.node_id, usage);
                    self.update_shared();
                }
                PipeWireEvent::DeviceRemoved(node_id) => {
                    if let Some(usage) = self.pipewire_devices.remove(&node_id) {
                        tracing::debug!(
                            "Device removed: {} was using {}",
                            usage.app_name,
                            usage.device_name
                        );
                    }
                    self.update_shared();
                }
                PipeWireEvent::Error(err) => {
                    tracing::error!("PipeWire error: {}", err);
                }
            },
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::batch([
            // PipeWire device monitoring
            pipewire_subscription().map(Message::PipeWire),
            // Timeline animation at 50Hz (like the community extension)
            cosmic::iced::time::every(Duration::from_millis(20)).map(Message::RecTick),
            // Camera polling via /proc (every 2 seconds)
            cosmic::iced::time::every(Duration::from_secs(2)).map(|_| Message::Tick),
        ])
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}
