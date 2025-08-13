// SPDX-License-Identifier: MPL-2.0

use alloc::vec::Vec;
use core::{any::Any, fmt::Debug};

use ostd::Pod;

use crate::event_type_codes::{EventTypeFlags, KeyEventMap, RelEventMap};

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
#[derive(Debug, Clone)]
pub struct InputCapability {
    /// Supported event types (EV_KEY, EV_REL, etc.)
    pub evbit: EventTypeFlags,
    /// Supported key/button codes
    pub keybit: KeyEventMap,
    /// Supported relative axis codes
    pub relbit: RelEventMap,
    // TODO: Add absbit, mscbit, ledbit, sndbit, ffbit, swbit
}

impl InputCapability {
    pub fn new() -> Self {
        Self {
            evbit: EventTypeFlags::new(),
            keybit: KeyEventMap::new(),
            relbit: RelEventMap::new(),
        }
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
    pub fn set_evbit(&mut self, event_type: crate::event_type_codes::EventType) {
        self.evbit.add_event_type(event_type);
    }

    /// Set key capability
    pub fn set_keybit(&mut self, key_event: crate::event_type_codes::KeyEvent) {
        self.keybit.set(key_event);
        self.set_evbit(crate::event_type_codes::EventType::EvKey);
    }

    /// Check if a key event is supported
    pub fn has_key(&self, key_event: crate::event_type_codes::KeyEvent) -> bool {
        self.keybit.contains(key_event)
    }

    /// Clear a key capability
    pub fn clear_keybit(&mut self, key_event: crate::event_type_codes::KeyEvent) {
        self.keybit.clear(key_event);
    }

    /// Set relative axis capability
    pub fn set_relbit(&mut self, rel_event: crate::event_type_codes::RelEvent) {
        self.relbit.set(rel_event);
        self.set_evbit(crate::event_type_codes::EventType::EvRel);
    }

    /// Check if a relative event is supported
    pub fn has_rel(&self, rel_event: crate::event_type_codes::RelEvent) -> bool {
        self.relbit.contains(rel_event)
    }

    /// Clear a relative capability
    pub fn clear_relbit(&mut self, rel_event: crate::event_type_codes::RelEvent) {
        self.relbit.clear(rel_event);
    }

    /// Check if an event type is supported
    pub fn supports_event_type(&self, event_type: crate::event_type_codes::EventType) -> bool {
        self.evbit.supports_event_type(event_type)
    }

    /// Remove support for an event type
    pub fn clear_evbit(&mut self, event_type: crate::event_type_codes::EventType) {
        self.evbit.remove_event_type(event_type);
    }

    /// Get all supported event types
    pub fn get_event_types(&self) -> EventTypeFlags {
        self.evbit
    }
}

pub trait InputDevice: Send + Sync + Any + Debug {
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
}
