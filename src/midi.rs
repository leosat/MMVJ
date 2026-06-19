use crate::common::{BaseNumT, get_interned_str, intern_str};
use crate::config;
use crate::config::DebugLevel;
use crate::mapped_controls::MappedCtlsMidi;
use crate::num_interval::NumInterval;
use crate::schemas_common::ObjId;
use crate::schemas_midi::{MidiChannelCfg, MidiControlMatcherCfg, MidiNumberCfg, MidiNumberSpecial};
use crate::schemas_midi::{MidiControlCode, MidiMessageType};
use anyhow::{Context, Result, bail};
use log::{debug, info, warn};
use midir::{MidiInput, MidiInputConnection};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub(crate) struct AvailableMidiDeviceInfo {
    pub(crate) name: String,
    pub(crate) port_index: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct MappedMidiMessage {
    // pub(crate) device_name_todo_newapi_remove: String,
    pub(crate) device_id: ObjId,
    pub(crate) message_type: MidiMessageType,
    pub(crate) channel: u8,
    pub(crate) note: Option<u8>,
    pub(crate) velocity: Option<u8>,
    pub(crate) control: Option<u8>,
    pub(crate) value: Option<u8>,
    pub(crate) pitch: Option<i16>,
}

impl MappedMidiMessage {
    pub(crate) fn from_raw_data(data: &[u8], device_id: ObjId, debug: DebugLevel) -> Option<Self> {
        let status = *data.first()?;
        let data1 = data.get(1).copied();
        let data2 = data.get(2).copied();
        if data.len() > 3 {
            log::warn!("Midi messages of more than 3 bytes in size {data:#?} are unexpected.");
        }

        let channel = status & 0x0F;
        let type_code = status & 0xF0;

        let mut m = Self {
            device_id: device_id,
            message_type: MidiMessageType::ProgramChange, /* overriden below */
            channel,
            note: None,
            velocity: None,
            control: None,
            value: None,
            pitch: None,
        };

        match (type_code, data1, data2) {
            (0x80, Some(note), Some(velocity)) => {
                m.message_type = MidiMessageType::NoteOff;
                m.note = Some(note);
                m.velocity = Some(velocity);
            }
            (0x90, Some(note), Some(velocity)) => {
                m.message_type = if velocity == 0 {
                    MidiMessageType::NoteOff
                } else {
                    MidiMessageType::NoteOn
                };
                m.note = Some(note);
                m.velocity = Some(velocity);
            }
            (0x0B0, Some(control), Some(value)) => {
                m.message_type = MidiMessageType::ControlChange;
                m.control = Some(control);
                m.value = Some(value);
            }
            (0x0E0, Some(lsb), Some(msb)) => {
                m.message_type = MidiMessageType::PitchWheel;
                let value = ((msb as i16) << 7) | (lsb as i16);
                m.pitch = Some(value - 8192); // Centered at 0
            }
            (0x0A0, Some(note), Some(pressure)) => {
                m.message_type = MidiMessageType::PolyAftertouch;
                m.note = Some(note);
                m.value = Some(pressure);
            }
            (0x0D0, Some(value), None | Some(_)) => {
                m.message_type = MidiMessageType::Aftertouch;
                m.value = Some(value);
            }
            (0x0C0, Some(value), None | Some(_)) => {
                m.message_type = MidiMessageType::ProgramChange;
                m.value = Some(value);
            }
            _ => {
                if debug.is_on() {
                    log::debug!("Unhandeled MIDI message of type {type_code:#?}");
                }
                return None;
            }
        }

        Some(m)
    }

    pub(crate) fn matches_control_matcher(&self, control_matcher: &MidiControlMatcherCfg) -> bool {
        control_matcher.midi_message.r#type == self.message_type.clone().into()
            && match control_matcher.midi_message.channel {
                MidiChannelCfg::Any => true,
                MidiChannelCfg::Number(n) => self.channel == n,
            }
            && (control_matcher.midi_message.r#type == MappedCtlsMidi::PitchWheel || {
                if let Some(number) = match self.message_type {
                    MidiMessageType::NoteOn | MidiMessageType::NoteOff => self.note,
                    MidiMessageType::ControlChange => self.control,
                    MidiMessageType::ProgramChange => self.value,
                    _ => None,
                } {
                    match &control_matcher.midi_message.number {
                        MidiNumberCfg::Single(control_matcher_number) => number == *control_matcher_number,
                        MidiNumberCfg::Multiple(control_matcher_numbers) => control_matcher_numbers.contains(&number),
                        MidiNumberCfg::Special(control_matcher_special) => {
                            *control_matcher_special == MidiNumberSpecial::Any
                        }
                    }
                } else {
                    false
                }
            })
    }

    pub(crate) fn get_value(&self) -> BaseNumT {
        match self.message_type {
            MidiMessageType::PitchWheel => self.pitch.unwrap_or(0) as BaseNumT,
            MidiMessageType::ControlChange => self.value.unwrap_or(0) as BaseNumT,
            MidiMessageType::NoteOn => self.velocity.unwrap_or(0) as BaseNumT,
            MidiMessageType::NoteOff => 0.0,
            MidiMessageType::Aftertouch => self.value.unwrap_or(0) as BaseNumT,
            MidiMessageType::PolyAftertouch => self.value.unwrap_or(0) as BaseNumT,
            MidiMessageType::ProgramChange => self.value.unwrap_or(0) as BaseNumT,
        }
    }

    pub(crate) fn get_cc_name(cc: u8) -> String {
        if let Ok(cc) = MidiControlCode::try_from(cc) {
            cc.to_string()
        } else {
            format!("CC {}", cc)
        }
    }

    pub(crate) fn pretty_print(&self, device_name: Option<&str>) {
        let timestamp = chrono::Local::now().format("%H:%M:%S%.3f");

        match self.message_type {
            MidiMessageType::NoteOn | MidiMessageType::NoteOff => {
                let note_names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
                if let Some(note) = self.note {
                    let note_name = note_names[(note % 12) as usize];
                    let octave = (note / 12) as i8 - 1;
                    let on_off = if self.message_type == MidiMessageType::NoteOn {
                        "ON"
                    } else {
                        "OFF"
                    };

                    info!(
                        "[{}][device id: {}, device name: {}] Note {}: {}{} (note={}, vel={}, ch={})",
                        timestamp,
                        self.device_id,
                        device_name.unwrap_or_default(),
                        on_off,
                        note_name,
                        octave,
                        note,
                        self.velocity.unwrap_or(0),
                        self.channel
                    );
                }
            }
            MidiMessageType::ControlChange => {
                if let (Some(control), Some(value)) = (self.control, self.value) {
                    let cc_name = Self::get_cc_name(control);
                    info!(
                        "[{}][{}] CC: {} (cc={}, val={}, ch={})",
                        timestamp, self.device_id, cc_name, control, value, self.channel
                    );
                }
            }
            MidiMessageType::PitchWheel => {
                if let Some(pitch) = self.pitch {
                    info!(
                        "[{}][{}] Pitch Wheel: {} (ch={})",
                        timestamp,
                        device_name.unwrap_or_default(),
                        pitch,
                        self.channel
                    );
                }
            }
            MidiMessageType::Aftertouch => {
                if let Some(value) = self.value {
                    info!(
                        "[{}][{}] Aftertouch: {} (ch={})",
                        timestamp,
                        device_name.unwrap_or_default(),
                        value,
                        self.channel
                    );
                }
            }
            MidiMessageType::PolyAftertouch => {
                if let Some(value) = self.value {
                    info!(
                        "[{}][{}] PolyAfterTouch: {} (note={} ch={})",
                        timestamp,
                        device_name.unwrap_or_default(),
                        value,
                        self.note.unwrap_or(u8::MAX),
                        self.channel
                    );
                }
            }
            MidiMessageType::ProgramChange => {
                if let Some(value) = self.value {
                    info!(
                        "[{}][{}] Program Change: {} (ch={})",
                        timestamp,
                        device_name.unwrap_or_default(),
                        value,
                        self.channel
                    );
                }
            }
        }
    }
}

