// SPDX-License-Identifier: MPL-2.0

#![expect(unused_variables)]

use super::*;
use crate::{
    current_userspace,
    events::IoEvents,
    fs::{inode_handle::FileIo, utils::IoctlCmd},
    prelude::*,
    process::signal::{PollHandle, Pollable},
    vm::perms::VmPerms,
};

use ostd::{Pod, mm::Frame, mm::UFrame, mm::PageFlags, mm::PageProperty, mm::CachePolicy};
pub(crate) use aster_framebuffer::get_framebuffer_info;

pub struct Fb;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct FbVarScreenInfo {
    pub xres: u32,
    pub yres: u32,
    pub xres_virtual: u32,
    pub yres_virtual: u32,
    pub bits_per_pixel: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct FbFixScreenInfo {
    pub smem_start: usize,  // Start of framebuffer memory
    pub smem_len: usize,    // Length of framebuffer memory
    pub line_length: usize, // Length of a line in bytes
    // Add other fields as needed
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

    fn mmap(
        &self,
        addr: Vaddr,
        len: usize,
        offset: usize,
        perms: VmPerms,
        ctx: &Context,
    ) -> Result<Vaddr> {
        println!("Fb mmap: Mapping framebuffer to user space");
    
        // Get the user space context
        let user_space: CurrentUserSpace<'_> = ctx.user_space();
        let root_vmar = user_space.root_vmar();
        println!("start to get framebuffer info, addr {:X}, len {:X}, offset {:X}",
                addr, len, offset);

        //if let Some(framebuffer) = get_framebuffer_info().as_deref() {
            // Ensure the framebuffer base address is page-aligned
            let framebuffer_paddr_start = align_down(
                get_framebuffer_info()
                    .ok_or(Errno::ENODEV)?
                    .io_mem_base() + offset,
                PAGE_SIZE,
            );
            let framebuffer_paddr_end = align_up(
                framebuffer_paddr_start + len,
                PAGE_SIZE,
            );
            println!("start to get vaddr_range");
            // Ensure the virtual address range is page-aligned
            let vaddr_range = if addr == 0 {
                root_vmar.vm_space().allocate_virtual_address_range(len)?
            } else {
                align_down(addr, PAGE_SIZE)..align_up(addr + len, PAGE_SIZE)
            };
    
            println!(
                "Got vaddr_range start: {:X}, end: {:X}",
                vaddr_range.start, vaddr_range.end
            );
    
            // Create a mutable cursor for the virtual address range
            let mut cursor = root_vmar.vm_space().cursor_mut(&vaddr_range)?;
            println!("Got cursor");
    
            // Map each page in the framebuffer's physical address range
            let mut current_paddr = framebuffer_paddr_start;
            let mut current_vaddr = vaddr_range.start;
            println!("Start to map");
    
            while current_paddr < framebuffer_paddr_end {
                // Create a Frame<dyn AnyFrameMeta> from the physical address
                let dyn_frame = Frame::from_in_use(current_paddr)
                    .map_err(|_| Errno::ENOMEM)?; // Handle errors if the frame is not in use
                //println!("Got dyn_frame");
                // Convert the Frame<dyn AnyFrameMeta> to a UFrame
                let frame = UFrame::try_from(dyn_frame)
                    .map_err(|_| Errno::EINVAL)?; // Handle errors if the frame is not untyped
                //println!("Got UFrame");
                // Map the frame to the virtual address
                let temp_perms = PageProperty::new(PageFlags::RW, CachePolicy::Writeback);
                cursor.map(frame, temp_perms);
    
                //println!("cur_paddr {:X}, cur_vaddr {:X}", current_paddr, current_vaddr);
    
                // Move to the next page
                current_paddr += PAGE_SIZE;
                current_vaddr += PAGE_SIZE;
            }
    
            Ok(vaddr_range.start) // Return the starting virtual address
        // } else {
        //     println!("ENOMEM");
        //     Err(Errno::ENOMEM.into())
        // }
    }

    fn ioctl(&self, cmd: IoctlCmd, arg: usize) -> Result<i32> {
        match cmd {
            IoctlCmd::GETVSCREENINFO => {
                println!("Fb ioctl: Get virtual screen info");

                // Use get_framebuffer_info to access the framebuffer
                if let Some(framebuffer_guard) = get_framebuffer_info() {
                    let framebuffer = &*framebuffer_guard; // Dereference the guard to access the FrameBuffer

                    let screen_info = FbVarScreenInfo {
                        xres: framebuffer.width() as u32,
                        yres: framebuffer.height() as u32,
                        xres_virtual: framebuffer.width() as u32,
                        yres_virtual: framebuffer.height() as u32,
                        bits_per_pixel: (framebuffer.bytes_per_pixel() * 8) as u32,
                    };

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

                    let screen_info = FbFixScreenInfo {
                        smem_start: framebuffer.io_mem_base(),
                        smem_len: framebuffer.width()
                            * framebuffer.height()
                            * framebuffer.bytes_per_pixel(),
                        line_length: framebuffer.width() * framebuffer.bytes_per_pixel(),
                    };

                    current_userspace!().write_val(arg, &screen_info)?;

                    Ok(0)
                } else {
                    println!("Framebuffer is not initialized");
                    return_errno!(Errno::ENODEV); // No such device
                }
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
                println!("Fb ioctl: Pan display");
                let offset = arg; // Assume `arg` contains the offset value
                println!("Panning display to offset: {}", offset);

                // Implement logic to pan the display
                Ok(0)
            }
            IoctlCmd::FBIOBLANK => {
                println!("Fb ioctl: Blank screen");
                let blank_mode = arg; // Assume `arg` contains the blank mode
                println!("Setting blank mode to: {}", blank_mode);

                // Implement logic to blank the screen
                Ok(0)
            }
            _ => {
                println!("Fb ioctl: Unsupported command -> {:?}", cmd);
                return_errno!(Errno::EINVAL); // Invalid argument error
            }
        }
    }
}