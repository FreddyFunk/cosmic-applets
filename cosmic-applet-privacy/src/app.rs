// Copyright 2025 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use crate::fl;
use crate::pipewire_monitor::{DeviceType, DeviceUsage, PipeWireEvent, check_camera_proc, pipewire_subscription};
use cosmic::iced::{Alignment, Length, Subscription, window};
use cosmic::iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::widget::{column, row};
use cosmic::widget::{divider, horizontal_space, icon, text};
use cosmic::{app, Application, Element, Task, theme};
use rustc_hash::FxHashMap;
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
    /// Popup window ID
    popup: Option<window::Id>,
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

    fn get_devices_by_type(&self, device_type: DeviceType) -> Vec<&DeviceUsage> {
        self.pipewire_devices
            .values()
            .chain(self.proc_cameras.values())
            .filter(|d| d.device_type == device_type)
            .collect()
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
    TogglePopup,
    CloseRequested(window::Id),
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
        // TEMPORARY TEST: Always show a static icon like bluetooth does
        // to verify if the background issue is in our view code or elsewhere
        self.core
            .applet
            .icon_button("camera-web-symbolic")
            .on_press_down(Message::TogglePopup)
            .into()
    }

    fn view_window(&self, _id: window::Id) -> Element<'_, Self::Message> {
        let spacing = theme::active().cosmic().spacing;

        let mut content: Vec<Element<Self::Message>> = vec![];

        // Camera section
        let camera_devices = self.get_devices_by_type(DeviceType::Camera);
        if !camera_devices.is_empty() {
            content.push(self.device_section(
                "camera-web-symbolic",
                fl!("camera"),
                &camera_devices,
            ));
        }

        // Microphone section
        let mic_devices = self.get_devices_by_type(DeviceType::Microphone);
        if !mic_devices.is_empty() {
            if !content.is_empty() {
                content.push(divider::horizontal::default().into());
            }
            content.push(self.device_section(
                "audio-input-microphone-symbolic",
                fl!("microphone"),
                &mic_devices,
            ));
        }

        // Screen share section
        let screenshare_devices = self.get_devices_by_type(DeviceType::ScreenShare);
        if !screenshare_devices.is_empty() {
            if !content.is_empty() {
                content.push(divider::horizontal::default().into());
            }
            content.push(self.device_section(
                "screen-shared-symbolic",
                fl!("screen-share"),
                &screenshare_devices,
            ));
        }

        // Screen record section
        let screenrecord_devices = self.get_devices_by_type(DeviceType::ScreenRecord);
        if !screenrecord_devices.is_empty() {
            if !content.is_empty() {
                content.push(divider::horizontal::default().into());
            }
            content.push(self.device_section(
                "media-record-symbolic",
                fl!("screen-record"),
                &screenrecord_devices,
            ));
        }

        // If no active devices, show a message
        if content.is_empty() {
            let msg = fl!("no-active-devices");
            content.push(text::body(msg).into());
        }

        self.core
            .applet
            .popup_container(
                column(content)
                    .spacing(spacing.space_xxs)
                    .padding([spacing.space_xs, spacing.space_none])
            )
            .into()
    }

    fn update(&mut self, message: Self::Message) -> app::Task<Self::Message> {
        match message {
            Message::Tick => {
                // Check for cameras using /proc (fallback for non-PipeWire camera access)
                self.proc_cameras = check_camera_proc();
                self.update_states();
            }
            Message::TogglePopup => {
                if let Some(p) = self.popup.take() {
                    return destroy_popup(p);
                }

                let new_id = window::Id::unique();
                self.popup.replace(new_id);

                let popup_settings = self.core.applet.get_popup_settings(
                    self.core.main_window_id().unwrap(),
                    new_id,
                    None,
                    None,
                    None,
                );

                return get_popup(popup_settings);
            }
            Message::CloseRequested(id) => {
                if Some(id) == self.popup {
                    self.popup = None;
                }
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

    fn on_close_requested(&self, id: window::Id) -> Option<Self::Message> {
        Some(Message::CloseRequested(id))
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}

impl PrivacyIndicator {
    fn device_section<'a>(
        &'a self,
        icon_name: &'a str,
        title: String,
        devices: &[&'a DeviceUsage],
    ) -> Element<'a, Message> {
        let spacing = theme::active().cosmic().spacing;

        let mut items: Vec<Element<Message>> = vec![
            row![
                icon::from_name(icon_name).size(20).symbolic(true),
                text::body(title),
            ]
            .spacing(spacing.space_xs)
            .align_y(Alignment::Center)
            .into(),
        ];

        for device in devices {
            items.push(
                row![
                    horizontal_space().width(Length::Fixed(28.0)),
                    column![
                        text::body(&device.app_name),
                        text::caption(&device.device_name),
                    ]
                ]
                .into(),
            );
        }

        column(items)
            .spacing(spacing.space_xxs)
            .padding([spacing.space_xxs, spacing.space_s])
            .into()
    }
}