pub(crate) struct MidiManager {
    debug: DebugLevel,
    midi_input: MidiInput,
    connections: HashMap<ObjId, (String, MidiInputConnection<()>)>,
    message_sender: mpsc::UnboundedSender<MappedMidiMessage>,
    _engine_stop_token: CancellationToken,
    all_devices_rx: mpsc::UnboundedReceiver<MappedMidiMessage>,
    note_states: HashMap<ObjId, HashSet<u8>>,
}

#[allow(non_upper_case_globals)]
pub(crate) const MIDIv1_CONTROL_INTERVAL: NumInterval<BaseNumT> = NumInterval::<BaseNumT> { from: 0.0, to: 127.0 };

#[allow(non_upper_case_globals)]
pub(crate) const MIDIv1_PITCH_WHEEL_INTERVAL: NumInterval<BaseNumT> = NumInterval::<BaseNumT> {
    from: (i16::MIN / 4) as BaseNumT,
    to: (i16::MAX / 4) as BaseNumT,
};

#[allow(non_upper_case_globals)]
pub(crate) const MIDIv1_CONTROL_RANGE: std::ops::RangeInclusive<BaseNumT> =
    MIDIv1_CONTROL_INTERVAL.make_range_inclusive();

#[allow(non_upper_case_globals)]
pub(crate) const MIDIv1_PITCH_WHEEL_RANGE: std::ops::RangeInclusive<BaseNumT> =
    MIDIv1_PITCH_WHEEL_INTERVAL.make_range_inclusive();

