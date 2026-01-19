// Copyright 2025 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

mod app;
mod localize;
mod pipewire_monitor;

use localize::localize;

pub fn run() -> cosmic::iced::Result {
    localize();
    app::run()
}
