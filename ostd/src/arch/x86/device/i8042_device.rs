// SPDX-License-Identifier: MPL-2.0

//! Provides i8042 PC keyboard I/O port access.

use spin::Once;
use core::hint::spin_loop;

use crate::{
    arch::x86::{
        device::io_port::{IoPort, ReadWriteAccess},
        kernel::{pic, IO_APIC},
    },
    sync::SpinLock,
    trap::{IrqLine, TrapFrame},
};

/// Data register (R/W)
pub static DATA_PORT: IoPort<u8, ReadWriteAccess> = unsafe { IoPort::new(0x60) };

/// Status register (R)
pub static STATUS_PORT: IoPort<u8, ReadWriteAccess> = unsafe { IoPort::new(0x64) };

/// IrqLine for i8042 keyboard.
static KEYBOARD_IRQ_LINE: Once<SpinLock<IrqLine>> = Once::new();

/// IrqLine for i8042 mouse.
static MOUSE_IRQ_LINE: Once<SpinLock<IrqLine>> = Once::new();

// Controller commands
const DISABLE_MOUSE: u8 = 0xA7;
const ENABLE_MOUSE: u8 = 0xA8;
const DISABLE_KEYBOARD: u8 = 0xAD;
const ENABLE_KEYBOARD: u8 = 0xAE;
const MOUSE_WRITE: u8 = 0xD4;
const READ_CONFIG: u8 = 0x20;
const WRITE_CONFIG: u8 = 0x60;

// Mouse commands
const MOUSE_ENABLE: u8 = 0xF4;
const MOUSE_RESET: u8 = 0xFF;
const MOUSE_DEFAULT: u8 = 0xF6;

// Configure bits
const ENABLE_KEYBOARD_BIT: u8 = 0x1;
const ENABLE_MOUSE_BIT: u8 = 0x2;
const ENABLE_MOUSE_CLOCK_BIT: u8 = 0x20;

pub(crate) fn init() {
    init_i8042_controller();

    if !IO_APIC.is_completed() {
        let k_irq = pic::allocate_irq(1).unwrap();
        let m_irq = pic::allocate_irq(12).unwrap();
        KEYBOARD_IRQ_LINE.call_once(|| SpinLock::new(k_irq));
        MOUSE_IRQ_LINE.call_once(|| SpinLock::new(m_irq));
    } else {
        let k_irq = IrqLine::alloc().unwrap();
        let m_irq = IrqLine::alloc().unwrap();

        let mut io_apic = IO_APIC.get().unwrap().first().unwrap().lock();
        io_apic.enable(1, k_irq.clone()).unwrap();
        io_apic.enable(12, m_irq.clone()).unwrap();

        KEYBOARD_IRQ_LINE.call_once(|| SpinLock::new(k_irq));
        MOUSE_IRQ_LINE.call_once(|| SpinLock::new(m_irq));
    }

    init_mouse_device();

}

/// Registers a callback function to be called when there is keyboard input.
pub fn register_keyboard_callback<F>(callback: F)
where
    F: Fn(&TrapFrame) + Sync + Send + 'static,
{
    let Some(irq) = KEYBOARD_IRQ_LINE.get() else {
        return;
    };

    irq.disable_irq().lock().on_active(callback);
}


/// Registers a callback function to be called when there is mouse input.
pub fn register_mouse_callback<F>(callback: F)
where
    F: Fn(&TrapFrame) + Sync + Send + 'static,
{
    let Some(irq) = MOUSE_IRQ_LINE.get() else {
        return;
    };

    irq.disable_irq().lock().on_active(callback);
}

/// Initialize i8042 controller
fn init_i8042_controller() {
    // Disable keyborad and mouse
    STATUS_PORT.write(DISABLE_MOUSE);
    STATUS_PORT.write(DISABLE_KEYBOARD);

    // Clear the input buffer
    while DATA_PORT.read() & 0x1 != 0 {
        let _ = DATA_PORT.read();
    }

    // Set up the configuration
    STATUS_PORT.write(READ_CONFIG); 
    let mut config = DATA_PORT.read();
    config |= ENABLE_KEYBOARD_BIT; 
    config |= ENABLE_MOUSE_BIT; 
    config &= !ENABLE_MOUSE_CLOCK_BIT;

    STATUS_PORT.write(WRITE_CONFIG);
    DATA_PORT.write(config);

    // Enable keyboard and mouse
    STATUS_PORT.write(ENABLE_KEYBOARD);
    STATUS_PORT.write(ENABLE_MOUSE);
}

/// Initialize i8042 mouse
fn init_mouse_device() {
    // Send reset command
    STATUS_PORT.write(MOUSE_WRITE);
    DATA_PORT.write(MOUSE_RESET);
    wait_ack();

    // Set up default configuration
    STATUS_PORT.write(MOUSE_WRITE);
    DATA_PORT.write(MOUSE_DEFAULT);
    wait_ack();

    // Enable data reporting
    STATUS_PORT.write(MOUSE_WRITE);
    DATA_PORT.write(MOUSE_ENABLE);
    wait_ack();
}

/// Wait for controller's acknowledgement
fn wait_ack() {
    loop {
        if STATUS_PORT.read() & 0x1 != 0 {
            let data = DATA_PORT.read();
            if data == 0xFA {
                return 
            }
        }
        spin_loop();
    }
}