impl MidiManager {
    pub(crate) fn new(debug: DebugLevel) -> Result<Self> {
        let midi_input = MidiInput::new(format!("{} MIDI Input", config::APP_NAME).as_str())
            .context("Failed to create MIDI input")?;

        let (tx, rx) = mpsc::unbounded_channel();

        Ok(Self {
            debug,
            midi_input,
            connections: HashMap::default(),
            _engine_stop_token: CancellationToken::new(),
            message_sender: tx,
            all_devices_rx: rx,
            note_states: HashMap::new(),
        })
    }

    pub(crate) fn enumerate_available_devices(&self) -> Vec<AvailableMidiDeviceInfo> {
        let ports = self.midi_input.ports();
        let mut devices = Vec::new();
        for (i, port) in ports.iter().enumerate() {
            match self.midi_input.port_name(port) {
                Ok(name) => {
                    devices.push(AvailableMidiDeviceInfo { name, port_index: i });
                }
                Err(error) => {
                    log::warn!("Can't open MIDI device port: {error}.");
                }
            }
        }
        devices
    }

    pub(crate) fn filter_by_name_pattern(
        &self,
        device_name_regex: &regex::Regex,
        devices: &[AvailableMidiDeviceInfo],
    ) -> Vec<String> {
        let mut matched = Vec::new();
        for device in devices {
            if device_name_regex.is_match(&device.name) {
                matched.push(device.name.clone());
            }
        }
        matched
    }

    pub(crate) fn open(&mut self, device_name: &str) -> Result<ObjId> {
        if let Some(d) = self.connections.iter().find(|v| v.1.0 == device_name) {
            log::info!("MIDI device {} already opened.", device_name);
            return Ok(*d.0);
        }

        let devices = self.enumerate_available_devices();
        if let Some(device) = devices.iter().find(|d| d.name == device_name) {
            let sender = self.message_sender.clone();
            let debug = self.debug;
            let device_name = device.name.clone();
            let midi_in = MidiInput::new(&format!("{} {}", config::APP_NAME, device_name))?;
            let ports = midi_in.ports();
            let device_id = ObjId::from(intern_str(&device_name));
            if device.port_index >= ports.len() {
                bail!("Invalid port index");
            }
            let port = ports[device.port_index].clone();
            let connection = midi_in
                .connect(
                    &port,
                    &device_name,
                    move |_stamp, message, _| {
                        if let Some(msg) = MappedMidiMessage::from_raw_data(message, device_id, debug) {
                            if debug.is_on() {
                                debug!(
                                    "MIDI {}: {:?}",
                                    get_interned_str(*msg.device_id).unwrap_or_default(),
                                    msg
                                );
                            }
                            let _ = sender.send(msg);
                        }
                    },
                    (),
                )
                .map_err(|e| anyhow::anyhow!("Failed to connect to MIDI device: {}", e))?;

            self.connections
                .insert(device_id, (device_name.to_string(), connection));

            info!("Opened MIDI device: {}", device_name);

            Ok(device_id)
        } else {
            bail!("MIDI device not found: {}", device_name)
        }
    }

