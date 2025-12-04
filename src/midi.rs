use anyhow::{bail, Context, Result};
use log::{debug, info, warn};
use midir::{MidiInput, MidiInputConnection};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config;

#[derive(Debug, Clone)]
pub(crate) struct MidiDeviceInfo {
    pub(crate) name: String,
    pub(crate) port_index: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct MidiMessage {
    pub(crate) device_name: String,
    pub(crate) message_type: MidiMessageType,
    pub(crate) channel: u8,
    pub(crate) note: Option<u8>,
    pub(crate) velocity: Option<u8>,
    pub(crate) control: Option<u8>,
    pub(crate) value: Option<u8>,
    pub(crate) pitch: Option<i16>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MidiMessageType {
    NoteOn,
    NoteOff,
    ControlChange,
    PitchWheel,
    Aftertouch,
    PolyAftertouch,
    ProgramChange,
}

pub(crate) struct MidiManager {
    debug: bool,
    midi_input: MidiInput,
    connections: Arc<Mutex<HashMap<String, MidiInputConnection<()>>>>,
    message_sender: mpsc::UnboundedSender<MidiMessage>,
    _engine_stop_token: CancellationToken,
    message_receiver: mpsc::UnboundedReceiver<MidiMessage>,
    note_states: Arc<Mutex<HashMap<String, HashSet<u8>>>>,
}

impl MidiManager {
    pub(crate) fn new(debug: bool) -> Result<Self> {
        let midi_input = MidiInput::new(format!("{} MIDI Input", config::APP_NAME).as_str())
            .context("Failed to create MIDI input")?;

        let (tx, rx) = mpsc::unbounded_channel();

        Ok(Self {
            debug,
            midi_input,
            connections: Arc::new(Mutex::new(HashMap::new())),
            _engine_stop_token: CancellationToken::new(),
            message_sender: tx,
            message_receiver: rx,
            note_states: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub(crate) fn enumerate_devices(&self) -> Vec<MidiDeviceInfo> {
        let ports = self.midi_input.ports();
        let mut devices = Vec::new();
        for (i, port) in ports.iter().enumerate() {
            match self.midi_input.port_name(port) {
                Ok(name) => {
                    devices.push(MidiDeviceInfo {
                        name,
                        port_index: i,
                    });
                }
                Err(error) => {
                    log::warn!("Can't open MIDI device port: {error}.");
                }
            }
        }
        devices
    }

    pub(crate) fn match_device(
        &self,
        device_name_regex: &regex::Regex,
        devices: &[MidiDeviceInfo],
    ) -> Vec<String> {
        let mut matched = Vec::new();
        for device in devices {
            if device_name_regex.is_match(&device.name) {
                matched.push(device.name.clone());
            }
        }
        matched
    }

    pub(crate) fn open_device(&mut self, device_name: &str) -> Result<()> {
        let devices = self.enumerate_devices();
        if let Some(device) = devices.iter().find(|d| d.name == device_name) {
            let sender = self.message_sender.clone();
            let note_states = self.note_states.clone();
            let debug = self.debug;
            let device_name_clone = device.name.clone();
            let midi_in = MidiInput::new(&format!("{} {}", config::APP_NAME, device_name))?;
            let ports = midi_in.ports();
            if device.port_index >= ports.len() {
                bail!("Invalid port index");
            }
            let port = ports[device.port_index].clone();
            let device_name_for_closure = device_name_clone.clone();
            let connection = midi_in
                .connect(
                    &port,
                    &device_name_clone,
                    move |_stamp, message, _| {
                        if let Some(msg) =
                            Self::parse_midi_message(message, &device_name_for_closure, debug)
                        {
                            // Update note states
                            if msg.message_type == MidiMessageType::NoteOn {
                                if let Some(note) = msg.note {
                                    let mut states = note_states.lock().unwrap();
                                    states
                                        .entry(device_name_for_closure.clone())
                                        .or_default()
                                        .insert(note);
                                }
                            } else if msg.message_type == MidiMessageType::NoteOff {
                                if let Some(note) = msg.note {
                                    let mut states = note_states.lock().unwrap();
                                    if let Some(notes) = states.get_mut(&device_name_for_closure) {
                                        notes.remove(&note);
                                    }
                                }
                            }

                            if debug {
                                debug!("MIDI: {:?}", msg);
                            }

                            let _ = sender.send(msg);
                        }
                    },
                    (),
                )
                .map_err(|e| anyhow::anyhow!("Failed to connect to MIDI device: {}", e))?;

            self.connections
                .lock()
                .unwrap()
                .insert(device_name.to_string(), connection);

            info!("Opened MIDI device: {}", device_name);

            Ok(())
        } else {
            bail!("MIDI device not found: {}", device_name)
        }
    }

    fn parse_midi_message(data: &[u8], device_name: &str, debug: bool) -> Option<MidiMessage> {
        let (status, data1, data2) = match data {
            [s] => (s, None, None),
            [s, d1] => (s, Some(*d1), None),
            [s, d1, d2, ..] => (s, Some(*d1), Some(*d2)),
            _ => {
                if debug {
                    log::warn!(
                        "Midi messages of more than 3 bytes in size {data:#?} are unexpected."
                    );
                }
                return None;
            }
        };

        let channel = status & 0x0F;
        let message_type_code = status & 0xF0;

        let mut message = MidiMessage {
            device_name: device_name.to_string(),
            message_type: MidiMessageType::ProgramChange, /* overriden below */
            channel,
            note: None,
            velocity: None,
            control: None,
            value: None,
            pitch: None,
        };

        match (message_type_code, data1, data2) {
            (0x80, Some(note), Some(velocity)) => {
                message.message_type = MidiMessageType::NoteOff;
                message.note = Some(note);
                message.velocity = Some(velocity);
            }
            (0x90, Some(note), Some(velocity)) => {
                message.message_type = if velocity == 0 {
                    MidiMessageType::NoteOff
                } else {
                    MidiMessageType::NoteOn
                };
                message.note = Some(note);
                message.velocity = Some(velocity);
            }
            (0x0B0, Some(control), Some(value)) => {
                message.message_type = MidiMessageType::ControlChange;
                message.control = Some(control);
                message.value = Some(value);
            }
            (0x0E0, Some(lsb), Some(msb)) => {
                message.message_type = MidiMessageType::PitchWheel;
                let value = ((msb as i16) << 7) | (lsb as i16);
                message.pitch = Some(value - 8192); // Centered at 0
            }
            (0x0A0, Some(note), Some(pressure)) => {
                message.message_type = MidiMessageType::PolyAftertouch;
                message.note = Some(note);
                message.value = Some(pressure);
            }
            (0x0D0, Some(value), None | Some(_)) => {
                message.message_type = MidiMessageType::Aftertouch;
                message.value = Some(value);
            }
            (0x0C0, Some(value), None | Some(_)) => {
                message.message_type = MidiMessageType::ProgramChange;
                message.value = Some(value);
            }
            _ => {
                if debug {
                    log::debug!("Unhandeled MIDI message of type {message_type_code:#?}");
                }
                return None;
            }
        }

        Some(message)
    }

    pub(crate) async fn get_message(&mut self) -> Option<MidiMessage> {
        self.message_receiver.recv().await
    }

    pub(crate) fn midi_type_matches(
        &self,
        msg_type: &MidiMessageType,
        spec_type: &crate::schemas::MidiMessageType,
    ) -> bool {
        use crate::schemas::MidiMessageType as SpecType;
        matches!(
            (msg_type, spec_type),
            (MidiMessageType::PitchWheel, SpecType::PitchWheel)
                | (MidiMessageType::ControlChange, SpecType::ControlChange)
                | (MidiMessageType::NoteOn, SpecType::Note)
                | (MidiMessageType::NoteOn, SpecType::NoteOn)
                | (MidiMessageType::NoteOff, SpecType::Note)
                | (MidiMessageType::NoteOff, SpecType::NoteOff)
                | (MidiMessageType::Aftertouch, SpecType::Aftertouch)
                // TODO: | (MidiMessageType::PolyAftertouch, ... )
                | (MidiMessageType::ProgramChange, SpecType::ProgramChange)
        )
    }

    pub(crate) fn midi_message_matches_spec(
        &self,
        msg: &MidiMessage,
        mapping: &config::ResolvedMapping,
    ) -> bool {
        if let config::ControlReference::Midi(midi_control) = &mapping.source.control {
            if let Some(spec) = &midi_control.midi_message {
                return self.check_midi_spec(msg, spec);
            }
        }
        false
    }

    fn check_midi_spec(&self, msg: &MidiMessage, spec: &crate::schemas::MidiMessage) -> bool {
        if !self.midi_type_matches(&msg.message_type, &spec.msg_type) {
            return false;
        }
        if !self.midi_channel_matches(msg.channel, &spec.channel) {
            return false;
        }
        if let Some(number_spec) = &spec.number {
            if !self.midi_number_matches(msg, number_spec) {
                return false;
            }
        }
        true
    }

    fn midi_channel_matches(
        &self,
        msg_channel: u8,
        spec_channel: &crate::schemas::MidiChannel,
    ) -> bool {
        use crate::schemas::MidiChannel;
        match spec_channel {
            MidiChannel::Any => true,
            MidiChannel::Number(n) => msg_channel == *n,
        }
    }

    fn midi_number_matches(
        &self,
        msg: &MidiMessage,
        spec_number: &crate::schemas::MidiNumber,
    ) -> bool {
        use crate::schemas::MidiNumber;
        let actual_number = match msg.message_type {
            MidiMessageType::NoteOn | MidiMessageType::NoteOff => msg.note,
            MidiMessageType::ControlChange => msg.control,
            MidiMessageType::ProgramChange => msg.value,
            _ => None,
        };

        if let Some(number) = actual_number {
            match spec_number {
                MidiNumber::Single(n) => number == *n,
                MidiNumber::Multiple(numbers) => numbers.contains(&number),
                MidiNumber::Special(s) => s.to_lowercase() == "any",
            }
        } else {
            false
        }
    }

    pub(crate) fn extract_midi_value(&self, msg: &MidiMessage) -> i32 {
        match msg.message_type {
            MidiMessageType::PitchWheel => msg.pitch.unwrap_or(0) as i32,
            MidiMessageType::ControlChange => msg.value.unwrap_or(0) as i32,
            MidiMessageType::NoteOn => msg.velocity.unwrap_or(0) as i32,
            MidiMessageType::NoteOff => 0,
            MidiMessageType::Aftertouch => msg.value.unwrap_or(0) as i32,
            // TODO: poly aftertouch should be applied to modify velocity of
            // already pressed associated note.
            MidiMessageType::PolyAftertouch => msg.value.unwrap_or(0) as i32,
            MidiMessageType::ProgramChange => msg.value.unwrap_or(0) as i32,
        }
    }

    // pub(crate) fn get_special_note_value(&self, device_name: &str, note_type: &str) -> u8 {
    //     let states = self.note_states.lock().unwrap();

    //     if let Some(notes) = states.get(device_name) {
    //         if notes.is_empty() {
    //             return 0;
    //         }

    //         match note_type {
    //             "lowest" => notes.iter().min().copied().unwrap_or(0),
    //             "highest" => notes.iter().max().copied().unwrap_or(0),
    //             "any" => {
    //                 if notes.is_empty() {
    //                     0
    //                 } else {
    //                     127
    //                 }
    //             }
    //             _ => 0,
    //         }
    //     } else {
    //         0
    //     }
    // }

    pub(crate) async fn monitor(&mut self, name_regex: &regex::Regex) -> Result<()> {
        let devices = self.enumerate_devices();
        let matched = self.match_device(name_regex, &devices);

        if matched.is_empty() {
            bail!("No devices found matching '{}'", name_regex);
        }

        info!("Monitoring devices: {:?}", matched);
        info!("Press Ctrl+C to stop monitoring...");
        info!("{}", "=".repeat(60));

        for device_name in &matched {
            self.open_device(device_name)?;
        }

        while let Some(msg) = self.get_message().await {
            Self::print_message(&msg);
        }

        Ok(())
    }

    fn print_message(msg: &MidiMessage) {
        let timestamp = chrono::Local::now().format("%H:%M:%S%.3f");

        match msg.message_type {
            MidiMessageType::NoteOn | MidiMessageType::NoteOff => {
                let note_names = [
                    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
                ];
                if let Some(note) = msg.note {
                    let note_name = note_names[(note % 12) as usize];
                    let octave = (note / 12) as i8 - 1;
                    let on_off = if msg.message_type == MidiMessageType::NoteOn {
                        "ON"
                    } else {
                        "OFF"
                    };

                    info!(
                        "[{}][{}] Note {}: {}{} (note={}, vel={}, ch={})",
                        timestamp,
                        msg.device_name,
                        on_off,
                        note_name,
                        octave,
                        note,
                        msg.velocity.unwrap_or(0),
                        msg.channel
                    );
                }
            }
            MidiMessageType::ControlChange => {
                if let (Some(control), Some(value)) = (msg.control, msg.value) {
                    let cc_name = Self::get_cc_name(control);
                    info!(
                        "[{}][{}] CC: {} (cc={}, val={}, ch={})",
                        timestamp, msg.device_name, cc_name, control, value, msg.channel
                    );
                }
            }
            MidiMessageType::PitchWheel => {
                if let Some(pitch) = msg.pitch {
                    info!(
                        "[{}][{}] Pitch Wheel: {} (ch={})",
                        timestamp, msg.device_name, pitch, msg.channel
                    );
                }
            }
            MidiMessageType::Aftertouch => {
                if let Some(value) = msg.value {
                    info!(
                        "[{}][{}] Aftertouch: {} (ch={})",
                        timestamp, msg.device_name, value, msg.channel
                    );
                }
            }
            MidiMessageType::PolyAftertouch => {
                if let Some(value) = msg.value {
                    info!(
                        "[{}][{}] PolyAftertouch: {} (note={} ch={})",
                        timestamp,
                        msg.device_name,
                        value,
                        msg.note.unwrap_or(u8::MAX),
                        msg.channel
                    );
                }
            }
            MidiMessageType::ProgramChange => {
                if let Some(value) = msg.value {
                    info!(
                        "[{}][{}] Program Change: {} (ch={})",
                        timestamp, msg.device_name, value, msg.channel
                    );
                }
            }
        }
    }

    fn get_cc_name(cc: u8) -> String {
        let name = match cc {
            1 => "Modulation",
            2 => "Breath Controller",
            7 => "Volume",
            10 => "Pan",
            11 => "Expression",
            64 => "Sustain",
            65 => "Portamento",
            66 => "Sostenuto",
            67 => "Soft Pedal",
            71 => "Filter Resonance",
            72 => "Release Time",
            73 => "Attack Time",
            74 => "Filter Cutoff",
            75 => "Decay Time",
            _ => return format!("CC{}", cc),
        };
        name.to_string()
    }

    pub(crate) fn stop(&mut self) -> Result<()> {
        // Drop all MIDI connections to stop background MIDI threads
        let mut connections = self.connections.lock().unwrap();
        connections.clear();
        Ok(())
    }
}

// MIDI Learn Mode
pub(crate) struct MidiLearnMode {
    midi_manager: MidiManager,
    // config_manager: ConfigManager,
    learned_controls: HashMap<String, HashMap<String, serde_yaml::Value>>,
    start_time: std::time::Instant,
}

impl MidiLearnMode {
    pub(crate) fn new(midi_manager: MidiManager /*, config_manager: ConfigManager */) -> Self {
        Self {
            midi_manager,
            // config_manager,
            learned_controls: HashMap::new(),
            start_time: std::time::Instant::now(),
        }
    }

    pub(crate) async fn run(&mut self) -> Result<()> {
        info!("\n{}", "=".repeat(60));
        info!("{} MIDI learn mode.", config::APP_NAME);
        info!("{}", "=".repeat(60));
        info!("This mode will automatically discover and learn MIDI controls.");
        info!("Press different controls on your MIDI devices to learn them.");
        info!("\nInstructions:");
        info!("1. Press keys, turn knobs, move sliders, and use pedals");
        info!("2. Each control will be automatically detected and configured");
        info!("3. Press Ctrl+C when finished to save the configuration (TODO!)");
        // TODO: configuration saving is not implemented.
        info!("\nConfiguration will be saved to: mmvj_autolearn.yaml");
        info!("{}", "=".repeat(60));
        info!("");

        let devices = self.midi_manager.enumerate_devices();

        if devices.is_empty() {
            warn!("No MIDI devices found!");
            return Ok(());
        }

        info!("Monitoring {} MIDI device(s):", devices.len());
        for (i, device) in devices.iter().enumerate() {
            info!("  {}. {}", i + 1, device.name);
            self.midi_manager.open_device(&device.name)?;
        }
        info!("");

        while let Some(msg) = self.midi_manager.get_message().await {
            self.process_learn_message(&msg);
        }

        Ok(())
    }

    // TODO: create YAML configuration and later save it.
    fn process_learn_message(&mut self, msg: &MidiMessage) {
        let device_controls = self
            .learned_controls
            .entry(msg.device_name.clone())
            .or_default();

        let control_key = match msg.message_type {
            MidiMessageType::NoteOn => {
                if let Some(note) = msg.note {
                    format!("note_{}", note)
                } else {
                    return;
                }
            }
            MidiMessageType::ControlChange => {
                if let Some(control) = msg.control {
                    format!("cc_{}", control)
                } else {
                    return;
                }
            }
            MidiMessageType::PitchWheel => "pitch_wheel".to_string(),
            MidiMessageType::Aftertouch => "aftertouch".to_string(),
            MidiMessageType::PolyAftertouch => "polyaftertouch".to_string(),
            _ => return,
        };

        if let std::collections::hash_map::Entry::Vacant(e) =
            device_controls.entry(control_key.clone())
        {
            let elapsed = self.start_time.elapsed().as_secs_f32();
            info!(
                "[{:6.1}s] Learned: [{}] {}",
                elapsed, msg.device_name, control_key
            );

            e.insert(serde_yaml::Value::Null);
        }
    }
}
