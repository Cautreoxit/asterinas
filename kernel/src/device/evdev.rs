// SPDX-License-Identifier: MPL-2.0

use alloc::{format, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use aster_input::{InputDevice, InputEvent, InputHandler, InputHandlerClass};
use aster_time::read_monotonic_time;
use ostd::{
    sync::{Mutex, SpinLock},
    Pod,
};

use crate::{
    events::IoEvents,
    fs::{
        device::{add_node, delete_node, Device, DeviceId, DeviceType},
        inode_handle::FileIo,
        utils::IoctlCmd,
    },
    prelude::*,
    process::signal::{PollHandle, Pollable},
    util::ring_buffer::{RbConsumer, RbProducer, RingBuffer},
    VmReader, VmWriter,
};

/// Maximum number of events in the evdev buffer
const EVDEV_BUFFER_SIZE: usize = 64;

/// Global minor number allocator for evdev devices
static EVDEV_MINOR_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Global registry of evdev devices for cleanup
static EVDEV_DEVICES: SpinLock<Vec<(u32, Arc<Evdev>)>> = SpinLock::new(Vec::new());

// TODO: duration?
// Compatible with Linux's event format
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct EvdevEvent {
    pub sec: u64,
    pub usec: u64,
    pub type_: u16,
    pub code: u16,
    pub value: i32,
}

impl EvdevEvent {
    pub fn from_event_and_time(event: &InputEvent, time_microseconds: u64) -> Self {
        let (type_, code, value) = event.to_raw();
        Self {
            sec: time_microseconds / 1_000_000,
            usec: time_microseconds % 1_000_000,
            type_,
            code,
            value,
        }
    }

    pub fn from_event(event: &InputEvent) -> Self {
        let time = read_monotonic_time();
        let (type_, code, value) = event.to_raw();
        Self {
            sec: time.as_secs(),
            usec: time.subsec_micros() as u64,
            type_,
            code,
            value,
        }
    }

    pub fn to_bytes(&self) -> [u8; 24] {
        let mut bytes = [0u8; 24];
        bytes[0..8].copy_from_slice(&self.sec.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.usec.to_le_bytes());
        bytes[16..18].copy_from_slice(&self.type_.to_le_bytes());
        bytes[18..20].copy_from_slice(&self.code.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.value.to_le_bytes());
        bytes
    }
}

pub struct EvdevClient {
    /// Consumer for reading events (used by user space)
    consumer: Mutex<RbConsumer<EvdevEvent>>,
    /// Client-specific clock type
    clock_type: AtomicU32,
    /// Number of events available
    event_count: AtomicUsize,
}

impl EvdevClient {
    fn new(evdev: Arc<Evdev>, buffer_size: usize) -> (Self, RbProducer<EvdevEvent>) {
        let (producer, consumer) = RingBuffer::new(buffer_size).split();

        let client = Self {
            consumer: Mutex::new(consumer),
            clock_type: AtomicU32::new(1), // Default to be CLOCK_MONOTONIC
            event_count: AtomicUsize::new(0),
        };
        (client, producer)
    }

    /// Read events from this client's buffer
    pub fn read_events(&self, count: usize) -> Vec<EvdevEvent> {
        let mut events = Vec::new();
        let mut consumer = self.consumer.lock();

        for _ in 0..count {
            if let Some(event) = consumer.pop() {
                events.push(event);
                self.event_count.fetch_sub(1, Ordering::Relaxed);
            } else {
                break;
            }
        }

        events
    }

    /// Check if buffer has events
    pub fn has_events(&self) -> bool {
        self.event_count.load(Ordering::Relaxed) > 0
    }

    /// Increment event count
    pub fn increment_event_count(&self) {
        self.event_count.fetch_add(1, Ordering::Relaxed);
    }
}

// Note: Drop for EvdevClient is handled by Evdev::detach_client
// when the Arc<EvdevClient> is removed from the client list

impl Pollable for EvdevClient {
    fn poll(&self, mask: IoEvents, _poller: Option<&mut PollHandle>) -> IoEvents {
        // Check if there are events available to read
        let has_events = self.has_events();

        let mut events = IoEvents::empty();
        if has_events && mask.contains(IoEvents::IN) {
            events |= IoEvents::IN;
        }
        if mask.contains(IoEvents::OUT) {
            events |= IoEvents::OUT; // Always writable
        }

        events
    }
}

impl FileIo for EvdevClient {
    fn read(&self, writer: &mut VmWriter) -> Result<usize> {
        // Read one event at a time (24 bytes per Linux input event)
        const EVENT_SIZE: usize = 24;

        let events = self.read_events(1);
        if let Some(event) = events.first() {
            let event_bytes = event.to_bytes();
            writer.write_fallible(&mut event_bytes.as_slice().into())?;
            return Ok(EVENT_SIZE);
        }

        Ok(0)
    }

    fn write(&self, reader: &mut VmReader) -> Result<usize> {
        // Writing to evdev devices is typically not supported in read-only mode
        Ok(reader.remain())
    }

    fn ioctl(&self, _cmd: IoctlCmd, _arg: usize) -> Result<i32> {
        Ok(0)
    }
}

impl core::fmt::Debug for EvdevClient {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.debug_struct("EvdevClient")
            .field("event_count", &self.event_count.load(Ordering::Relaxed))
            .field("clock_type", &self.clock_type.load(Ordering::Relaxed))
            .finish()
    }
}

