// SPDX-License-Identifier: MPL-2.0

//! The framebuffer of Asterinas.
#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use alloc::{sync::Arc, vec::Vec};
use core::{
    fmt::{self, Debug},
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
    ops::Deref,
};

use aster_console::{AnyConsoleDevice, ConsoleCallback};
use aster_keyboard::InputKey;
use component::{init_component, ComponentInitError};

use font8x8::UnicodeFonts;
use ostd::{
    boot::boot_info,
    io::IoMem,
    mm::{VmIo, VmReader},
    sync::{PreemptDisabled, SpinLock, SpinLockGuard}, task::scheduler::info,
};

use spin::Once;

#[init_component]
fn init() -> Result<(), ComponentInitError> {
    log::error!("This is init in kernel/comps/framebuffer/src/lib.rs");
    framebuffer_init();
    framebuffer_console_init();
    Ok(())
}

/// The framebuffer used for text or graphical output.
#[derive(Debug)]
pub struct FrameBuffer {
    io_mem: IoMem,
    frame: Vec<u8>,
    base: usize,
    width: usize,
    height: usize,
    bytes_per_pixel: usize,
}

/// A text console rendered onto the framebuffer.
pub struct FramebufferConsole {
    callbacks: SpinLock<Vec<&'static ConsoleCallback>>,
    x_pos: AtomicUsize,
    y_pos: AtomicUsize,
    fg_color: AtomicU32,
    bg_color: AtomicU32,
}

pub static FRAMEBUFFER: Once<SpinLock<FrameBuffer>> = Once::new();

pub fn get_framebuffer_info() -> Option<SpinLockGuard<'static, FrameBuffer, PreemptDisabled>> {
    FRAMEBUFFER.get().map(|fb| fb.lock())
}

fn framebuffer_init() {
    let Some(framebuffer_arg) = boot_info().framebuffer_arg else {
        log::error!("Framebuffer not found");
        return;
    };

    if framebuffer_arg.address == 0 {
        log::error!("Framebuffer address is zero");
        return;
    }

    let mut framebuffer = {
        let fb_base = framebuffer_arg.address;
        let fb_size = framebuffer_arg.width * framebuffer_arg.height * (framebuffer_arg.bpp / 8);
        let io_mem = IoMem::acquire(fb_base..fb_base + fb_size).unwrap();
        let frame = alloc::vec![0; fb_size];
        FrameBuffer {
            io_mem,
            frame,
            base: framebuffer_arg.address,
            width: framebuffer_arg.width,
            height: framebuffer_arg.height,
            bytes_per_pixel: framebuffer_arg.bpp / 8,
        }
    };

    framebuffer.clear();
    FRAMEBUFFER.call_once(|| SpinLock::new(framebuffer));
    aster_keyboard::register_callback(&handle_keyboard_input);
    aster_mouse::register_callback(&handle_mouse_input);
    log::error!("This is the end of framebuffer_init in kernel/comps/framebuffer/src/lib.rs");
}

impl FrameBuffer {
    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn io_mem_base(&self) -> usize {
        self.base
    }

