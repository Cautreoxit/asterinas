// SPDX-License-Identifier: MPL-2.0

#![expect(unused_variables)]

use align_ext::AlignExt;
pub(crate) use aster_framebuffer::{get_framebuffer_info, FrameBufferBitfield};
use ostd::{
    boot::boot_info,
    io::IoMem,
    mm::{CachePolicy, Frame, PageFlags, PageProperty, UFrame},
    task::disable_preempt,
    Pod,
};

use super::*;
use crate::{
    current_userspace,
    events::IoEvents,
    fs::{inode_handle::FileIo, utils::IoctlCmd},
    prelude::*,
    process::signal::{PollHandle, Pollable},
    vm::perms::VmPerms,
};

pub struct Fb;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct FbVarScreenInfo {
    pub xres: u32, // Visible resolution
    pub yres: u32,
    pub xres_virtual: u32, // Virtual resolution
    pub yres_virtual: u32,
    pub xoffset: u32, // Offset from virtual to visible
    pub yoffset: u32,
    pub bits_per_pixel: u32, // Guess what
    pub grayscale: u32,      // 0 = color, 1 = grayscale, >1 = FOURCC
    // Add other fields as needed
    pub red: FrameBufferBitfield, // Bitfield in framebuffer memory if true color
    pub green: FrameBufferBitfield, // Else only length is significant
    pub blue: FrameBufferBitfield,
    pub transp: FrameBufferBitfield, // Transparency
    pub nonstd: u32,                 // Non-standard pixel format
    pub activate: u32,               // See FB_ACTIVATE_*
    pub height: u32,                 // Height of picture in mm
    pub width: u32,                  // Width of picture in mm
    pub accel_flags: u32,            // (OBSOLETE) see fb_info.flags
    pub pixclock: u32,               // Pixel clock in ps (pico seconds)
    pub left_margin: u32,            // Time from sync to picture
    pub right_margin: u32,           // Time from picture to sync
    pub upper_margin: u32,           // Time from sync to picture
    pub lower_margin: u32,
    pub hsync_len: u32,     // Length of horizontal sync
    pub vsync_len: u32,     // Length of vertical sync
    pub sync: u32,          // See FB_SYNC_*
    pub vmode: u32,         // See FB_VMODE_*
    pub rotate: u32,        // Angle we rotate counter-clockwise
    pub colorspace: u32,    // Colorspace for FOURCC-based modes
    pub reserved: [u32; 4], // Reserved for future compatibility
}

