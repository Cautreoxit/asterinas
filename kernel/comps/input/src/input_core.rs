// SPDX-License-Identifier: MPL-2.0

use alloc::{sync::{Arc, Weak}, vec::Vec};
use ostd::sync::RwLock;

use crate::{InputDevice, InputHandlerClass, InputHandler, InputEvent};
use core::fmt::Debug;


/// Registry entry for each registered device
/// 
/// This serves as the connection point between devices and their handlers.
#[derive(Debug)]
pub struct InputDeviceRegistry {
    /// The input device
    pub device: Arc<dyn InputDevice>,
    /// Handlers connected to this device (shared with RegisteredInputDevice)
    pub handlers: Arc<RwLock<Vec<Arc<dyn InputHandler>>>>,
}

/// Type-safe wrapper ensuring only registered devices can submit events.
pub struct RegisteredInputDevice {
    /// Original device
    device: Arc<dyn InputDevice>,
    /// Reference to handlers for direct event dispatch
    handlers: Arc<RwLock<Vec<Arc<dyn InputHandler>>>>,
}

impl RegisteredInputDevice {
    pub fn new(device: Arc<dyn InputDevice>, handlers: Arc<RwLock<Vec<Arc<dyn InputHandler>>>>) -> Self {
        Self {
            device,
            handlers,
        }
    }

    /// Submit a single event to all handlers efficiently
    pub fn submit_event(&self, event: &InputEvent) {
        let handlers = self.handlers.read();
        if handlers.is_empty() {
            log::error!("No handlers for device: {}, event dropped!", self.device.name());
            return;
        }
        
        for handler in handlers.iter() {
            handler.handle_event(event);
        }
    }

    /// Submit multiple events in batch
    pub fn submit_events(&self, events: &[InputEvent]) {
        let handlers = self.handlers.read();
        if handlers.is_empty() {
            log::error!("No handlers for device: {}, event dropped!", self.device.name());
            return;
        }
        
        for handler in handlers.iter() {
            handler.handle_events(events);
        }
    }

    /// Get the underlying device reference
    pub fn device(&self) -> &Arc<dyn InputDevice> {
        &self.device
    }

    /// Get the number of connected handlers
    pub fn handler_count(&self) -> usize {
        self.handlers.read().len()
    }
    
    // /// Check if the device is still registered and valid for event submission
    // pub fn is_valid(&self) -> bool {
    //     !self.handlers.read().is_empty()
    // }
}

impl Drop for RegisteredInputDevice {
    fn drop(&mut self) {
        log::info!("Unregistering input device: {}", self.device.name());
    }
}

#[derive(Debug)]
pub struct InputCore {
    /// All registered devices with their handlers
    devices: RwLock<Vec<InputDeviceRegistry>>,
    /// All registered handler classes
    handler_classes: RwLock<Vec<Arc<dyn InputHandlerClass>>>,
}

impl InputCore {
    /// Create a new input core
    pub fn new() -> Self {
        Self {
            devices: RwLock::new(Vec::new()),
            handler_classes: RwLock::new(Vec::new()),
        }
    }

    /// Register a new handler class
    pub fn register_handler_class(
        &self,
        handler_class: Arc<dyn InputHandlerClass>,
    ) -> Result<(), i32> {
        // Add to handler classes registry
        self.handler_classes.write().push(handler_class.clone());
        
        // Connect to all existing devices
        let devices = self.devices.read();
        for device_registry in devices.iter() {
            if let Ok(handler) = handler_class.connect(device_registry.device.clone()) {
                device_registry.handlers.write().push(handler);
            }
        }
        
        log::info!("Registered handler class: {}", handler_class.name());
        Ok(())
    }

    /// Unregister a handler class
    pub fn unregister_handler_class(
        &self,
        handler_class: &Arc<dyn InputHandlerClass>,
    ) -> Result<(), i32> {
        let class_name = handler_class.name();
        
        // Remove from handler classes
        self.handler_classes.write().retain(|h| h.name() != class_name);
        
        // Disconnect from all devices and remove handlers
        let devices = self.devices.read();
        for device_registry in devices.iter() {
            // Notify handler class about disconnection
            let _ = handler_class.disconnect(&device_registry.device);
            
            // Remove handlers belonging to this class
            device_registry.handlers.write().retain(|h| h.class_name() != class_name);
        }
        
        log::info!("Unregistered handler class: {}", handler_class.name());
        Ok(())
    }

    /// Register a new input device and return type-safe wrapper
    pub fn register_device(
        &self,
        device: Arc<dyn InputDevice>,
    ) -> Result<RegisteredInputDevice, i32> {        
        // Connect all existing handler classes
        let handler_classes = self.handler_classes.read();
        let mut connected_handlers = Vec::new();
        
        for handler_class in handler_classes.iter() {
            if let Ok(handler) = handler_class.connect(device.clone()) {
                connected_handlers.push(handler);
            }
        }
        
        let handlers = Arc::new(RwLock::new(connected_handlers));
        
        let new_registry = InputDeviceRegistry {
            device: device.clone(),
            handlers: handlers.clone(),
        };
        
        // Add to devices registry
        self.devices.write().push(new_registry);
        
        Ok(RegisteredInputDevice::new(device, handlers))
    }

    /// Unregister an input device
    pub fn unregister_device(&self, device: &Arc<dyn InputDevice>) -> Result<(), i32> {
        let mut devices = self.devices.write();
        
        // Find the device to remove
        if let Some(pos) = devices.iter().position(|registry| Arc::ptr_eq(&registry.device, device)) {
            let device_registry = devices.remove(pos);
            
            // Clear handlers to invalidate any existing RegisteredInputDevice instances
            device_registry.handlers.write().clear();
            
            // Disconnect all handler classes from this device
            let handler_classes = self.handler_classes.read();
            for handler_class in handler_classes.iter() {
                let _ = handler_class.disconnect(&device_registry.device);
            }
            
            log::info!("Unregistered input device: {}", device.name());
            Ok(())
        } else {
            log::warn!("Device {} not found in registry, cannot unregister", device.name());
            Err(-22)  //EINVAL
        }
    }

    /// Get device count
    pub fn device_count(&self) -> usize {
        self.devices.read().len()
    }

    /// Get handler class count
    pub fn handler_class_count(&self) -> usize {
        self.handler_classes.read().len()
    }

    /// Get all registered devices
    pub fn all_devices(&self) -> Vec<Arc<dyn InputDevice>> {
        let devices = self.devices.read();
        devices.iter().map(|registry| registry.device.clone()).collect()
    }
}