    /// Returns the resolution in pixels.
    pub fn resolution(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Returns the number of bytes per pixel (color depth).
    pub fn bytes_per_pixel(&self) -> usize {
        self.bytes_per_pixel
    }

    /// Writes a pixel at the specified position with the given color.
    ///
    /// The `color` is expected to be in RGBA format.
    pub fn write_pixel_at(&mut self, x: usize, y: usize, color: u32) {
        let pixel_offset = (y * self.width + x) * self.bytes_per_pixel;
        // Convert the RGBA color to bytes in big-endian format
        let color_bytes = color.to_be_bytes(); // Big-Endian order: [R, G, B, A]
        match self.bytes_per_pixel {
            // Grayscale format: single luminance value (8 bits)
            1 => {
                // Extract the R, G, B components directly from color_bytes
                let r = color_bytes[0] as u32; // Red
                let g = color_bytes[1] as u32; // Green
                let b = color_bytes[2] as u32; // Blue

                // Integer-based weights scaled by 256
                let red_weight = 77; // Equivalent to 0.299 * 256
                let green_weight = 150; // Equivalent to 0.587 * 256
                let blue_weight = 29; // Equivalent to 0.114 * 256

                // Calculate the grayscale value
                let grayscale = (r * red_weight + g * green_weight + b * blue_weight) >> 8;
                self.frame[pixel_offset] = grayscale as u8;
            }
            // RGB565 format: 5 bits for Red, 6 bits for Green, 5 bits for Blue
            2 => {
                let r = (color_bytes[0] >> 3) as u16; // Red (5 bits)
                let g = (color_bytes[1] >> 2) as u16; // Green (6 bits)
                let b = (color_bytes[2] >> 3) as u16; // Blue (5 bits)
                                                      // Combine into RGB565 format
                let rgb565 = (r << 11) | (g << 5) | b;
                self.frame[pixel_offset..(pixel_offset + 2)].copy_from_slice(&rgb565.to_be_bytes());
            }
            // RGB888 format: 8 bits for Red, Green, and Blue
            3 => {
                self.frame[pixel_offset..(pixel_offset + 3)].copy_from_slice(&color_bytes[..3]);
            }
            // RGBA format: 8 bits for Red, Green, Blue, and Alpha
            4 => {
                self.frame[pixel_offset..(pixel_offset + 4)].copy_from_slice(&color_bytes[..4]);
            }
            _ => panic!("unsupported bit depth"),
        }
        self.io_mem
            .write_bytes(
                pixel_offset,
                &self.frame[pixel_offset..(pixel_offset + self.bytes_per_pixel)],
            )
            .unwrap();
    }

    /// Returns the framebuffer as an immutable byte slice.
    pub fn as_bytes(&self) -> &[u8] {
        &self.frame
    }

    /// Returns the framebuffer as a mutable byte slice.
    pub fn as_mut_bytes(&mut self) -> &mut [u8] {
        &mut self.frame
    }

    /// Flushes the framebuffer.
    pub fn flush_frame(&mut self) {
        self.io_mem.write_bytes(0, self.frame.as_slice()).unwrap();
    }

    /// Clears the framebuffer with default color (black).
    pub fn clear(&mut self) {
        self.frame.fill(0);
        self.flush_frame();
    } 
}

pub static CONSOLE_NAME: &str = "Framebuffer-Console";

pub static FRAMEBUFFER_CONSOLE: Once<Arc<FramebufferConsole>> = Once::new();

fn framebuffer_console_init() {
    if FRAMEBUFFER.get().is_none() {
        log::warn!("Framebuffer not initialized");
        return;
    }
    FRAMEBUFFER_CONSOLE.call_once(|| Arc::new(FramebufferConsole::new()));
}

impl AnyConsoleDevice for FramebufferConsole {
    fn send(&self, buf: &[u8]) {
        let mut fb = FRAMEBUFFER.get().unwrap().disable_irq().lock();
        self.send_buf(buf, &mut fb);
    }

    fn register_callback(&self, callback: &'static ConsoleCallback) {
        self.callbacks.disable_irq().lock().push(callback);
    }
}

impl FramebufferConsole {
    fn new() -> Self {
        Self {
            callbacks: SpinLock::new(Vec::new()),
            x_pos: AtomicUsize::new(0),
            y_pos: AtomicUsize::new(0),
            fg_color: AtomicU32::new(0xFFFFFFFF), // Default foreground color (white)
            bg_color: AtomicU32::new(0x00000000), // Default background color (black)
        }
    }

    /// Returns the current cursor position.
    pub fn cursor(&self) -> (usize, usize) {
        let x = self.x_pos.load(Ordering::Relaxed);
        let y = self.y_pos.load(Ordering::Relaxed);
        (x, y)
    }

