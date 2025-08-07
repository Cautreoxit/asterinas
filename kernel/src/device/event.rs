// SPDX-License-Identifier: MPL-2.0

use super::*;
use crate::{
    events::IoEvents,
    fs::inode_handle::FileIo,
    prelude::*,
    process::signal::{PollHandle, Pollable, Pollee},
    syscall::ClockId,
    util::MultiWrite,
};
use aster_input::{register_handler, InputHandler, InputEvent, event_type_codes::EventType};
use alloc::collections::VecDeque;
use spin::{Mutex, Once};
use ostd::sync::SpinLock;
use aster_input::InputDevice;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InputEventLinux {
    pub sec: u64,    // Seconds (time.tv_sec or __sec)
    pub usec: u64,   // Microseconds (time.tv_usec or __usec)
    pub type_: u16,  // Event type
    pub code: u16,   // Event code
    pub value: i32,  // Event value
}

impl InputEventLinux {
    pub fn to_bytes(&self) -> [u8; 24] {
        let mut bytes = [0u8; 24];
        bytes[..8].copy_from_slice(&self.sec.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.usec.to_le_bytes());
        bytes[16..18].copy_from_slice(&self.type_.to_le_bytes());
        bytes[18..20].copy_from_slice(&self.code.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.value.to_le_bytes());
        bytes
    }
}

pub struct EventDevice {
    id: usize,
    clock_id: ClockId,
    event_queue: Arc<SpinLock<VecDeque<InputEventLinux>>>,
    input_device: Arc<dyn InputDevice>,
    pollee: Pollee,
}

static EVENT_DEVICE_HANDLER: Once<Arc<EventDeviceHandler>> = Once::new();

impl EventDevice {
    pub fn new(id: usize, input_device: Arc<dyn InputDevice>) -> Arc<Self> {
        let clock_id = ClockId::CLOCK_MONOTONIC;
        let event_device = Arc::new(Self {
            id,
            clock_id,
            event_queue: Arc::new(SpinLock::new(VecDeque::new())),
            input_device: input_device.clone(),
            pollee: Pollee::new(),
        });

        // Initialize the static handler if it hasn't been initialized yet
        let handler = EVENT_DEVICE_HANDLER.call_once(|| {
            Arc::new(EventDeviceHandler {
                event_devices: Mutex::new(Vec::new()), // Initialize the Mutex
            })
        });

        // Update the handler's weak reference to point to the new EventDevice
        handler.event_devices.lock().push(Arc::downgrade(&event_device));

        // Register the handler
        register_handler(handler.clone());

        // Connect the input_device to the handler
        aster_input::acquire_connection(input_device, handler.clone());

        event_device
    }

    pub fn push_event(&self, event: InputEventLinux) {
        let mut notify = false;
        {
            let mut queue = self.event_queue.disable_irq().lock();
            if queue.len() >= 128 {
                queue.pop_front();
            }
            queue.push_back(event);
            if event.type_ == EventType::EvSyn as u16 {
                notify = true;
            }
        }
        if notify {
            self.pollee.notify(IoEvents::IN);
        }
    }

    pub fn input_device(&self) -> Arc<dyn InputDevice> {
        Arc::clone(&self.input_device)
    }
}

impl Clone for EventDevice {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            clock_id: self.clock_id,
            event_queue: Arc::clone(&self.event_queue),
            input_device: Arc::clone(&self.input_device),
            pollee: self.pollee.clone(),
        }
    }
}

impl Device for EventDevice {
    fn type_(&self) -> DeviceType {
        DeviceType::CharDevice
    }

    fn id(&self) -> DeviceId {
        DeviceId::new(13, self.id as u32)
    }

    fn open(&self) -> Result<Option<Arc<dyn FileIo>>> {
        Ok(Some(Arc::new(self.clone())))
    }
}

impl Pollable for EventDevice {
    fn poll(&self, mask: IoEvents, poller: Option<&mut PollHandle>) -> IoEvents {
        // println!("EventDevice::poll called with mask: {:?}", mask);

        // Use the Pollee mechanism to manage readiness and notifications
        self.pollee.poll_with(mask, poller, || {
            // Check if there are events in the queue
            let queue = self.event_queue.disable_irq().lock();
            if !queue.is_empty() {
                IoEvents::IN // Data is available to read
            } else {
                IoEvents::empty() // No events available
            }
        })
    }
}

impl FileIo for EventDevice {
    fn read(&self, writer: &mut VmWriter) -> Result<usize> {
        // println!("EventDevice::read called");
        let mut queue = self.event_queue.disable_irq().lock(); // Lock the event queue for thread-safe access
        if let Some(event) = queue.pop_front() { // Retrieve the oldest event from the queue
            let event_bytes = event.to_bytes(); // Serialize the event into bytes
            let mut reader = VmReader::from(&event_bytes[..]); // Create a reader for the serialized bytes
            writer.write(&mut reader)?; // Write the serialized event to the writer
            if queue.is_empty() {
                self.pollee.invalidate();
            }
            Ok(event_bytes.len()) // Return the size of the serialized event
        } else {
            Ok(0) // Return 0 if the queue is empty
        }
    }

    fn write(&self, reader: &mut VmReader) -> Result<usize> {
        Ok(reader.remain())
    }
}

#[derive(Debug)]
pub struct EventDeviceHandler {
    event_devices: Mutex<Vec<Weak<EventDevice>>>, // Wrap in a Mutex for mutable access
}

impl InputHandler for EventDeviceHandler {
    /// Specifies the event types this handler can process.
    fn supported_event_types(&self) -> Vec<u16> {
        vec![EventType::EvSyn as u16, EventType::EvKey as u16, EventType::EvRel as u16] // Supports keyboard and mouse events
    }

    /// Handles the input event by pushing it to the event queue.
    fn handle_event(&self, event: InputEvent, str: &str) -> core::result::Result<(), core::convert::Infallible> {
        let devices = self.event_devices.lock();
        for weak_dev in devices.iter() {
            if let Some(event_device) = weak_dev.upgrade() {
                let metadata = event_device.input_device.metadata();
                let name = metadata.name.as_str();
                if name != str {
                    continue;
                }

                // Use the timestamp from the input event instead of getting current time
                // This ensures events from the same input action have the same timestamp
                let sec = event.time / 1_000_000;
                let usec = event.time % 1_000_000;

                // Convert InputEvent to InputEventLinux
                let linux_event = InputEventLinux {
                    sec,
                    usec,
                    type_: event.type_,
                    code: event.code,
                    value: event.value,
                };

                event_device.push_event(linux_event);
            }
        }

        Ok(())
    }
}
