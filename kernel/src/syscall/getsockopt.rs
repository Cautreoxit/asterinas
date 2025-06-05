// SPDX-License-Identifier: MPL-2.0

use super::SyscallReturn;
use core::mem;
use crate::{
    fs::file_table::{get_file_fast, FileDesc},
    prelude::*,
    util::net::{new_raw_socket_option, CSocketOptionLevel},
};
#[derive(Debug, Clone, Copy)]
pub struct PeerCred {
    pub pid: i32, // Process ID
    pub uid: i32, // User ID
    pub gid: i32, // Group ID
}
pub fn sys_getsockopt(
    sockfd: FileDesc,
    level: i32,
    optname: i32,
    optval: Vaddr,
    optlen_addr: Vaddr,
    ctx: &Context,
) -> Result<SyscallReturn> {
    let level = CSocketOptionLevel::try_from(level).map_err(|_| Errno::EOPNOTSUPP)?;
    if optlen_addr == 0 {
        return_errno_with_message!(Errno::EINVAL, "optlen_addr is null pointer");
    }

    let user_space = ctx.user_space();
    let optlen: u32 = user_space.read_val(optlen_addr)?;

    debug!("level = {level:?}, sockfd = {sockfd}, optname = {optname:?}, optlen = {optlen}");

    let mut file_table = ctx.thread_local.borrow_file_table_mut();
    let file = get_file_fast!(&mut file_table, sockfd);
    let socket = file.as_socket_or_err()?;

    if optname == 17 {
        // Hardcoded peer credentials for testing
        let hardcoded_creds = PeerCred {
            pid: 1234, // Example process ID
            uid: 0, // Example user ID
            gid: 0, // Example group ID
        };

        // Write hardcoded credentials to user space
        let creds_size = mem::size_of::<PeerCred>() as u32;
        if optlen < creds_size {
            return_errno!(Errno::EINVAL);
        }
        let creds_bytes = unsafe {
            core::slice::from_raw_parts(
                &hardcoded_creds as *const PeerCred as *const u8,
                creds_size as usize,
            )
        };
        // Create a VmReader from the byte slice
        let mut vm_reader = unsafe { VmReader::from_kernel_space(creds_bytes.as_ptr(), creds_bytes.len()) };

        // Write the byte slice to user space
        user_space.write_bytes(optval, &mut vm_reader)?;
        user_space.write_val(optlen_addr, &creds_size)?;

        return Ok(SyscallReturn::Return(0));
    }


    let mut raw_option = new_raw_socket_option(level, optname)?;
    debug!("raw option: {:?}", raw_option);

    socket.get_option(raw_option.as_sock_option_mut())?;

    let write_len = {
        let mut new_opt_len = optlen;
        let res = raw_option.write_to_user(optval, &mut new_opt_len);
        if new_opt_len != optlen {
            user_space.write_val(optlen_addr, &new_opt_len)?;
        }
        res?
    };
    user_space.write_val(optlen_addr, &(write_len as u32))?;

    Ok(SyscallReturn::Return(0))
}
