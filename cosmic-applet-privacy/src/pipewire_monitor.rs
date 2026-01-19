// Copyright 2025 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cosmic::iced::{Subscription, stream};
use glob::glob;
use pipewire::{context::ContextRc, main_loop::MainLoopRc, types::ObjectType};
use rustc_hash::{FxHashMap, FxHashSet};
use std::{cell::RefCell, rc::Rc, thread};

/// Type of privacy-sensitive device
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Camera,
    Microphone,
    ScreenShare,
    ScreenRecord,
}


/// Information about an application using a privacy-sensitive device
#[derive(Debug, Clone)]
pub struct DeviceUsage {
    pub device_type: DeviceType,
    pub device_name: String,
    pub app_name: String,
    #[allow(dead_code)] // Will be used for popup view
    pub app_id: Option<String>,
    #[allow(dead_code)] // Will be used for popup view
    pub process_id: Option<u32>,
    pub node_id: u32,
}

/// Events from PipeWire monitoring
#[derive(Debug, Clone)]
pub enum PipeWireEvent {
    /// A device started being used
    DeviceAdded(DeviceUsage),
    /// A device stopped being used
    DeviceRemoved(u32),
    /// PipeWire connection error
    #[allow(dead_code)] // Reserved for future error handling
    Error(String),
}

/// PipeWire media classes we monitor
const STREAM_INPUT_AUDIO: &str = "Stream/Input/Audio";
const STREAM_INPUT_VIDEO: &str = "Stream/Input/Video";
const VIDEO_SOURCE: &str = "Video/Source";

/// Known screen recording applications
const RECORDING_APPS: &[&str] = &[
    "obs",
    "obs-studio",
    "simplescreenrecorder",
    "kazam",
    "recordmydesktop",
    "peek",
    "gifcurry",
    "vokoscreen",
    "kooha",
];

fn is_recording_app(app_name: &str) -> bool {
    let lower = app_name.to_lowercase();
    RECORDING_APPS.iter().any(|&app| lower.contains(app))
}

/// Pending video stream info (waiting for link classification)
#[derive(Debug, Clone)]
struct PendingVideoStream {
    device_name: String,
    app_name: String,
    app_id: Option<String>,
    process_id: Option<u32>,
}

/// Subscribe to PipeWire events for privacy monitoring
pub fn pipewire_subscription() -> Subscription<PipeWireEvent> {
    struct PipeWireSubscription;

    Subscription::run_with_id(
        std::any::TypeId::of::<PipeWireSubscription>(),
        stream::channel(100, move |output| async move {
            // Spawn PipeWire monitoring thread - it sends directly to iced's channel
            thread::spawn(move || {
                if let Err(e) = run_pipewire_monitor(output) {
                    tracing::error!("PipeWire monitor error: {}", e);
                }
            });
            // Keep the async block alive - the thread handles everything
            futures::future::pending::<()>().await;
        }),
    )
}