pub struct Evdev {
    /// Minor device number
    minor: u32,
    /// Reference count of open clients
    open: SpinLock<u32>,
    /// Input device associated with this evdev
    device: Arc<dyn InputDevice>,
    /// List of clients with their producers (open file handles)
    client_list: SpinLock<Vec<(Arc<EvdevClient>, RbProducer<EvdevEvent>)>>,
    /// Device name
    device_name: String,
    /// Whether device exists (not disconnected)
    exist: SpinLock<bool>,
}

impl core::fmt::Debug for Evdev {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.debug_struct("Evdev")
            .field("minor", &self.minor)
            .field("device_name", &self.device_name)
            .field("open", &self.open)
            .field("exist", &self.exist)
            .finish()
    }
}

impl Evdev {
    fn new(minor: u32, device: Arc<dyn InputDevice>) -> Self {
        let device_name = device.name().to_string();
        Self {
            minor,
            open: SpinLock::new(0),
            device,
            client_list: SpinLock::new(Vec::new()),
            device_name,
            exist: SpinLock::new(true),
        }
    }

    /// Check if this evdev device is associated with the given input device
    pub fn matches_input_device(&self, input_device: &Arc<dyn InputDevice>) -> bool {
        Arc::ptr_eq(&self.device, input_device)
    }

    /// Get the minor device number
    pub fn minor(&self) -> u32 {
        self.minor
    }

    /// Add a client to this evdev device
    pub fn attach_client(&self, client: Arc<EvdevClient>, producer: RbProducer<EvdevEvent>) {
        let mut client_list = self.client_list.lock();
        client_list.push((client, producer));
    }

    /// Remove a client from this evdev device
    pub fn detach_client(&self, client: &Arc<EvdevClient>) {
        let mut client_list = self.client_list.lock();
        client_list.retain(|(c, _)| !Arc::ptr_eq(c, client));
    }

    /// Distribute an event to all clients
    // TODO: if full?
    // TODO: SYN's timestamp
    pub fn pass_event(&self, event: &InputEvent) {
        // Convert InputEvent to EvdevEvent by adding current timestamp
        let timed_event = EvdevEvent::from_event(event);

        let mut client_list = self.client_list.lock();

        // Send event to all clients using their producers
        for (client, producer) in client_list.iter_mut() {
            // Try to push event to the buffer
            if let Some(()) = producer.push(timed_event) {
                // Successfully pushed event, increment counter
                client.increment_event_count();
            } else {
                // Buffer is full, create a SYN_DROPPED event and force push
                let dropped_event = EvdevEvent::from_event_and_time(
                    &InputEvent::sync(aster_input::event_type_codes::SynEvent::SynDropped),
                    timed_event.sec * 1_000_000 + timed_event.usec,
                );

                // Try to push the dropped event
                if producer.push(dropped_event).is_some() {
                    client.increment_event_count();
                }
            }

            // Note: SYN_REPORT events mark the end of input packets
            // Currently we don't need special handling for packet boundaries
        }
    }

    pub fn open_device(&self) -> Result<i32> {
        let mut open = self.open.lock();
        let exist = self.exist.lock();

        if !*exist {
            return_errno_with_message!(Errno::ENODEV, "no device found");
        }

        *open += 1;
        Ok(0)
    }

    pub fn close_device(&self) {
        let mut open = self.open.lock();
        if *open > 0 {
            *open -= 1;
        }
    }

    /// Create a new client for this evdev device (requires Arc<Self>)
    pub fn create_client(self: &Arc<Self>, buffer_size: usize) -> Result<Arc<dyn FileIo>> {
        // Open the device first
        self.open_device()?;

        // Create the client and get the producer
        let (client, producer) = EvdevClient::new(self.clone(), buffer_size);
        let client = Arc::new(client);

        // Attach the client to this device
        self.attach_client(client.clone(), producer);

        Ok(client as Arc<dyn FileIo>)
    }
}

impl Device for Evdev {
    fn type_(&self) -> DeviceType {
        DeviceType::CharDevice
    }

    fn id(&self) -> DeviceId {
        // Linux input devices use major number 13
        DeviceId::new(13, self.minor)
    }

