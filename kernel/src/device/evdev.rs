// SPDX-License-Identifier: MPL-2.0

//! Evdev (Event Device) Implementation
//!
//! This module implements the /dev/input/eventX character devices that provide
//! raw input events to userspace applications. It integrates with the input
//! subsystem to automatically create device nodes for all input devices.

use alloc::{format, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use aster_input::{
    event_type_codes::EventType, input_register_handler, InputDevice, InputEvent,
    InputHandle, InputHandler,
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

/// Safe event buffer with minimal locking
/// Optimized for short critical sections, safe for interrupt contexts
struct SafeEventBuffer {
    /// Ring buffer storage
    buffer: SpinLock<SafeRingBuffer>,
    /// Capacity of the buffer
    capacity: usize,
}

/// Internal ring buffer implementation
struct SafeRingBuffer {
    /// Event storage
    events: Vec<InputEvent>,
    /// Write position (head)
    head: usize,
    /// Read position (tail)  
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

impl SafeEventBuffer {
    fn new(capacity: usize) -> Self {
        let capacity = capacity.next_power_of_two().max(4); // Minimum 4 slots
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

/// Evdev client structure - represents one open file handle to an evdev device
/// This corresponds to Linux's `struct evdev_client`
pub struct EvdevClient {
    /// Safe event buffer for this client
    buffer: SafeEventBuffer,
    /// Position of the first element of next packet
    packet_head: AtomicUsize,
    /// Reference to the evdev device
    evdev: Arc<Evdev>,
    /// Client-specific clock type
    clock_type: AtomicU32, // 0=CLOCK_REALTIME, 1=CLOCK_MONOTONIC
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

    /// Add an event to this client's buffer (safe for interrupt context)
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

/// Main evdev structure - represents one input device's evdev interface
/// This corresponds to Linux's `struct evdev`
pub struct Evdev {
    /// Minor device number
    minor: u32,
    /// Reference count of open clients
    open: SpinLock<u32>,
    /// Input device associated with this evdev
    device: Arc<dyn InputDevice>,
    /// List of clients (open file handles)
    client_list: SpinLock<Vec<Arc<EvdevClient>>>,
    /// Client that has grabbed the device exclusively
    grab: SpinLock<Option<Arc<EvdevClient>>>,
    /// Device name for debugging
    device_name: String,
    /// Key state tracking (768 bits for key states)
    key_state: SpinLock<[u32; 24]>, // 768 bits / 32 = 24 u32s
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
            grab: SpinLock::new(None),
            device_name,
            key_state: SpinLock::new([0; 24]),
            exist: SpinLock::new(true),
        }
    }

    /// Add a client to this evdev device
    pub fn attach_client(&self, client: Arc<EvdevClient>) {
        log::error!("--------------This is evdev attach_client--------------");
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
        log::error!("--------------This is evdev pass_event--------------");
        // Update key state for key events
        if event.type_ == EventType::EvKey as u16 {
            self.update_key_state(event.code, event.value);
        }

        let client_list = self.client_list.lock();
        let grab = self.grab.lock();

        if let Some(grabbed_client) = grab.as_ref() {
            // If device is grabbed, only send to the grabbing client
            grabbed_client.push_event(*event);
        } else {
            // Send event to all clients
            for client in client_list.iter() {
                client.push_event(*event);
            }
        }
    }

    /// Update key state tracking
    fn update_key_state(&self, key_code: u16, value: i32) {
        if key_code < 768 {
            let mut key_state = self.key_state.lock();
            let word_idx = (key_code / 32) as usize;
            let bit_idx = key_code % 32;

            if value != 0 {
                // Key pressed
                key_state[word_idx] |= 1 << bit_idx;
            } else {
                // Key released
                key_state[word_idx] &= !(1 << bit_idx);
            }
        }
    }

    /// Get current key state as byte array
    pub fn get_key_state_bytes(&self) -> Vec<u8> {
        let key_state = self.key_state.lock();
        let mut bytes = Vec::with_capacity(96); // 768 bits / 8 = 96 bytes

        for &word in key_state.iter() {
            bytes.extend_from_slice(&word.to_le_bytes());
        }

        bytes
    }

    /// Open device (increment reference count)
    pub fn open_device(&self) -> core::result::Result<(), i32> {
        let mut open = self.open.lock();
        let exist = self.exist.lock();

        if !*exist {
            return Err(-19); // ENODEV
        }

        if *open == 0 {
            // First open - start the device
            // In our implementation, we don't need special start logic
        }

        *open += 1;
        Ok(())
    }

    /// Close device (decrement reference count)
    pub fn close_device(&self) {
        let mut open = self.open.lock();
        if *open > 0 {
            *open -= 1;

            if *open == 0 {
                // Last close - stop the device if needed
                // In evdev, this typically doesn't do much
            }
        }
    }

    /// Grab device for exclusive access by a client
    pub fn grab_device(&self, client: Arc<EvdevClient>) -> core::result::Result<(), i32> {
        let mut grab = self.grab.lock();

        if grab.is_some() {
            return Err(-16); // EBUSY
        }

        *grab = Some(client);

        // In Linux, grabbing also grabs the underlying input device
        // TODO: Implement input_grab_device equivalent on the device itself
        // We could call self.device.grab() if such a method existed

        Ok(())
    }

    /// Release device grab
    pub fn ungrab_device(&self, client: &Arc<EvdevClient>) -> core::result::Result<(), i32> {
        let mut grab = self.grab.lock();

        if let Some(ref grabbed_client) = *grab {
            if Arc::ptr_eq(grabbed_client, client) {
                *grab = None;

                // Release the underlying input device
                // TODO: Implement input_release_device equivalent on the device itself
                // We could call self.device.release() if such a method existed

                Ok(())
            } else {
                Err(-22) // EINVAL
            }
        } else {
            Err(-22) // EINVAL
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
        if let Err(err) = self.evdev.open_device() {
            return Err(Error::with_message(
                Errno::ENODEV,
                "failed to open evdev device",
            ));
        }

        // Open the underlying input device
        if let Err(_) = self.evdev.device.open() {
            // If device open fails, close the evdev device
            let _ = self.evdev.close_device();
            return Err(Error::with_message(
                Errno::ENODEV,
                "failed to open input device",
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
        
        // Close the underlying input device
        let _ = self.client.evdev.device.close();
        
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

    fn ioctl(&self, cmd: IoctlCmd, arg: usize) -> Result<i32> {
        // Implement basic evdev IOCTLs
        // match cmd.raw() {
        //     // EVIOCGVERSION - Get driver version
        //     0x80044501 => Ok(0x010001), // Version 1.0.
        //     // EVIOCGKEY - Get key state
        //     0x80404518 => {
        //         let key_state = self.client.evdev.get_key_state_bytes();
        //         // TODO: Copy key state to user buffer at arg
        //         Ok(key_state.len() as i32)
        //     }

        //     // For now, return EINVAL for other IOCTLs
        //     // TODO: Implement full IOCTL support
        //     _ => return_errno_with_message!(Errno::EINVAL, "unsupported ioctl"),
        // }
        Ok(0)
    }
}

/// Evdev handler that creates device nodes
pub struct EvdevHandler {
    name: String,
}

impl EvdevHandler {
    pub fn new() -> Self {
        Self {
            name: "evdev".to_string(),
        }
    }
}

impl InputHandler for EvdevHandler {
    fn name(&self) -> &str {
        &self.name
    }

    fn match_device(&self, _dev: &Arc<dyn InputDevice>) -> bool {
        // Evdev handler accepts all input devices
        true
    }

    fn connect(&self, dev: Arc<dyn InputDevice>) -> core::result::Result<(), i32> {
        // Allocate a new minor number
        let minor = EVDEV_MINOR_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Create evdev device
        let evdev = Arc::new(Evdev::new(minor, dev.clone()));

        // Store device mapping for event handling
        DEVICE_TO_EVDEV.lock().push((dev.name().to_string(), evdev.clone()));

        // Create the character device and add it to /dev/input/eventX
        let evdev_device = Arc::new(EvdevDevice::new(evdev.clone()));
        let device_path = format!("input/event{}", minor);

        // Create the device node
        match add_node(evdev_device.clone(), &device_path) {
            Ok(_) => {
                // Register the device for cleanup
                EVDEV_DEVICES.lock().push((minor, evdev_device));
                log::info!("Created evdev device node: /dev/{}", device_path);
                Ok(())
            }
            Err(err) => {
                log::error!("Failed to create device node {}: {:?}", device_path, err);
                // Remove from mapping on failure
                DEVICE_TO_EVDEV.lock().retain(|(name, _)| name != dev.name());
                Err(-12) // ENOMEM
            }
        }
    }

    fn disconnect(&self, dev: &Arc<dyn InputDevice>) -> core::result::Result<(), i32> {
        // Find and remove evdev from mapping
        let mut device_mapping = DEVICE_TO_EVDEV.lock();
        let evdev_opt = if let Some(pos) = device_mapping.iter().position(|(name, _)| name == dev.name()) {
            let (_, evdev) = device_mapping.remove(pos);
            Some(evdev)
        } else {
            None
        };
        drop(device_mapping);

        // Remove device node if we have the evdev
        if let Some(evdev) = evdev_opt {
            let minor = evdev.minor;
            let device_path = format!("input/event{}", minor);

            // Mark device as non-existent
            *evdev.exist.lock() = false;

            // Remove from registry
            let mut devices = EVDEV_DEVICES.lock();
            devices.retain(|(m, _)| *m != minor);

            // Delete the device node
            if let Err(err) = delete_node(&device_path) {
                log::warn!("Failed to remove device node {}: {:?}", device_path, err);
            } else {
                log::info!("Removed evdev device node: /dev/{}", device_path);
            }
        }

        Ok(())
    }

    fn start(&self, _dev: &Arc<dyn InputDevice>) -> core::result::Result<(), i32> {
        // Called when the first user opens the evdev device
        Ok(())
    }

    fn event(&self, dev: &Arc<dyn InputDevice>, event: &InputEvent) {
        log::error!("--------------This is evdev event--------------");
        // Find evdev for this device and pass event to all clients
        let device_mapping = DEVICE_TO_EVDEV.lock();
        if let Some((_, evdev)) = device_mapping.iter().find(|(name, _)| name == dev.name()) {
            evdev.pass_event(event);
        }
    }

    fn events(&self, dev: &Arc<dyn InputDevice>, events: &[InputEvent]) {
        // Handle multiple events efficiently
        let device_mapping = DEVICE_TO_EVDEV.lock();
        if let Some((_, evdev)) = device_mapping.iter().find(|(name, _)| name == dev.name()) {
            for event in events {
                evdev.pass_event(event);
            }
        }
    }
}

/// Initialize evdev support in the kernel device system
pub fn init() -> Result<()> {
    let handler = Arc::new(EvdevHandler::new());

    // Register the evdev handler with the input subsystem
    if let Err(err) = input_register_handler(handler) {
        log::error!("Failed to register evdev handler: {}", err);
        return Err(Error::with_message(
            Errno::EINVAL,
            "Failed to register evdev handler",
        ));
    }

    log::error!("Evdev device support initialized");
    Ok(())
}