fn run_pipewire_monitor(output: futures::channel::mpsc::Sender<PipeWireEvent>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing::debug!("Starting PipeWire monitor thread...");
    pipewire::init();

    let main_loop = MainLoopRc::new(None)?;
    let context = ContextRc::new(&main_loop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;

    // Shared state for tracking nodes and links
    // Camera source node IDs (Video/Source with media.role=Camera)
    let camera_sources: Rc<RefCell<FxHashSet<u32>>> = Rc::new(RefCell::new(FxHashSet::default()));
    // Pending video streams waiting for link-based classification
    let pending_streams: Rc<RefCell<FxHashMap<u32, PendingVideoStream>>> = Rc::new(RefCell::new(FxHashMap::default()));
    // Streams already classified as camera (to avoid duplicate events)
    let camera_streams: Rc<RefCell<FxHashSet<u32>>> = Rc::new(RefCell::new(FxHashSet::default()));
    // Track link IDs to their input node (for removal)
    let link_to_stream: Rc<RefCell<FxHashMap<u32, u32>>> = Rc::new(RefCell::new(FxHashMap::default()));

    // Clones for the listener closures
    let camera_sources_node = camera_sources.clone();
    let camera_sources_link = camera_sources.clone();
    let camera_sources_remove = camera_sources.clone();
    let pending_streams_node = pending_streams.clone();
    let pending_streams_link = pending_streams.clone();
    let pending_streams_remove = pending_streams.clone();
    let camera_streams_link = camera_streams.clone();
    let camera_streams_remove = camera_streams.clone();
    let link_to_stream_link = link_to_stream.clone();
    let link_to_stream_remove = link_to_stream.clone();

    let output_node = output.clone();
    let output_link = output.clone();
    let output_remove = output.clone();

    let _listener = registry
        .add_listener_local()
        .global(move |global| {
            match global.type_ {
                ObjectType::Node => {
                    let Some(props) = global.props else { return };
                    let Some(media_class) = props.get("media.class") else { return };
                    let media_role = props.get("media.role");

                    // Track camera source nodes
                    if media_class == VIDEO_SOURCE && media_role == Some("Camera") {
                        tracing::debug!("Tracking camera source node {}", global.id);
                        camera_sources_node.borrow_mut().insert(global.id);
                        return;
                    }

                    match media_class {
                        STREAM_INPUT_AUDIO => {
                            // Skip monitor sources (internal loopbacks)
                            if let Some(name) = props.get("node.name") {
                                if name.contains("monitor") {
                                    return;
                                }
                            }

                            let device_usage = DeviceUsage {
                                device_type: DeviceType::Microphone,
                                device_name: props
                                    .get("node.description")
                                    .or_else(|| props.get("node.name"))
                                    .unwrap_or("Unknown Device")
                                    .to_string(),
                                app_name: props
                                    .get("application.name")
                                    .or_else(|| props.get("application.process.binary"))
                                    .unwrap_or("Unknown")
                                    .to_string(),
                                app_id: props.get("application.id").map(String::from),
                                process_id: props.get("application.process.id").and_then(|s| s.parse().ok()),
                                node_id: global.id,
                            };

                            tracing::debug!("Microphone stream detected: {} (node {})", device_usage.app_name, global.id);
                            let mut sender = output_node.clone();
                            let _ = sender.try_send(PipeWireEvent::DeviceAdded(device_usage));
                        }
                        STREAM_INPUT_VIDEO => {
                            // Store as pending - will be classified when we see its link
                            let pending = PendingVideoStream {
                                device_name: props
                                    .get("node.description")
                                    .or_else(|| props.get("node.name"))
                                    .unwrap_or("Unknown Device")
                                    .to_string(),
                                app_name: props
                                    .get("application.name")
                                    .or_else(|| props.get("application.process.binary"))
                                    .or_else(|| props.get("node.name"))
                                    .unwrap_or("Unknown")
                                    .to_string(),
                                app_id: props.get("application.id").map(String::from),
                                process_id: props.get("application.process.id").and_then(|s| s.parse().ok()),
                            };

                            tracing::debug!("Video stream pending classification: {} (node {})", pending.app_name, global.id);
                            pending_streams_node.borrow_mut().insert(global.id, pending);
                        }
                        _ => {}
                    }
                }
                ObjectType::Link => {
                    let Some(props) = global.props else { return };

                    // Get link endpoints
                    let output_node_id = props.get("link.output.node").and_then(|s| s.parse::<u32>().ok());
                    let input_node_id = props.get("link.input.node").and_then(|s| s.parse::<u32>().ok());

                    let (Some(output_id), Some(input_id)) = (output_node_id, input_node_id) else {
                        return;
                    };

                    // Check if this link connects a camera source to a video stream
                    let is_camera_link = camera_sources_link.borrow().contains(&output_id);

                    if is_camera_link {
                        // Check if we have a pending stream for this input
                        if let Some(pending) = pending_streams_link.borrow_mut().remove(&input_id) {
                            // This stream is connected to a camera - emit Camera event
                            tracing::debug!(
                                "Link {} connects camera {} to stream {} - classifying as Camera",
                                global.id, output_id, input_id
                            );

                            camera_streams_link.borrow_mut().insert(input_id);
                            link_to_stream_link.borrow_mut().insert(global.id, input_id);

                            let device_usage = DeviceUsage {
                                device_type: DeviceType::Camera,
                                device_name: pending.device_name,
                                app_name: pending.app_name,
                                app_id: pending.app_id,
                                process_id: pending.process_id,
                                node_id: input_id,
                            };

                            let mut sender = output_link.clone();
                            let _ = sender.try_send(PipeWireEvent::DeviceAdded(device_usage));
                        } else if camera_streams_link.borrow().contains(&input_id) {
                            // Already classified as camera, just track the link
                            link_to_stream_link.borrow_mut().insert(global.id, input_id);
                        }
                    } else {
                        // Not a camera link - if we have a pending stream, classify it as screen share
                        if let Some(pending) = pending_streams_link.borrow_mut().remove(&input_id) {
                            tracing::debug!(
                                "Link {} connects non-camera {} to stream {} - classifying as ScreenShare",
                                global.id, output_id, input_id
                            );

                            let app_name = &pending.app_name;
                            let device_type = if is_recording_app(app_name) {
                                DeviceType::ScreenRecord
                            } else {
                                DeviceType::ScreenShare
                            };

                            let device_usage = DeviceUsage {
                                device_type,
                                device_name: pending.device_name,
                                app_name: pending.app_name,
                                app_id: pending.app_id,
                                process_id: pending.process_id,
                                node_id: input_id,
                            };

                            let mut sender = output_link.clone();
                            let _ = sender.try_send(PipeWireEvent::DeviceAdded(device_usage));
                        }
                    }
                }
                _ => {}
            }
        })
        .global_remove(move |id| {
            // Clean up tracking state
            camera_sources_remove.borrow_mut().remove(&id);
            pending_streams_remove.borrow_mut().remove(&id);
            camera_streams_remove.borrow_mut().remove(&id);

            // Check if this was a link we were tracking
            if let Some(stream_id) = link_to_stream_remove.borrow_mut().remove(&id) {
                // Link removed - the stream might still exist but is no longer connected
                // We'll let the node removal handle the DeviceRemoved event
                tracing::debug!("Link {} removed (was connected to stream {})", id, stream_id);
            }

            // Send removal event (will be ignored if not tracked by app)
            let mut sender = output_remove.clone();
            let _ = sender.try_send(PipeWireEvent::DeviceRemoved(id));
        })
        .register();

    main_loop.run();

    Ok(())
}

/// Check for camera usage by scanning /proc for open /dev/video* file descriptors.
/// This catches applications that bypass PipeWire (e.g., direct V4L2 access).
pub fn check_camera_proc() -> FxHashMap<u32, DeviceUsage> {
    let mut cameras = FxHashMap::default();

    let Ok(entries) = glob("/proc/[0-9]*/fd/[0-9]*") else {
        return cameras;
    };

    for entry in entries.filter_map(Result::ok) {
        if let Ok(link) = std::fs::read_link(&entry) {
            let link_str = link.to_string_lossy();
            if link_str.starts_with("/dev/video") {
                // Extract PID from path: /proc/PID/fd/FD
                if let Some(pid_str) = entry.to_str().and_then(|s| {
                    s.strip_prefix("/proc/")
                        .and_then(|s| s.split('/').next())
                }) {
                    if let Ok(pid) = pid_str.parse::<u32>() {
                        // Get process name
                        let app_name = std::fs::read_to_string(format!("/proc/{}/comm", pid))
                            .map(|s| s.trim().to_string())
                            .unwrap_or_else(|_| "Unknown".to_string());

                        // Skip PipeWire daemon and related processes - they access
                        // /dev/video* on behalf of other apps, not directly
                        if app_name == "pipewire" || app_name == "wireplumber" {
                            continue;
                        }

                        // Use PID as a unique identifier for this camera usage
                        let node_id = pid;

                        cameras.insert(
                            node_id,
                            DeviceUsage {
                                device_type: DeviceType::Camera,
                                device_name: link_str.to_string(),
                                app_name,
                                app_id: None,
                                process_id: Some(pid),
                                node_id,
                            },
                        );
                    }
                }
            }
        }
    }

    cameras
}
