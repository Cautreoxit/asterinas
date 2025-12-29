// SPDX-License-Identifier: MPL-2.0

#[derive(Debug)]
pub(super) struct NvmeNamespace {
    /// Namespace ID reported by the controller.
    pub id: u32,
    /// Total number of logical blocks (NSZE).
    pub nsze: u64,
    /// Logical block size in bytes (2^LBADS from the active LBA format).
    pub lba_size: u64,
}
