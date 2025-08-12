// SPDX-License-Identifier: MPL-2.0

use alloc::{string::String, vec::Vec};
use core::any::Any;

use ostd::Pod;

/// Input device identification - equivalent to Linux struct input_id
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct InputId {
    pub bustype: u16, // Bus type
    pub vendor: u16,  // Vendor ID
    pub product: u16, // Product ID
    pub version: u16, // Version number
}

impl InputId {
    pub fn new(bustype: u16, vendor: u16, product: u16, version: u16) -> Self {
        Self {
            bustype,
            vendor,
            product,
            version,
        }
    }

    /// Common bus types
    pub const BUS_PCI: u16 = 0x01;
    pub const BUS_ISAPNP: u16 = 0x02;
    pub const BUS_USB: u16 = 0x03;
    pub const BUS_HIL: u16 = 0x04;
    pub const BUS_BLUETOOTH: u16 = 0x05;
    pub const BUS_VIRTUAL: u16 = 0x06;
    pub const BUS_ISA: u16 = 0x10;
    pub const BUS_I8042: u16 = 0x11;
    pub const BUS_XTKBD: u16 = 0x12;
    pub const BUS_RS232: u16 = 0x13;
    pub const BUS_GAMEPORT: u16 = 0x14;
    pub const BUS_PARPORT: u16 = 0x15;
    pub const BUS_AMIGA: u16 = 0x16;
    pub const BUS_ADB: u16 = 0x17;
    pub const BUS_I2C: u16 = 0x18;
    pub const BUS_HOST: u16 = 0x19;
    pub const BUS_GSC: u16 = 0x1A;
    pub const BUS_ATARI: u16 = 0x1B;
    pub const BUS_SPI: u16 = 0x1C;
    pub const BUS_RMI: u16 = 0x1D;
    pub const BUS_CEC: u16 = 0x1E;
    pub const BUS_INTEL_ISHTP: u16 = 0x1F;
}

/// Input device capability bitmaps.
#[derive(Debug, Clone, Default)]
pub struct InputCapability {
    /// Supported event types (EV_KEY, EV_REL, etc.)
    pub evbit: Vec<u16>,
    /// Supported key/button codes
    pub keybit: Vec<u16>,
    /// Supported relative axis codes
    pub relbit: Vec<u16>,
    /// Supported absolute axis codes
    pub absbit: Vec<u16>,
    /// Supported MSC events
    pub mscbit: Vec<u16>,
    /// Supported LED events
    pub ledbit: Vec<u16>,
    /// Supported sound events
    pub sndbit: Vec<u16>,
    /// Supported force feedback events
    pub ffbit: Vec<u16>,
    /// Supported switch events
    pub swbit: Vec<u16>,
}

impl InputCapability {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a capability bit
    pub fn set_bit(bitmap: &mut Vec<u16>, bit: u16) {
        if !bitmap.contains(&bit) {
            bitmap.push(bit);
        }
    }

    /// Test if a capability bit is set
    pub fn test_bit(bitmap: &[u16], bit: u16) -> bool {
        bitmap.contains(&bit)
    }

    /// Set event type capability
    pub fn set_evbit(&mut self, ev_type: u16) {
        Self::set_bit(&mut self.evbit, ev_type);
    }

    /// Set key capability
    pub fn set_keybit(&mut self, key_code: u16) {
        Self::set_bit(&mut self.keybit, key_code);
        self.set_evbit(crate::event_type_codes::EventType::EvKey as u16);
    }

    /// Set relative axis capability
    pub fn set_relbit(&mut self, rel_code: u16) {
        Self::set_bit(&mut self.relbit, rel_code);
        self.set_evbit(crate::event_type_codes::EventType::EvRel as u16);
    }

    /// Set absolute axis capability
    pub fn set_absbit(&mut self, abs_code: u16) {
        Self::set_bit(&mut self.absbit, abs_code);
        self.set_evbit(crate::event_type_codes::EventType::EvAbs as u16);
    }
}

/// Input device trait - equivalent to Linux struct input_dev
pub trait InputDevice: Send + Sync + Any {
    /// Device name
    fn name(&self) -> &str;

    /// Physical location
    fn phys(&self) -> &str;

    /// Unique identifier
    fn uniq(&self) -> &str;

    /// Device ID
    fn id(&self) -> InputId;

    /// Device capabilities
    fn capability(&self) -> &InputCapability;

    /// Open device
    fn open(&self) -> Result<(), i32> {
        Ok(())
    }

    /// Close device
    fn close(&self) -> Result<(), i32> {
        Ok(())
    }

    /// Handle events sent TO the device
    fn event(&self, _type_: u16, _code: u16, _value: i32) -> Result<(), i32> {
        Ok(())
    }

    /// Flush device
    fn flush(&self) -> Result<(), i32> {
        Ok(())
    }

    /// Get keycode (equivalent to input_dev->getkeycode)
    fn getkeycode(&self, _scancode: u32) -> Result<u32, i32> {
        Err(-1) // Not implemented by default
    }

    /// Set keycode (equivalent to input_dev->setkeycode)
    fn setkeycode(&self, _scancode: u32, _keycode: u32) -> Result<u32, i32> {
        Err(-1) // Not implemented by default
    }
}
