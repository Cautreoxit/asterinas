// SPDX-License-Identifier: MPL-2.0

use alloc::{string::String, sync::Arc};
use core::any::Any;

use crate::{InputDevice, InputEvent};

/// Input handler trait - equivalent to Linux struct input_handler
pub trait InputHandler: Send + Sync + Any {
    /// Handler name
    fn name(&self) -> &str;

    /// Check if handler matches device
    fn match_device(&self, dev: &Arc<dyn InputDevice>) -> bool;

    /// Connect handler to device
    /// Returns Ok(()) if connection successful, Err(code) otherwise
    fn connect(&self, dev: Arc<dyn InputDevice>) -> Result<(), i32>;

    /// Disconnect handler from device
    fn disconnect(&self, dev: &Arc<dyn InputDevice>) -> Result<(), i32>;

    /// Start handler
    fn start(&self, dev: &Arc<dyn InputDevice>) -> Result<(), i32> {
        Ok(())
    }

    /// Handle single event
    fn event(&self, dev: &Arc<dyn InputDevice>, event: &InputEvent);

    /// Handle multiple events
    fn events(&self, dev: &Arc<dyn InputDevice>, events: &[InputEvent]) {
        for event in events {
            self.event(dev, event);
        }
    }
}