    pub(crate) async fn consume_any_opened_device_message(&mut self) -> Option<MappedMidiMessage> {
        if let Some(msg) = self.all_devices_rx.recv().await {
            if msg.message_type == MidiMessageType::NoteOn {
                if let Some(note) = msg.note {
                    if let Some(val) = self.note_states.get_mut(&msg.device_id) {
                        val.insert(note);
                    } else {
                        self.note_states.entry(msg.device_id).or_default().insert(note);
                    }
                }
            } else if msg.message_type == MidiMessageType::NoteOff
                && let Some(note) = msg.note
                && let Some(notes) = self.note_states.get_mut(&msg.device_id)
            {
                notes.remove(&note);
            }

            return Some(msg);
        }
        None
    }

    pub(crate) async fn monitor(&mut self, name_regex: &regex::Regex) -> Result<()> {
        let matched_devices = self.filter_by_name_pattern(name_regex, &self.enumerate_available_devices());

        if matched_devices.is_empty() {
            bail!("No devices found matching '{}'", name_regex);
        }

        info!("Monitoring devices: {:?}", matched_devices);
        info!("Press Ctrl+C to stop monitoring...");
        info!("{}", "=".repeat(60));

        for device_name in &matched_devices {
            self.open(device_name)?;
        }

        while let Some(msg) = self.consume_any_opened_device_message().await {
            msg.pretty_print(Some(
                &self
                    .connections
                    .get(&msg.device_id)
                    .expect(
                        "Trying to resolve device name from device id \
                            but it's not found among opened ones...",
                    )
                    .0,
            ));
        }

        Ok(())
    }

    pub(crate) fn stop(&mut self) -> Result<()> {
        self.connections.clear();
        Ok(())
    }
}

pub(crate) struct MidiLearnMode {
    midi_manager: MidiManager,
    learned_controls: HashMap<ObjId, HashMap<String, MappedMidiMessage>>,
    start_time: std::time::Instant,
}

impl MidiLearnMode {
    pub(crate) fn new(midi_manager: MidiManager) -> Self {
        Self {
            midi_manager,
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
        info!("3. Press Ctrl+C when finished.");
        info!("{}", "=".repeat(60));
        info!("");

        let devices = self.midi_manager.enumerate_available_devices();

        if devices.is_empty() {
            warn!("No MIDI devices found!");
            return Ok(());
        }

        info!("Monitoring {} MIDI device(s):", devices.len());
        for (i, device) in devices.iter().enumerate() {
            info!("  {}. {}", i + 1, device.name);
            self.midi_manager.open(&device.name)?;
        }
        info!("");

        while let Some(msg) = self.midi_manager.consume_any_opened_device_message().await {
            self.process_learn_message(&msg);
        }

        Ok(())
    }

    // TODO: create YAML configuration and later save it.
    fn process_learn_message(&mut self, msg: &MappedMidiMessage) {
        let device_controls = self.learned_controls.entry(msg.device_id).or_default();

        let control_str = match msg.message_type {
            MidiMessageType::NoteOn => {
                if let Some(note) = msg.note {
                    format!("Note number {}", note)
                } else {
                    return;
                }
            }
            MidiMessageType::NoteOff => return,
            MidiMessageType::ControlChange => {
                if let Some(control) = msg.control {
                    if control == 1 {
                        format!("Control Change, Modulation Wheel, control number {}", control)
                    } else {
                        format!("Control Change, control number {}", control)
                    }
                } else {
                    format!("Unknown control change {:?}", msg.message_type)
                }
            }
            _ => msg.message_type.to_string(),
        };

        if let std::collections::hash_map::Entry::Vacant(e) = device_controls.entry(control_str.clone()) {
            let elapsed = self.start_time.elapsed().as_secs_f32();
            info!("[{:6.1}s] Learned: [{}] {}", elapsed, msg.device_id, control_str);

            e.insert(msg.clone());
        }
    }
}

// -------------------------------------------------------------
