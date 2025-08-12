// SPDX-License-Identifier: MPL-2.0

use alloc::sync::Arc;
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
    fn handle_event(&self, dev: &Arc<dyn InputDevice>, event: &InputEvent);

    /// Handle multiple events
    fn handle_events(&self, dev: &Arc<dyn InputDevice>, events: &[InputEvent]) {
        for event in events {
            self.handle_event(dev, event);
        }
    }
}





// pub trait InputHandlerClass: Send + Sync + Any {
//     /// Handler name
//     fn name(&self) -> &str;

//     /// Check if handler matches device
//     // fn match_device(&self, dev: &Arc<dyn InputDevice>) -> bool;

//     /// Connect handler to device
//     /// Returns Ok(()) if connection successful, Err(code) otherwise
//     fn match(&self, dev: Arc<dyn InputDevice>) -> Result<Arc<dyn InputHandler>, i32>;
// }

// struct InputCore {
//     devices: RwLock<Vec<InputDeviceRegistry>>,
//     handler_classes: RwLock<Vec<Arc<dyn InputDevice>>>,
// }
// struct InputDeviceRegistry {
//     device: Arc<dyn InputDevice>,
//     handlers: Arc<RwLock<Vec<Arc<dyn InputHandler>>>,
// }

// // useless?
// struct Handle {
//     device: Arc<dyn InputDevice>,
//     class: Arc<dyn InputHandlerClass>
// }

// pub struct RegisteredInputDevice {
//     handlers: Arc<RwLock<Vec<Arc<dyn InputHandler>>>>,
// }

// impl RegisteredInputDevice {
//     pub fn submit_event(&self, event: InputEvent) {
//         let handlers = self.handlers.read();
//         for hanlder in handlers {
//             hanlder.handle_event(event);
//         }        
//     }
// }

// impl Drop for RegisteredInputDevice {
//     fn drop(&mut self) {
//         ...
//     }
// }
// trait InputHandler {
//     /// Handle single event
//     fn handle_event(&self, event: &InputEvent);

//     /// Handle multiple events
//     fn handle_events(&self, events: &[InputEvent]) {
//         for event in events { 
//             self.event(dev, event);
//         }
//     }
// }

// struct EvedevInputHandler {
//     clients: RwLock<Vec<EvdevClient>>,
// }

// trait InputHandler {
//     /// Handle single event
//     fn handle_event(&self, event: &InputEvent);

//     /// Handle multiple events
//     fn handle_events(&self, events: &[InputEvent]) {
//         let clients = self.clients.read();
//         for client in clients {
//             client.enqueue_event(event);
//         }
//     }
// }
