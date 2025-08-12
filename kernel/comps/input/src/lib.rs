// SPDX-License-Identifier: MPL-2.0

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

pub mod event_type_codes;
pub mod input_core;
pub mod input_dev;
pub mod input_handler;

use alloc::sync::Arc;

use component::{init_component, ComponentInitError};
pub use event_type_codes::*;
pub use input_core::{InputCore, InputHandle};
pub use input_dev::{InputCapability, InputDevice, InputId};
pub use input_handler::InputHandler;
use ostd::Pod;
use spin::Once;

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

static INPUT_CORE: Once<InputCore> = Once::new();

// Initialize the input subsystem.
#[init_component]
fn init() -> Result<(), ComponentInitError> {
    let core = InputCore::new();
    INPUT_CORE.call_once(|| core);
    Ok(())
}

/// Get the global input core instance.
fn get_input_core() -> &'static InputCore {
    INPUT_CORE.get().expect("Input subsystem not initialized")
}

/// Register an input device.
pub fn register_device(device: Arc<dyn InputDevice>) -> Result<(), i32> {
    get_input_core().register_device(device)
}

/// Unregister an input device.
pub fn unregister_device(device: &Arc<dyn InputDevice>) -> Result<(), i32> {
    get_input_core().unregister_device(device)
}

/// Register an input handler.
pub fn register_handler(handler: Arc<dyn InputHandler>) -> Result<(), i32> {
    // error!("--------------This is input_register_handler--------------");
    get_input_core().register_handler(handler)
}

/// Unregister an input handler.
pub fn unregister_handler(handler: &Arc<dyn InputHandler>) -> Result<(), i32> {
    get_input_core().unregister_handler(handler)
}

/// Report an input event.
pub fn submit_event(device: &Arc<dyn InputDevice>, event: &InputEvent) {
    get_input_core().submit_event(device, event)
}

// /// Open an input device.
// pub fn input_open_device(handle: &InputHandle) -> Result<(), i32> {
//     get_input_core().open_device(handle)
// }

// /// Close an input device.
// pub fn input_close_device(handle: &InputHandle) -> Result<(), i32> {
//     get_input_core().close_device(handle)
// }

/// List all registered devices.
pub fn list_devices() -> alloc::vec::Vec<Arc<dyn InputDevice>> {
    get_input_core().list_devices()
}

/// List all registered handlers.
pub fn list_handlers() -> alloc::vec::Vec<Arc<dyn InputHandler>> {
    get_input_core().list_handlers()
}

/// List all active handles.
pub fn list_handles() -> alloc::vec::Vec<Arc<InputHandle>> {
    get_input_core().list_handles()
}