    fn open(&self) -> Result<Option<Arc<dyn FileIo>>> {
        // Find the Arc<Evdev> from the global registry
        let devices = EVDEV_DEVICES.lock();
        if let Some((_, evdev)) = devices.iter().find(|(minor, _)| *minor == self.minor) {
            // Create a new client for this evdev device
            match evdev.create_client(EVDEV_BUFFER_SIZE) {
                Ok(client) => Ok(Some(client)),
                Err(e) => Err(e),
            }
        } else {
            return_errno_with_message!(Errno::ENODEV, "evdev device not found in registry");
        }
    }
}

impl Pollable for Evdev {
    fn poll(&self, mask: IoEvents, _poller: Option<&mut PollHandle>) -> IoEvents {
        // This shouldn't be called directly, but we need to implement it
        // for the Device trait requirement
        mask & IoEvents::OUT // Always indicate writable
    }
}

impl FileIo for Evdev {
    fn read(&self, _writer: &mut VmWriter) -> Result<usize> {
        // This shouldn't be called directly since we return a different FileIo in open()
        return_errno_with_message!(Errno::ENODEV, "direct read on evdev device not supported");
    }

    fn write(&self, _reader: &mut VmReader) -> Result<usize> {
        // This shouldn't be called directly since we return a different FileIo in open()
        return_errno_with_message!(Errno::ENODEV, "direct write on evdev device not supported");
    }

    fn ioctl(&self, _cmd: IoctlCmd, _arg: usize) -> Result<i32> {
        // This shouldn't be called directly since we return a different FileIo in open()
        return_errno_with_message!(Errno::ENODEV, "direct ioctl on evdev device not supported");
    }
}

/// Evdev handler class that creates device nodes for input devices
#[derive(Debug)]
pub struct EvdevHandlerClass {
    name: String,
}

impl EvdevHandlerClass {
    pub fn new() -> Self {
        Self {
            name: "evdev".to_string(),
        }
    }
}

impl InputHandlerClass for EvdevHandlerClass {
    fn name(&self) -> &str {
        &self.name
    }

    fn connect(
        &self,
        dev: Arc<dyn InputDevice>,
    ) -> core::result::Result<Arc<dyn InputHandler>, ()> {
        // Allocate a new minor number
        let minor = EVDEV_MINOR_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Create evdev device
        let evdev = Arc::new(Evdev::new(minor, dev.clone()));
        let device_path = format!("input/event{}", minor);

        // Create the device node
        match add_node(evdev.clone(), &device_path) {
            Ok(_) => {
                EVDEV_DEVICES.lock().push((minor, evdev.clone()));

                // Create handler instance for this device
                let handler = EvdevHandler::new(minor as u64, dev.name().to_string(), evdev);
                Ok(Arc::new(handler))
            }
            Err(_err) => Err(()),
        }
    }

    fn disconnect(&self, dev: &Arc<dyn InputDevice>) {
        let mut devices = EVDEV_DEVICES.lock();
        let device_name = dev.name();

        // Find the device by checking if it matches the input device
        if let Some(pos) = devices
            .iter()
            .position(|(_, evdev)| evdev.matches_input_device(dev))
        {
            let (minor, evdev) = devices.remove(pos);
            let device_path = format!("input/event{}", minor);

            // Delete the device node
            match delete_node(&device_path) {
                Ok(_) => {
                    log::info!(
                        "Successfully removed evdev device node: /dev/{} for device '{}'",
                        device_path,
                        device_name
                    );
                }
                Err(err) => {
                    log::error!(
                        "Failed to remove device node /dev/{} for device '{}': {:?}",
                        device_path,
                        device_name,
                        err
                    );
                }
            }
        } else {
            log::warn!(
                "Attempted to disconnect device '{}' but it did not connect to evdev",
                device_name
            );
        }
    }
}

/// Evdev handler instance for a specific device
#[derive(Debug)]
pub struct EvdevHandler {
    minor: u64,
    device_name: String,
    evdev: Arc<Evdev>,
}

impl EvdevHandler {
    fn new(minor: u64, device_name: String, evdev: Arc<Evdev>) -> Self {
        Self {
            minor,
            device_name,
            evdev,
        }
    }
}

impl InputHandler for EvdevHandler {
    fn class_name(&self) -> &str {
        "evdev"
    }

    fn handle_event(&self, event: &InputEvent) {
        // Forward event to the evdev device
        self.evdev.pass_event(event);
    }

    // TODO: impl handle_events()
}

/// Initialize evdev support in the kernel device system
pub fn init() -> Result<()> {
    let handler_class = Arc::new(EvdevHandlerClass::new());

    // Register the evdev handler class with the input subsystem
    aster_input::register_handler_class(handler_class);

    log::info!("Evdev device support initialized");
    Ok(())
}

// TODO:
// struct EvdevInputHandler {
//     clients: RwLock<Vec<EvdevClient>>,
// }
