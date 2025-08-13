// SPDX-License-Identifier: MPL-2.0

use alloc::{format, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use aster_input::{
    event_type_codes::EventType, InputDevice, InputEvent, InputHandlerClass, InputHandler,
};
use ostd::sync::SpinLock;

use crate::{
    events::IoEvents,
    fs::{
        device::{add_node, delete_node, Device, DeviceId, DeviceType},
        inode_handle::FileIo,
        utils::IoctlCmd,
    },
    prelude::*,
    process::signal::{PollHandle, Pollable},
    VmReader, VmWriter,
};

/// Maximum number of events in the evdev buffer
const EVDEV_BUFFER_SIZE: usize = 64;

/// Global minor number allocator for evdev devices
static EVDEV_MINOR_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Global registry of evdev devices for cleanup
static EVDEV_DEVICES: SpinLock<Vec<(u32, Arc<EvdevDevice>)>> = SpinLock::new(Vec::new());

/// Global mapping from device name to evdev instance
static DEVICE_TO_EVDEV: SpinLock<Vec<(String, Arc<Evdev>)>> = SpinLock::new(Vec::new());

struct SafeRingBuffer {
    /// Event storage
    events: Vec<InputEvent>,
    /// Write position
    head: usize,
    /// Read position
    tail: usize,
    /// Actual capacity
    capacity: usize,
}

impl SafeRingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            events: vec![InputEvent::new(0, 0, 0, 0); capacity],
            head: 0,
            tail: 0,
            capacity,
        }
    }

    /// Check if buffer is full
    fn is_full(&self) -> bool {
        self.len() >= self.capacity - 1 // Leave one slot empty to distinguish full from empty
    }

    /// Check if buffer is empty
    fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    /// Get number of events in buffer
    fn len(&self) -> usize {
        if self.head >= self.tail {
            self.head - self.tail
        } else {
            self.capacity - self.tail + self.head
        }
    }

    /// Push an event to the buffer
    fn push(&mut self, event: InputEvent) -> bool {
        if self.is_full() {
            return false;
        }

        self.events[self.head] = event;
        self.head = (self.head + 1) % self.capacity;
        true
    }

    /// Pop an event from the buffer
    fn pop(&mut self) -> Option<InputEvent> {
        if self.is_empty() {
            return None;
        }

        let event = self.events[self.tail];
        self.tail = (self.tail + 1) % self.capacity;
        Some(event)
    }

    /// Force push by dropping the oldest event if buffer is full
    fn force_push(&mut self, event: InputEvent) {
        if self.is_full() {
            // Drop oldest event
            self.tail = (self.tail + 1) % self.capacity;
        }

        self.events[self.head] = event;
        self.head = (self.head + 1) % self.capacity;
    }
}

struct SafeEventBuffer {
    /// Ring buffer storage
    buffer: SpinLock<SafeRingBuffer>,
    /// Capacity of the buffer
    capacity: usize,
}

impl SafeEventBuffer {
    fn new(capacity: usize) -> Self {
        let capacity = capacity.next_power_of_two();
        Self {
            buffer: SpinLock::new(SafeRingBuffer::new(capacity)),
            capacity,
        }
    }

    /// Try to push an event to the buffer
    fn push(&self, event: InputEvent) -> bool {
        let mut buffer = self.buffer.lock();
        buffer.push(event)
    }

    /// Force push an event, dropping oldest if necessary
    fn force_push(&self, event: InputEvent) {
        let mut buffer = self.buffer.lock();
        buffer.force_push(event)
    }

    /// Pop an event from the buffer
    fn pop(&self) -> Option<InputEvent> {
        let mut buffer = self.buffer.lock();
        buffer.pop()
    }

    /// Check if buffer has events
    fn is_empty(&self) -> bool {
        let buffer = self.buffer.lock();
        buffer.is_empty()
    }

    /// Get number of events in buffer
    fn len(&self) -> usize {
        let buffer = self.buffer.lock();
        buffer.len()
    }
}

pub struct EvdevClient {
    /// Safe event buffer for this client
    buffer: SafeEventBuffer,
    /// Position of the first element of next packet
    packet_head: AtomicUsize,
    /// Reference to the evdev device
    evdev: Arc<Evdev>,
    /// Client-specific clock type
    clock_type: AtomicU32,
}

