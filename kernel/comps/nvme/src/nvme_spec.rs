// SPDX-License-Identifier: MPL-2.0

//! NVMe protocol data structures.
//!
//! Refer to NVM Express Base Specification Revision 2.0, Section 3.3.3

/// Submission Queue Entry (SQE).
///
/// See NVMe Spec 2.0, Section 3.3.1 (Submission Queue Entry).
#[repr(C)]
#[derive(Debug, Copy, Clone, Pod)]
pub(crate) struct NvmeCommand {
    /// Opcode.
    pub opcode: u8,
    /// Flags.
    pub flags: u8,
    /// Command ID.
    pub cid: u16,
    /// Namespace identifier.
    pub nsid: u32,
    /// Reserved.
    pub _rsvd: u64,
    /// Metadata pointer.
    pub mptr: u64,
    /// Data pointer.
    pub dptr: [u64; 2],
    /// Command dword 10.
    pub cdw10: u32,
    /// Command dword 11.
    pub cdw11: u32,
    /// Command dword 12.
    pub cdw12: u32,
    /// Command dword 13.
    pub cdw13: u32,
    /// Command dword 14.
    pub cdw14: u32,
    /// Command dword 15.
    pub cdw15: u32,
}

/// Completion Queue Entry (CQE).
///
/// See NVMe Spec 2.0, Section 3.3.3.2 (Common Completion Queue Entry), Figure 89.
/// Layout by dword (offsets0–15 in the entry):
/// - **Dword 0**: Command specific.
/// - **Dword 1**: Command specific.
/// - **Dword 2**: SQ Head Pointer (bits 15:0) | SQ Identifier (bits 31:16).
/// - **Dword 3**: Command Identifier (bits 15:0) | Phase Tag (bit 16) | Status Field (bits 31:17, 15 bits).
///
/// The Status bits are further defined in Figure 92 (DNR, M, CRD, SCT, SC, etc.).
#[repr(C)]
#[derive(Debug, Copy, Clone, Pod)]
pub(crate) struct NvmeCompletion {
    /// Dword 0: Command Specific (32 bits).
    pub dword0: u32,

    /// Dword 1: Command Specific (32 bits).
    pub dword1: u32,

    /// Dword 2, bits 0-15: SQ Head Pointer (16 bits).
    ///
    /// The head pointer of the corresponding Submission Queue that is updated
    /// by the controller when this entry is placed into the Completion Queue.
    pub sq_head: u16,

    /// Dword 2, bits 16-31: SQ Identifier (16 bits).
    ///
    /// The Submission Queue identifier that is associated with this completion.
    pub sq_id: u16,

    /// Dword 3, bits 0-15: Command Identifier (16 bits).
    ///
    /// The Command Identifier (CID) of the command that this completion is associated with.
    pub cid: u16,

    /// Dword 3, bits 16-31: Status Field (16 bits).
    pub status: u16,
}

impl NvmeCompletion {
    /// Masks the Status field (DW3 bits 31:17) in bits 15:1 of `status`.
    const STATUS_FIELD_MASK: u16 = 0xFFFE;

    /// Masks Status Code (SC), DW3 bits 24:17, in bits 8:1 of `status`.
    const STATUS_SC_MASK: u16 = 0x01FE;

    /// Returns the Phase Tag (P).
    pub(crate) fn phase_tag(&self) -> bool {
        (self.status & 1) != 0
    }

    /// Checks if the completion indicates an error.
    ///
    /// Returns `true` if the Status Code is non-zero or DNR is set.
    pub(crate) fn has_error(&self) -> bool {
        (self.status & Self::STATUS_FIELD_MASK) != 0
    }

    /// Gets the Status Code from the completion status field.
    ///
    /// Returns the Status Code (SC).
    pub(crate) fn status_code(&self) -> u8 {
        ((self.status & Self::STATUS_SC_MASK) >> 1) as u8
    }
}
