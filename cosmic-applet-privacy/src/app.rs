// Copyright 2025 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use crate::pipewire_monitor::{DeviceType, DeviceUsage, PipeWireEvent, check_camera_proc, pipewire_subscription};
use cosmic::iced::Subscription;
use cosmic::theme::{Svg, Theme};
use cosmic::widget::svg::Style as SvgStyle;
use cosmic::widget::{icon, layer_container, Column, Row};
use cosmic::{app, Application, Apply, Element, Task};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

const APP_ID: &str = "com.system76.CosmicAppletPrivacy";

/// How long to keep showing an indicator after device stops being used
const COOLDOWN_DURATION: Duration = Duration::from_secs(10);

pub fn run() -> cosmic::iced::Result {
    cosmic::applet::run::<PrivacyIndicator>(())
}

/// State for each indicator type
#[derive(Debug, Clone, Copy)]
struct IndicatorState {
    /// Whether the device is currently active
    active: bool,
    /// When the device was last active (for cooldown)
    last_active: Option<Instant>,
}

impl Default for IndicatorState {
    fn default() -> Self {
        Self {
            active: false,
            last_active: None,
        }
    }
}

impl IndicatorState {
    /// Returns true if the indicator should be visible (active or in cooldown)
    fn should_show(&self) -> bool {
        if self.active {
            return true;
        }
        if let Some(last) = self.last_active {
            return last.elapsed() < COOLDOWN_DURATION;
        }
        false
    }

    /// Update state based on whether device is currently active
    fn update(&mut self, is_active: bool) {
        if is_active {
            self.active = true;
            self.last_active = Some(Instant::now());
        } else if self.active {
            // Transition from active to inactive - start cooldown
            self.active = false;
            self.last_active = Some(Instant::now());
        }
    }
}

#[derive(Default)]
pub struct PrivacyIndicator {
    core: cosmic::app::Core,
    /// Active devices from PipeWire, keyed by node_id
    pipewire_devices: FxHashMap<u32, DeviceUsage>,
    /// Active cameras from /proc scanning, keyed by PID
    proc_cameras: FxHashMap<u32, DeviceUsage>,
    /// State for each indicator type
    camera_state: IndicatorState,
    microphone_state: IndicatorState,
    screenshare_state: IndicatorState,
    screenrecord_state: IndicatorState,
}

impl PrivacyIndicator {
    fn has_device_type(&self, device_type: DeviceType) -> bool {
        self.pipewire_devices.values().any(|d| d.device_type == device_type)
            || self.proc_cameras.values().any(|d| d.device_type == device_type)
    }

    fn update_states(&mut self) {
        let camera_active = self.has_device_type(DeviceType::Camera) || !self.proc_cameras.is_empty();
        let mic_active = self.has_device_type(DeviceType::Microphone);
        let screenshare_active = self.has_device_type(DeviceType::ScreenShare);
        let screenrecord_active = self.has_device_type(DeviceType::ScreenRecord);

        self.camera_state.update(camera_active);
        self.microphone_state.update(mic_active);
        self.screenshare_state.update(screenshare_active);
        self.screenrecord_state.update(screenrecord_active);
    }

    fn any_visible(&self) -> bool {
        self.camera_state.should_show()
            || self.microphone_state.should_show()
            || self.screenshare_state.should_show()
            || self.screenrecord_state.should_show()
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
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
        (
            Self {
                core,
                ..Default::default()
            },
            Task::none(),
        )
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let horizontal = self.core.applet.is_horizontal();
        let size = self.core.applet.suggested_size(true);
        let pad = self.core.applet.suggested_padding(true);

        // If nothing visible, return empty (hides applet)
        if !self.any_visible() {
            return "".into();
        }

        let mut icons: Vec<Element<Self::Message>> = vec![];

        // Helper to create an indicator with appropriate styling
        let make_indicator = |icon_name: &str, state: &IndicatorState| -> Element<Self::Message> {
            let is_active = state.active;

            // Icon color: accent/active hint when active, normal button color when in cooldown
            let icon_style: Rc<dyn Fn(&Theme) -> SvgStyle> = if is_active {
                Rc::new(|theme: &Theme| SvgStyle {
                    color: Some(theme.cosmic().accent.base.into()),
                })
            } else {
                Rc::new(|theme: &Theme| SvgStyle {
                    color: Some(theme.cosmic().button_color().into()),
                })
            };

            icon(icon::from_name(icon_name).into())
                .class(Svg::Custom(icon_style))
                .size(size.0)
                .into()
        };

        // Add indicators for each device type if they should be shown
        if self.camera_state.should_show() {
            icons.push(make_indicator("camera-web-symbolic", &self.camera_state));
        }
        if self.microphone_state.should_show() {
            icons.push(make_indicator("audio-input-microphone-symbolic", &self.microphone_state));
        }
        if self.screenshare_state.should_show() {
            icons.push(make_indicator("screen-shared-symbolic", &self.screenshare_state));
        }
        if self.screenrecord_state.should_show() {
            icons.push(make_indicator("media-record-symbolic", &self.screenrecord_state));
        }

        let container = if horizontal {
            Row::with_children(icons)
                .spacing(pad.0)
                .apply(layer_container)
        } else {
            Column::with_children(icons)
                .spacing(pad.1)
                .apply(layer_container)
        }
        .padding([pad.1, pad.0]);

        self.core.applet.autosize_window(container).into()
    }

    fn update(&mut self, message: Self::Message) -> app::Task<Self::Message> {
        match message {
            Message::Tick => {
                // Check for cameras using /proc (fallback for non-PipeWire camera access)
                self.proc_cameras = check_camera_proc();
                self.update_states();
            }
            Message::PipeWire(event) => match event {
                PipeWireEvent::DeviceAdded(usage) => {
                    tracing::debug!(
                        "Device added: {} using {}",
                        usage.app_name,
                        usage.device_name
                    );
                    self.pipewire_devices.insert(usage.node_id, usage);
                    self.update_states();
                }
                PipeWireEvent::DeviceRemoved(node_id) => {
                    if let Some(usage) = self.pipewire_devices.remove(&node_id) {
                        tracing::debug!(
                            "Device removed: {} was using {}",
                            usage.app_name,
                            usage.device_name
                        );
                    }
                    self.update_states();
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
            // Tick every 500ms to check cooldown timers and /proc cameras
            cosmic::iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick),
        ])
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}