impl EvdevClient {
    fn new(evdev: Arc<Evdev>, buffer_size: usize) -> Self {
        Self {
            buffer: SafeEventBuffer::new(buffer_size),
            packet_head: AtomicUsize::new(0),
            evdev,
            clock_type: AtomicU32::new(1), // Default to CLOCK_MONOTONIC
        }
    }

    /// Add an event to this client's buffer
    pub fn push_event(&self, event: InputEvent) {
        // Try to push event to the buffer
        if !self.buffer.push(event) {
            // Buffer is full, create a SYN_DROPPED event and force push
            let time_microseconds = event.sec * 1_000_000 + event.usec;
            let dropped_event = InputEvent::new(
                time_microseconds,
                EventType::EvSyn as u16,
                aster_input::event_type_codes::SynEvent::SynDropped as u16,
                0,
            );

            // Force push the dropped event (will automatically drop oldest)
            self.buffer.force_push(dropped_event);

            // Update packet head for dropped events
            self.packet_head.store(0, Ordering::Release);
        }

        // Update packet head for SYN_REPORT events
        if event.type_ == EventType::EvSyn as u16
            && event.code == aster_input::event_type_codes::SynEvent::SynReport as u16
        {
            // For now, just reset packet head - more sophisticated tracking could be added
            self.packet_head.store(0, Ordering::Release);
        }
    }

    /// Read events from this client's buffer
    pub fn read_events(&self, count: usize) -> Vec<InputEvent> {
        let mut events = Vec::new();

        for _ in 0..count {
            if let Some(event) = self.buffer.pop() {
                events.push(event);
            } else {
                break;
            }
        }

        events
    }

    /// Check if buffer has events
    pub fn has_events(&self) -> bool {
        !self.buffer.is_empty()
    }
}

impl core::fmt::Debug for EvdevClient {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.debug_struct("EvdevClient")
            .field("buffer_len", &self.buffer.len())
            .field("packet_head", &self.packet_head.load(Ordering::Relaxed))
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
    /// List of clients (open file handles)
    client_list: SpinLock<Vec<Arc<EvdevClient>>>,
    /// Device name for debugging
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

    /// Add a client to this evdev device
    pub fn attach_client(&self, client: Arc<EvdevClient>) {
        let mut client_list = self.client_list.lock();
        client_list.push(client);
    }

    /// Remove a client from this evdev device
    pub fn detach_client(&self, client: &Arc<EvdevClient>) {
        let mut client_list = self.client_list.lock();
        client_list.retain(|c| !Arc::ptr_eq(c, client));
    }

