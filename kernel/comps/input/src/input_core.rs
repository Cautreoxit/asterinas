// SPDX-License-Identifier: MPL-2.0

use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use core::{
    any::Any,
    sync::atomic::{AtomicU32, Ordering},
};
use log::debug;
use log::error;

use ostd::sync::SpinLock;
use alloc::format;
use alloc::boxed::Box;

use crate::{InputDevice, InputEvent, InputHandler};

/// Input handle - connects device to handler (equivalent to Linux struct input_handle)
pub struct InputHandle {
    /// Connected device (equivalent to input_handle->dev)
    pub dev: Arc<dyn InputDevice>,

    /// Connected handler (equivalent to input_handle->handler)
    pub handler: Arc<dyn InputHandler>,

    /// Handle name (equivalent to input_handle->name)
    pub name: String,

    /// Open count (equivalent to input_handle->open)
    pub open_count: AtomicU32,

    /// Private handler data (equivalent to input_handle->private)
    private: SpinLock<Option<Box<dyn Any + Send + Sync>>>,
}

impl InputHandle {
    pub fn new(dev: Arc<dyn InputDevice>, handler: Arc<dyn InputHandler>, name: String) -> Self {
        Self {
            dev,
            handler,
            name,
            open_count: AtomicU32::new(0),
            private: SpinLock::new(None),
        }
    }

    /// Check if handle is open
    pub fn is_open(&self) -> bool {
        // error!("--------------This is is_open--------------");
        self.open_count.load(Ordering::Relaxed) > 0
    }

    /// Set private data
    pub fn set_private<T: Any + Send + Sync>(&self, data: T) {
        *self.private.lock() = Some(Box::new(data));
    }

    /// Get private data with closure for safe access
    pub fn with_private<T: Any + Send + Sync, R, F>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&T) -> R,
    {
        let guard = self.private.lock();
        if let Some(boxed_any) = guard.as_ref() {
            if let Some(data) = boxed_any.downcast_ref::<T>() {
                Some(f(data))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get a mutable reference to private data with closure
    pub fn with_private_mut<T: Any + Send + Sync, R, F>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut T) -> R,
    {
        let mut guard = self.private.lock();
        if let Some(boxed_any) = guard.as_mut() {
            if let Some(data) = boxed_any.downcast_mut::<T>() {
                Some(f(data))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Remove private data
    pub fn clear_private(&self) {
        *self.private.lock() = None;
    }
}

/// Input core state - manages the entire input subsystem
pub struct InputCore {
    /// List of registered input devices
    devices: SpinLock<Vec<Arc<dyn InputDevice>>>,

    /// List of registered input handlers 
    handlers: SpinLock<Vec<Arc<dyn InputHandler>>>,

    /// List of active handles
    handles: SpinLock<Vec<Arc<InputHandle>>>,

    /// Device ID counter for unique device identification
    next_dev_id: AtomicU32,
}

impl InputCore {
    pub fn new() -> Self {
        Self {
            devices: SpinLock::new(Vec::new()),
            handlers: SpinLock::new(Vec::new()),
            handles: SpinLock::new(Vec::new()),
            next_dev_id: AtomicU32::new(1),
        }
    }

    /// Register an input device.
    pub fn register_device(&self, device: Arc<dyn InputDevice>) -> Result<(), i32> {
        // Add device to registry
        self.devices.lock().push(device.clone());

        // Try to connect device to all compatible handlers
        let handlers = self.handlers.lock().clone();
        for handler in handlers {
            if handler.match_device(&device) {
                // Handler just indicates if it wants to connect
                if handler.connect(device.clone()).is_ok() {
                    // InputCore creates the handle internally
                    let handle_name = format!("{}-{}", handler.name(), device.name());
                    let handle = Arc::new(InputHandle::new(
                        device.clone(),
                        handler.clone(),
                        handle_name,
                    ));
                    self.handles.lock().push(handle);
                }
            }
        }

        Ok(())
    }

    /// Unregister an input device.
    pub fn unregister_device(&self, device: &Arc<dyn InputDevice>) -> Result<(), i32> {
        // Remove device from registry
        self.devices.lock().retain(|d| !Arc::ptr_eq(d, device));

        // Disconnect all handles for this device
        let mut handles = self.handles.lock();
        handles.retain(|handle| {
            if Arc::ptr_eq(&handle.dev, device) {
                let _ = handle.handler.disconnect(device);
                false
            } else {
                true
            }
        });

        Ok(())
    }

    /// Register an input handler.
    pub fn register_handler(&self, handler: Arc<dyn InputHandler>) -> Result<(), i32> {
        // Add handler to registry
        self.handlers.lock().push(handler.clone());

        // Try to connect handler to all compatible devices
        let devices = self.devices.lock().clone();
        for device in devices {
            if handler.match_device(&device) {
                // Handler just indicates if it wants to connect
                if handler.connect(device.clone()).is_ok() {
                    // InputCore creates the handle internally
                    let handle_name = format!("{}-{}", handler.name(), device.name());
                    let handle = Arc::new(InputHandle::new(
                        device,
                        handler.clone(),
                        handle_name,
                    ));
                    self.handles.lock().push(handle);
                }
            }
        }

        Ok(())
    }

    /// Unregister an input handler.
    pub fn unregister_handler(&self, handler: &Arc<dyn InputHandler>) -> Result<(), i32> {
        // Remove handler from registry
        self.handlers.lock().retain(|h| !Arc::ptr_eq(h, handler));

        // Disconnect all handles for this handler
        let mut handles = self.handles.lock();
        handles.retain(|handle| {
            if Arc::ptr_eq(&handle.handler, handler) {
                let _ = handle.handler.disconnect(&handle.dev);
                false
            } else {
                true
            }
        });

        Ok(())
    }

    /// Pass an event to all handlers connected to this device.
    pub fn submit_event(&self, dev: &Arc<dyn InputDevice>, event: &InputEvent) {
        let handles = self.handles.lock();
        
        for handle in handles.iter() {
            let name_match = handle.dev.name() == dev.name();
            let ptr_match = Arc::ptr_eq(&handle.dev, dev);
            if (name_match || ptr_match) && handle.is_open() {
                handle.handler.event(&handle.dev, event);
            }
        }
    }

    /// Open an input device.
    pub fn open_device(&self, handle: &InputHandle) -> Result<(), i32> {
        let prev_count = handle.open_count.fetch_add(1, Ordering::Relaxed);
        if prev_count == 0 {
            handle.dev.open()?;
        }
        Ok(())
    }

    /// Close an input device.
    pub fn close_device(&self, handle: &InputHandle) -> Result<(), i32> {
        let prev_count = handle.open_count.fetch_sub(1, Ordering::Relaxed);
        if prev_count == 1 {
            handle.dev.close()?;
        }
        Ok(())
    }

    /// Get a list of all registered devices.
    pub fn list_devices(&self) -> Vec<Arc<dyn InputDevice>> {
        self.devices.lock().clone()
    }

    /// Get a list of all registered handlers.
    pub fn list_handlers(&self) -> Vec<Arc<dyn InputHandler>> {
        self.handlers.lock().clone()
    }

    /// Get a list of all active handles.
    pub fn list_handles(&self) -> Vec<Arc<InputHandle>> {
        self.handles.lock().clone()
    }
}
