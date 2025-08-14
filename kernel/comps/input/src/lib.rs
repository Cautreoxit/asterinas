// SPDX-License-Identifier: MPL-2.0

//! The input devices of Asterinas.
//!
//! This crate provides a comprehensive input subsystem for handling various input devices,
//! including keyboards, mice, etc. It implements an event-driven architecture similar to
//! the Linux input subsystem.
//!
//! # Architecture
//!
//! ```text
//! Input Device → Input Core → Input Handler
//!      ↓             ↓            ↓
//!   Hardware    Event Router   Event Consumer
//!                              (e.g., evdev)
//! ```
//!
//! # Example Usage
//!
//! ```no_run
//! // Register an input device
//! let device = Arc::new(MyInputDevice::new());
//! input::register_device(device)?;
//!
//! // Register an input handler
//! let handler = Arc::new(MyInputHandler::new());
//! input::register_handler(handler)?;
//!
//! // Submit an event from device
//! let event = InputEvent::new(timestamp, EV_KEY, KEY_A, 1);
//! input::submit_event(&device, &event);
//! ```
//!
#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

pub mod event_type_codes;
mod input_core;
pub mod input_dev;
pub mod input_handler;

use alloc::{sync::Arc, vec::Vec};

use component::{init_component, ComponentInitError};
pub use event_type_codes::*;
pub use input_dev::{InputCapability, InputDevice, InputId};
pub use input_handler::{InputHandler, InputHandlerClass};
pub use input_core::RegisteredInputDevice;
use ostd::Pod;
use spin::Once;

use self::input_core::InputCore;

/// Input event structure without timing information for device-to-core communication
/// This matches the Linux kernel design where drivers submit events without timestamps.
///
/// Use an enum to provide type-safe event data based on event type.
/// For now we only implement EV_SYN, EV_KEY, EV_REL. Other types are TODO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// Synchronization events (EV_SYN)
    Sync {
        sync_type: event_type_codes::SynEvent,
        value: i32,
    },
    /// Key press/release events (EV_KEY)
    Key {
        key: event_type_codes::KeyEvent,
        status: event_type_codes::KeyStatus,
    },
    /// Relative movement events (EV_REL)
    Relative {
        axis: event_type_codes::RelEvent,
        value: i32,
    },
    // TODO: Add EV_ABS, EV_MSC, EV_SW, EV_LED, EV_SND, ... as needed
}

// TODO: duration?
/// Input event structure with timing information for evdev clients
/// This matches the Linux input_event structure exposed to userspace
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct InputEventTimed {
    /// Timestamp seconds since Unix epoch
    pub sec: u64,
    /// Timestamp microseconds part  
    pub usec: u64,
    /// Event type (EV_KEY, EV_REL, EV_ABS, etc.)
    pub type_: u16,
    /// Event code (KEY_A, REL_X, etc.)
    pub code: u16,
    /// Event value
    pub value: i32,
}

impl InputEvent {
    /// Create a synchronization event
    pub fn sync(sync_type: event_type_codes::SynEvent, value: i32) -> Self {
        Self::Sync { sync_type, value }
    }

    /// Create a key event
    pub fn key(key: event_type_codes::KeyEvent, status: event_type_codes::KeyStatus) -> Self {
        Self::Key { key, status }
    }

    /// Create a relative movement event
    pub fn relative(axis: event_type_codes::RelEvent, value: i32) -> Self {
        Self::Relative { axis, value }
    }

    /// Convert enum to raw Linux input event triplet (type, code, value)
    pub fn to_raw(&self) -> (u16, u16, i32) {
        match self {
            InputEvent::Sync { sync_type, value } => (
                event_type_codes::EventType::EvSyn as u16,
                *sync_type as u16,
                *value,
            ),
            InputEvent::Key { key, status } => (
                event_type_codes::EventType::EvKey as u16,
                *key as u16,
                *status as i32,
            ),
            InputEvent::Relative { axis, value } => (
                event_type_codes::EventType::EvRel as u16,
                *axis as u16,
                *value,
            ),
        }
    }

    /// Get the event type
    pub fn event_type(&self) -> event_type_codes::EventType {
        match self {
            InputEvent::Sync { .. } => event_type_codes::EventType::EvSyn,
            InputEvent::Key { .. } => event_type_codes::EventType::EvKey,
            InputEvent::Relative { .. } => event_type_codes::EventType::EvRel,
        }
    }
}

impl InputEventTimed {
    /// Create a new timed input event from an InputEvent and timestamp
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

    /// Create a new timed input event with current system time
    pub fn from_event(event: &InputEvent) -> Self {
        use aster_time::read_monotonic_time;
        let time = read_monotonic_time();
        let time_microseconds = time.as_micros() as u64;
        Self::from_event_and_time(event, time_microseconds)
    }

    /// Convert to byte array for userspace (compatible with Linux struct input_event)
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

/// Register an input device using new architecture.
pub fn register_device(device: Arc<dyn InputDevice>) -> Result<RegisteredInputDevice, i32> {
    COMPONENT
        .get()
        .unwrap()
        .input_core
        .register_device(device)
}

/// Unregister an input device
pub fn unregister_device(device: &Arc<dyn InputDevice>) -> Result<(), i32> {
    COMPONENT
        .get()
        .unwrap()
        .input_core
        .unregister_device(device)
}

/// Get device count
pub fn device_count() -> usize {
    COMPONENT
        .get()
        .unwrap()
        .input_core
        .device_count()
}

/// Get handler class count
pub fn handler_class_count() -> usize {
    COMPONENT
        .get()
        .unwrap()
        .input_core
        .handler_class_count()
}

/// Register a handler class
pub fn register_handler_class(handler_class: Arc<dyn InputHandlerClass>) -> Result<(), i32> {
    COMPONENT
        .get()
        .unwrap()
        .input_core
        .register_handler_class(handler_class)
}

/// Unregister a handler class
pub fn unregister_handler_class(handler_class: &Arc<dyn InputHandlerClass>) -> Result<(), i32> {
    COMPONENT
        .get()
        .unwrap()
        .input_core
        .unregister_handler_class(handler_class)
}

/// Get all registered devices
pub fn all_devices() -> Vec<Arc<dyn InputDevice>> {
    COMPONENT
        .get()
        .unwrap()
        .input_core
        .all_devices()
}


static COMPONENT: Once<Component> = Once::new();

#[init_component]
fn component_init() -> Result<(), ComponentInitError> {
    let component = Component::init()?;
    COMPONENT.call_once(|| component);
    Ok(())
}

#[derive(Debug)]
struct Component {
    input_core: InputCore,
}

impl Component {
    pub fn init() -> Result<Self, ComponentInitError> {
        Ok(Self {
            input_core: InputCore::new(),
        })
    }
}