impl Default for FbVarScreenInfo {
    fn default() -> Self {
        Self {
            xres: 0,
            yres: 0,
            xres_virtual: 0,
            yres_virtual: 0,
            xoffset: 0,
            yoffset: 0,
            bits_per_pixel: 0,
            grayscale: 0,
            red: FrameBufferBitfield::default(),
            green: FrameBufferBitfield::default(),
            blue: FrameBufferBitfield::default(),
            transp: FrameBufferBitfield::default(),
            nonstd: 0,
            activate: 0,
            height: 0,
            width: 0,
            accel_flags: 0,
            pixclock: 0,
            left_margin: 0,
            right_margin: 0,
            upper_margin: 0,
            lower_margin: 0,
            hsync_len: 0,
            vsync_len: 0,
            sync: 0,
            vmode: 0,
            rotate: 0,
            colorspace: 0,
            reserved: [0; 4],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct FbFixScreenInfo {
    pub id: [u8; 16],       // Identification string, e.g., "TT Builtin"
    pub smem_start: u64,    // Start of framebuffer memory (physical address)
    pub smem_len: u32,      // Length of framebuffer memory
    pub type_: u32,         // See FB_TYPE_*
    pub type_aux: u32,      // Interleave for interleaved planes
    pub visual: u32,        // See FB_VISUAL_*
    pub xpanstep: u16,      // Zero if no hardware panning
    pub ypanstep: u16,      // Zero if no hardware panning
    pub ywrapstep: u16,     // Zero if no hardware ywrap
    pub line_length: u32,   // Length of a line in bytes
    pub mmio_start: u64,    // Start of Memory Mapped I/O (physical address)
    pub mmio_len: u32,      // Length of Memory Mapped I/O
    pub accel: u32,         // Indicate to driver which specific chip/card we have
    pub capabilities: u16,  // See FB_CAP_*
    pub reserved: [u16; 2], // Reserved for future compatibility
}

impl Default for FbFixScreenInfo {
    fn default() -> Self {
        Self {
            id: [0; 16],
            smem_start: 0,
            smem_len: 0,
            type_: 0,
            type_aux: 0,
            visual: 0,
            xpanstep: 0,
            ypanstep: 0,
            ywrapstep: 0,
            line_length: 0,
            mmio_start: 0,
            mmio_len: 0,
            accel: 0,
            capabilities: 0,
            reserved: [0; 2],
        }
    }
}

impl Device for Fb {
    fn type_(&self) -> DeviceType {
        DeviceType::CharDevice
    }

    fn id(&self) -> DeviceId {
        // Same value with Linux
        DeviceId::new(29, 0)
    }

    fn open(&self) -> Result<Option<Arc<dyn FileIo>>> {
        Ok(Some(Arc::new(Fb)))
    }
}

impl Pollable for Fb {
    fn poll(&self, mask: IoEvents, poller: Option<&mut PollHandle>) -> IoEvents {
        let events = IoEvents::IN | IoEvents::OUT;
        events & mask
    }
}

/// Aligns the address down to the nearest lower multiple of `alignment`.
fn align_down(addr: usize, alignment: usize) -> usize {
    addr & !(alignment - 1)
}

/// Aligns the address up to the nearest higher multiple of `alignment`.
fn align_up(addr: usize, alignment: usize) -> usize {
    (addr + alignment - 1) & !(alignment - 1)
}

impl FileIo for Fb {
    fn read(&self, _writer: &mut VmWriter) -> Result<usize> {
        println!("Fb read");
        Ok(0)
    }

    fn write(&self, reader: &mut VmReader) -> Result<usize> {
        println!("Fb write");
        Ok(reader.remain())
    }

    // fn mmap(
    //     &self,
    //     addr: Vaddr,
    //     len: usize,
    //     offset: usize,
    //     perms: VmPerms,
    //     ctx: &Context,
    // ) -> Result<Vaddr> {
    //     println!("Fb mmap: Mapping framebuffer to user space");

    //     // Get the user space context
    //     let user_space: CurrentUserSpace<'_> = ctx.user_space();
    //     let root_vmar = user_space.root_vmar();
    //     println!(
    //         "start to get framebuffer info, addr {:X}, len {:X}, offset {:X}",
    //         addr, len, offset
    //     );

    //     if let Some(framebuffer) = get_framebuffer_info().as_deref() {
    //         println!("start to get vaddr_range");
    //         // Ensure the virtual address range is page-aligned
    //         let vaddr_range = if addr == 0 {
    //             root_vmar.vm_space().allocate_virtual_address_range(len)?
    //         } else {
    //             align_down(addr, PAGE_SIZE)..align_up(addr + len, PAGE_SIZE)
    //         };

    //         println!(
    //             "Got vaddr_range start: {:X}, end: {:X}",
    //             vaddr_range.start, vaddr_range.end
    //         );

    //         let preempt_guard = disable_preempt();

    //         // Create a mutable cursor for the virtual address range
    //         let mut cursor = root_vmar.vm_space().cursor_mut(&preempt_guard, &vaddr_range)?;
    //         println!("Got cursor");

    //         // Map the framebuffer memory to the user space
    //         cursor.map_iomem(
    //             framebuffer.io_mem(),
    //             PageProperty::new(PageFlags::from(perms), CachePolicy::Uncacheable),
    //         );

    //         Ok(vaddr_range.start) // Return the starting virtual address
    //     } else {
    //         None
    //     }
    // }

    fn get_io_mem(&self) -> Option<IoMem> {
        if let Some(framebuffer) = get_framebuffer_info() {
            let iomem = framebuffer.io_mem();
            Some(iomem.clone())
        } else {
            None
        }
    }

    fn ioctl(&self, cmd: IoctlCmd, arg: usize) -> Result<i32> {
        match cmd {
            IoctlCmd::GETVSCREENINFO => {
                println!("Fb ioctl: Get virtual screen info");

                // Use get_framebuffer_info to access the framebuffer
                if let Some(framebuffer_guard) = get_framebuffer_info() {
                    let framebuffer = &*framebuffer_guard; // Dereference the guard to access the FrameBuffer

                    // FIXME: On demand add more fields
                    let mut screen_info = FbVarScreenInfo::default();
                    screen_info.xres = framebuffer.width() as u32;
                    screen_info.yres = framebuffer.height() as u32;
                    screen_info.xres_virtual = framebuffer.width() as u32;
                    screen_info.yres_virtual = framebuffer.height() as u32;
                    screen_info.bits_per_pixel = (8 * framebuffer.bytes_per_pixel()) as u32;
                    screen_info.red = framebuffer.red();
                    screen_info.green = framebuffer.green();
                    screen_info.blue = framebuffer.blue();
                    screen_info.transp = framebuffer.reserved();

                    // Data are set according to the linux efifb driver
                    screen_info.pixclock =
                        10000000 / framebuffer.width() as u32 * 1000 / framebuffer.height() as u32;
                    screen_info.left_margin = framebuffer.width() as u32 / 8 & 0xf8;
                    screen_info.right_margin = 32;
                    screen_info.upper_margin = 16;
                    screen_info.lower_margin = 4;

                    screen_info.vsync_len = 4;
                    screen_info.hsync_len = framebuffer.width() as u32 / 8 & 0xf8;

                    current_userspace!().write_val(arg, &screen_info)?;

                    Ok(0)
                } else {
                    println!("Framebuffer is not initialized");
                    return_errno!(Errno::ENODEV); // No such device
                }
            }
            IoctlCmd::GETFSCREENINFO => {
                println!("Fb ioctl: Get fixed screen info");

                // Use get_framebuffer_info to access the framebuffer
                if let Some(framebuffer_guard) = get_framebuffer_info() {
                    let framebuffer = &*framebuffer_guard;

                    // FIXME: On demand add more fields
                    let mut screen_info = FbFixScreenInfo::default();
                    screen_info.smem_start = framebuffer.io_mem_base() as u64;
                    screen_info.smem_len = (framebuffer.width()
                        * framebuffer.height()
                        * framebuffer.bytes_per_pixel())
                        as u32;
                    screen_info.line_length =
                        (framebuffer.width() * framebuffer.bytes_per_pixel()) as u32;

                    current_userspace!().write_val(arg, &screen_info)?;

                    Ok(0)
                } else {
                    println!("Framebuffer is not initialized");
                    return_errno!(Errno::ENODEV); // No such device
                }
            }
            IoctlCmd::PUTVSCREENINFO => {
                // Not support for efifb
                // Behavior is aligned with Linux
                Ok(0)
            }
            IoctlCmd::GETCMAP => {
                println!("Fb ioctl: Get color map");
                // Implement logic to get the color map
                Ok(0)
            }
            IoctlCmd::PUTCMAP => {
                println!("Fb ioctl: Set color map");
                // Implement logic to set the color map
                Ok(0)
            }
            IoctlCmd::PANDISPLAY => {
                // Not support for efifb
                // Behavior is aligned with Linux
                return_errno!(Errno::EINVAL);
            }
            IoctlCmd::FBIOBLANK => {
                // Not support for efifb
                // Behavior is aligned with Linux
                return_errno!(Errno::EINVAL);
            }
            _ => {
                println!("Fb ioctl: Unsupported command -> {:?}", cmd);
                return_errno!(Errno::EINVAL); // Invalid argument error
            }
        }
    }
}