    /// Sets the cursor position.
    pub fn set_cursor(&self, x: usize, y: usize) {
        self.x_pos.store(x, Ordering::Relaxed);
        self.y_pos.store(y, Ordering::Relaxed);
    }

    /// Returns the foreground color.
    pub fn fg_color(&self) -> u32 {
        self.fg_color.load(Ordering::Relaxed)
    }

    /// Sets the foreground color.
    pub fn set_fg_color(&self, color: u32) {
        self.fg_color.store(color, Ordering::Relaxed);
    }

    /// Returns the background color.
    pub fn bg_color(&self) -> u32 {
        self.bg_color.load(Ordering::Relaxed)
    }

    /// Sets the background color.
    pub fn set_bg_color(&self, color: u32) {
        self.bg_color.store(color, Ordering::Relaxed);
    }

    fn newline(&self, fb: &mut FrameBuffer) {
        let mut y = self.y_pos.load(Ordering::Relaxed);
        if y >= fb.height - 8 {
            self.shift_lines_up(fb);
            y -= 8;
        }
        self.y_pos.store(y + 8, Ordering::Relaxed);
        self.carriage_return();
    }

    fn carriage_return(&self) {
        self.x_pos.store(0, Ordering::Relaxed);
    }

    fn shift_lines_up(&self, fb: &mut FrameBuffer) {
        let offset = fb.width * fb.bytes_per_pixel * 8;
        let frame = fb.as_mut_bytes();
        let frame_len = frame.len();
        frame.copy_within(offset.., 0);
        frame[frame_len - offset..].fill(0);
        fb.flush_frame();
    }

    /// Sends a single character to be drawn on the framebuffer.
    pub fn send_char(&self, c: char, fb: &mut FrameBuffer) {
        if c == '\n' {
            self.newline(fb);
            return;
        } else if c == '\r' {
            self.carriage_return();
            return;
        }

        if self.x_pos.load(Ordering::Relaxed) >= fb.width {
            self.newline(fb);
        }

        let (x_pos, y_pos) = self.cursor();
        let fg_color = self.fg_color();
        let bg_color = self.bg_color();
        let rendered = font8x8::BASIC_FONTS
            .get(c)
            .expect("character not found in basic font");
        for (y, byte) in rendered.iter().enumerate() {
            for (x, bit) in (0..8).enumerate() {
                let on = *byte & (1 << bit) != 0;
                let color = if on { fg_color } else { bg_color };
                fb.write_pixel_at(x_pos + x, y_pos + y, color);
            }
        }
        self.x_pos.store(x_pos + 8, Ordering::Relaxed);
    }

    /// Sends a buffer of bytes to be drawn on the framebuffer.
    ///
    /// # Panics
    ///
    /// This method will panic if any byte in the buffer cannot be converted
    /// into a valid Unicode character.
    pub fn send_buf(&self, buf: &[u8], fb: &mut FrameBuffer) {
        // TODO: handle ANSI escape sequences.
        for &ch in buf.iter() {
            if ch != 0 {
                let char = char::from_u32(ch as u32).unwrap();
                self.send_char(char, fb);
            }
        }
    }
}

impl Debug for FramebufferConsole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FramebufferConsole")
            .field("(x_pos, y_pos)", &self.cursor())
            .field("fg_color", &self.fg_color())
            .field("bg_color", &self.bg_color())
            .finish()
    }
}

fn handle_keyboard_input(key: InputKey) {
    if key == InputKey::Nul {
        return;
    }

    let Some(console) = FRAMEBUFFER_CONSOLE.get() else {
        return;
    };

    let callbacks = &console.callbacks;

    let buffer = key.deref();
    for callback in callbacks.disable_irq().lock().iter() {
        let reader = VmReader::from(buffer);
        callback(reader);
    }
}

// TODO: framebuffer using mouse input
fn handle_mouse_input() {
    log::error!("End: This is in kernel/comps/framebuffer/src/lib.rs!");
}