use anyhow::{bail, Context, Result};
use evdev::Device;
use log::{error, info};
use std::collections::HashMap;
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub(crate) struct MouseDeviceInfo {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    // pub(crate) phys: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct MouseEvent {
    pub(crate) device_key: String,
    pub(crate) control_type: crate::common::ControlType,
    pub(crate) value: i32,
}

pub(crate) struct MouseDevice {
    //path: PathBuf,
    device_name: String,
    device: evdev::EventStream,
    sender: mpsc::UnboundedSender<MouseEvent>,
    debug: bool,
}

impl MouseDevice {
    pub(crate) fn new(
        path: PathBuf,
        device_name: String,
        sender: mpsc::UnboundedSender<MouseEvent>,
        debug: bool,
    ) -> Result<Self> {
        let /*mut*/ device = Device::open(&path)
            .context(format!("Failed to open device: {}", path.display()))?
            .into_event_stream()?;
        // device.device_mut().grab().context("can't grab the device {device_name}!")?;
        Ok(Self {
            //path,
            device_name,
            device,
            sender,
            debug,
        })
    }

    pub(crate) fn run(&mut self, engine_stop_token: CancellationToken) {
        let mut sleep_millis = 0;
        loop {
            if engine_stop_token.is_cancelled() {
                info!(
                    "Stopping run thread for mouse {:?} {}",
                    self.device.device().physical_path(),
                    self.device_name
                );
                return;
            }

            let result = {
                // Using non-blocking mode, so manually handling WouldBlock case.
                match self.device.device_mut().fetch_events() {
                    Ok(events) => {
                        let mut mouse_events = Vec::new();
                        for event in events {
                            if let Some(mouse_event) = Self::evdev_mouse_event_to_internal(
                                &self.device_name,
                                event,
                                self.debug,
                            ) {
                                mouse_events.push(mouse_event);
                            }
                        }
                        Some(Ok(mouse_events))
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => None,
                    Err(e) => Some(Err(e)),
                }
            };

            match result {
                None => {
                    if sleep_millis < 5 {
                        sleep_millis += 1;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(sleep_millis));
                }
                Some(anyhow::Result::Ok(mouse_events)) => {
                    sleep_millis = 0;
                    for mouse_event in mouse_events {
                        let _ = self.sender.send(mouse_event);
                    }
                }
                Some(Err(e)) => {
                    if self.debug {
                        error!("Error reading mouse events: {}", e);
                    }
                    break;
                }
            }
        }
    }

    fn evdev_mouse_event_to_internal(
        device_name: &str,
        event: evdev::InputEvent,
        debug: bool,
    ) -> Option<MouseEvent> {
        let control_type = crate::common::ControlType::from(event);
        if control_type.is_unhandled() {
            if debug && event.event_type() != evdev::EventType::SYNCHRONIZATION {
                log::debug!("Event not associated with any control: {:?}", event);
            }
            None
        } else {
            MouseEvent {
                device_key: device_name.into(),
                control_type,
                value: event.value(),
            }
            .into()
        }
    }
}

pub(crate) struct MouseManager {
    debug: bool,
    devices: HashMap<String, task::JoinHandle<()>>,
    sender: mpsc::UnboundedSender<MouseEvent>,
    receiver: Arc<AsyncMutex<mpsc::UnboundedReceiver<MouseEvent>>>,
    engine_stop_token: CancellationToken,
}

impl MouseManager {
    pub(crate) fn new(debug: bool) -> Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();

        Ok(Self {
            debug,
            devices: HashMap::new(),
            sender: tx,
            receiver: Arc::new(AsyncMutex::new(rx)),
            engine_stop_token: CancellationToken::new(),
        })
    }

    pub(crate) fn enumerate_devices(&self) -> Result<Vec<MouseDeviceInfo>> {
        let mut devices = Vec::new();

        // TODO: use evdev device enumeration.
        // Scan /dev/input/event* devices
        for entry in std::fs::read_dir("/dev/input")? {
            let entry = entry?;
            let path = entry.path();

            if let Some(name) = path.file_name() {
                if name.to_string_lossy().starts_with("event") {
                    if let std::result::Result::Ok(device) = Device::open(&path) {
                        // Check if device has relative axes (mouse/trackpad characteristic)
                        if device.supported_relative_axes().is_some() {
                            devices.push(MouseDeviceInfo {
                                name: device.name().unwrap_or("Unknown").to_string(),
                                path: path.clone(),
                                //phys: device.physical_path().map(|s| s.to_string()),
                            });
                        }
                    }
                }
            }
        }

        Ok(devices)
    }

    pub(crate) fn match_device(
        &self,
        pattern: &regex::Regex,
        devices: &[MouseDeviceInfo],
    ) -> Vec<MouseDeviceInfo> {
        let mut matched = Vec::new();
        for device in devices {
            if pattern.is_match(&device.name) {
                matched.push(device.clone());
            }
        }
        matched
    }

    pub(crate) fn open_device(
        &mut self,
        device_info: &MouseDeviceInfo,
        key_name: &str,
    ) -> Result<()> {
        let sender = self.sender.clone();
        let path = device_info.path.clone();
        let device_name = key_name.to_string();
        let debug = self.debug;
        let stop_token = self.engine_stop_token.child_token();

        let handle = task::spawn_blocking(move || {
            if let std::result::Result::Ok(mut device) =
                MouseDevice::new(path, device_name, sender, debug)
            {
                device.run(stop_token);
            }
        });

        self.devices.insert(key_name.to_string(), handle);

        if self.debug {
            info!("Opened mouse device: {} as {}", device_info.name, key_name);
        }

        Ok(())
    }

    pub(crate) async fn get_event(&self) -> Option<MouseEvent> {
        self.receiver.lock().await.recv().await
    }

    pub(crate) async fn monitor(&mut self, name_regex: &regex::Regex) -> Result<()> {
        let devices = self.enumerate_devices()?;
        let matched = self.match_device(name_regex, &devices);

        if matched.is_empty() {
            bail!("No devices found matching '{}'", name_regex);
        }

        println!("Monitoring mouse devices:");
        for device in &matched {
            println!("  - {} @ {}", device.name, device.path.display());
            self.open_device(device, &device.name)?;
        }

        println!("Press Ctrl+C to stop monitoring...");

        while let Some(event) = self.get_event().await {
            log::info!(
                "[{}] {} = {}",
                event.device_key,
                event.control_type,
                event.value
            );
        }

        Ok(())
    }

    pub(crate) fn stop(&mut self) -> Result<()> {
        self.engine_stop_token.cancel();

        for handle in self
            .devices
            .drain()
            .map(|(_, handle)| handle)
            .collect::<Vec<_>>()
        {
            // NB: for a blocking parallel task (our case) it's a noop if it is running.
            //     but can prevent the task from running if it's not already started.
            handle.abort();
        }
        Ok(())
    }
}
