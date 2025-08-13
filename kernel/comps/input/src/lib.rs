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

/// Input event structure   //evdev
// TODO: rename
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct InputEvent {
    /// Timestamp seconds since Unix epoch
    pub sec: u64,   //duration
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
    /// Create a new input event with timestamp in microseconds
    pub fn new(time_microseconds: u64, type_: u16, code: u16, value: i32) -> Self {
        Self {
            sec: time_microseconds / 1_000_000,
            usec: time_microseconds % 1_000_000,
            type_,
            code,
            value,
        }
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