    /// Distribute an event to all clients
    pub fn pass_event(&self, event: &InputEvent) {
        let client_list = self.client_list.lock();

        // Send event to all clients
        for client in client_list.iter() {
            client.push_event(*event);
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
}

/// Character device that represents /dev/input/eventX
pub struct EvdevDevice {
    evdev: Arc<Evdev>,
}

impl EvdevDevice {
    pub fn new(evdev: Arc<Evdev>) -> Self {
        Self { evdev }
    }
}

impl Device for EvdevDevice {
    fn type_(&self) -> DeviceType {
        DeviceType::CharDevice
    }

    fn id(&self) -> DeviceId {
        // Linux input devices use major number 13
        DeviceId::new(13, self.evdev.minor)
    }

    fn open(&self) -> Result<Option<Arc<dyn FileIo>>> {
        // Create a new client for this open operation
        let client = Arc::new(EvdevClient::new(self.evdev.clone(), EVDEV_BUFFER_SIZE));

        // Open the evdev device
        if let Err(_err) = self.evdev.open_device() {
            return Err(Error::with_message(
                Errno::ENODEV,
                "failed to open evdev device",
            ));
        }

        // Attach the client
        self.evdev.attach_client(client.clone());

        Ok(Some(Arc::new(EvdevFileIo::new(client))))
    }
}

impl Pollable for EvdevDevice {
    fn poll(&self, mask: IoEvents, _poller: Option<&mut PollHandle>) -> IoEvents {
        // This shouldn't be called directly, but we need to implement it
        // for the Device trait requirement
        mask & IoEvents::OUT // Always indicate writable
    }
}

impl FileIo for EvdevDevice {
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

/// FileIo implementation for evdev devices
pub struct EvdevFileIo {
    client: Arc<EvdevClient>,
}

impl EvdevFileIo {
    pub fn new(client: Arc<EvdevClient>) -> Self {
        Self { client }
    }
}

impl Drop for EvdevFileIo {
    fn drop(&mut self) {
        // Detach client and close device when file is closed
        self.client.evdev.detach_client(&self.client);

        self.client.evdev.close_device();
    }
}

impl Pollable for EvdevFileIo {
    fn poll(&self, mask: IoEvents, _poller: Option<&mut PollHandle>) -> IoEvents {
        // Check if there are events available to read
        let has_events = self.client.has_events();

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

impl FileIo for EvdevFileIo {
    fn read(&self, writer: &mut VmWriter) -> Result<usize> {
        // Read one event at a time (24 bytes per Linux input event with 64-bit timestamps)
        const EVENT_SIZE: usize = 24;

        let events = self.client.read_events(1);
        if let Some(event) = events.first() {
            let event_bytes = event.to_bytes();
            writer.write_fallible(&mut event_bytes.as_slice().into())?;
            return Ok(EVENT_SIZE);
        }

        // No events available - would block in a real implementation
        Ok(0)
    }

    fn write(&self, reader: &mut VmReader) -> Result<usize> {
        // Writing to evdev devices is typically not supported in read-only mode
        // But we consume the data to maintain compatibility
        Ok(reader.remain())
    }

    fn ioctl(&self, _cmd: IoctlCmd, _arg: usize) -> Result<i32> {
        Ok(0)
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

    fn connect(&self, dev: Arc<dyn InputDevice>) -> core::result::Result<Arc<dyn InputHandler>, i32> {
        // Evdev handler accepts all input devices
        // Allocate a new minor number
        let minor = EVDEV_MINOR_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Create evdev device
        let evdev = Arc::new(Evdev::new(minor, dev.clone()));

        // Create the character device and add it to /dev/input/eventX
        let evdev_device = Arc::new(EvdevDevice::new(evdev.clone()));
        let device_path = format!("input/event{}", minor);

        // Create the device node
        match add_node(evdev_device.clone(), &device_path) {
            Ok(_) => {
                EVDEV_DEVICES.lock().push((minor, evdev_device));

                // Create handler instance for this device
                let handler = EvdevHandler::new(minor as u64, dev.name().to_string(), evdev);
                Ok(Arc::new(handler))
            }
            Err(_err) => {
                Err(-12) // ENOMEM
            }
        }
    }

    fn disconnect(&self, dev: &Arc<dyn InputDevice>) -> core::result::Result<(), i32> {
        // Find and remove evdev from mapping by device name
        let _device_name = dev.name();
        let mut devices = EVDEV_DEVICES.lock();
        
        // Find the device by checking evdev metadata
        if let Some(pos) = devices.iter().position(|(_, _evdev_device)| {
            // This is a simplified check - in real implementation, 
            // we'd need to properly track device name to minor mapping
            true // For now, just remove all - this should be improved
        }) {
            let (minor, _) = devices.remove(pos);
            let device_path = format!("input/event{}", minor);
            
            // Delete the device node
            if let Err(err) = delete_node(&device_path) {
                log::warn!("Failed to remove device node {}: {:?}", device_path, err);
            } else {
                log::info!("Removed evdev device node: /dev/{}", device_path);
            }
        }
        
        Ok(())
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

    fn start(&self) -> core::result::Result<(), i32> {
        log::info!("Starting evdev handler for device: {} (minor: {})", 
                   self.device_name, self.minor);
        Ok(())
    }

    fn handle_event(&self, event: &InputEvent) {
        // Forward event to the evdev device
        self.evdev.pass_event(event);
    }
}

/// Initialize evdev support in the kernel device system
pub fn init() -> Result<()> {
    let handler_class = Arc::new(EvdevHandlerClass::new());

    // Register the evdev handler class with the input subsystem
    if let Err(err) = aster_input::register_handler_class(handler_class) {
        log::error!("Failed to register evdev handler class: {}", err);
        return Err(Error::with_message(
            Errno::EINVAL,
            "Failed to register evdev handler class",
        ));
    }

    log::info!("Evdev device support initialized");
    Ok(())
}



// struct EvdevInputHandler {
//     clients: RwLock<Vec<EvdevClient>>,
// }