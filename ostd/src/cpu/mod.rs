// SPDX-License-Identifier: MPL-2.0

//! CPU-related definitions.

pub mod local;
pub mod set;

cfg_if::cfg_if! {
    if #[cfg(target_arch = "x86_64")] {
        pub use crate::arch::x86::cpu::*;
    } else if #[cfg(target_arch = "riscv64")] {
        pub use crate::arch::riscv::cpu::*;
    }
}

pub use set::{AtomicCpuSet, CpuSet};
use spin::Once;

use crate::{arch::boot::smp::count_processors, cpu_local_cell, task::atomic_mode::InAtomicMode};

/// The ID of a CPU in the system.
///
/// If converting from/to an integer, the integer must start from 0 and be less
/// than the number of CPUs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuId(u32);

impl CpuId {
    /// Returns the CPU ID of the bootstrap processor (BSP).
    pub const fn bsp() -> Self {
        CpuId(0)
    }

    /// Converts the CPU ID to an `usize`.
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

impl TryFrom<usize> for CpuId {
    type Error = &'static str;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        if value < num_cpus() {
            Ok(CpuId(value as u32))
        } else {
            Err("The given CPU ID is out of range")
        }
    }
}

/// The number of CPUs.
static NUM_CPUS: Once<u32> = Once::new();

/// Initializes the number of CPUs.
///
/// # Safety
///
/// The caller must ensure that this function is called only once on the BSP
/// at the correct time when the number of CPUs is available from the platform.
pub(crate) unsafe fn init_num_cpus() {
    let num_processors = count_processors().unwrap_or(1);
    NUM_CPUS.call_once(|| num_processors);
}

/// Initializes the number of the current CPU.
///
/// # Safety
///
/// The caller must ensure that this function is called only once on the
/// correct CPU with the correct CPU ID.
pub(crate) unsafe fn set_this_cpu_id(id: u32) {
    CURRENT_CPU.store(id);
}

/// Returns the number of CPUs.
pub fn num_cpus() -> usize {
    debug_assert!(
        NUM_CPUS.get().is_some(),
        "The number of CPUs is not initialized"
    );
    // SAFETY: The number of CPUs is initialized. The unsafe version is used
    // to avoid the overhead of the check.
    let num = unsafe { *NUM_CPUS.get_unchecked() };
    num as usize
}

/// Returns an iterator over all CPUs.
pub fn all_cpus() -> impl Iterator<Item = CpuId> {
    (0..num_cpus()).map(|id| CpuId(id as u32))
}

/// A marker trait for guard types that can "pin" the current task to the
/// current CPU.
///
/// Such guard types include [`DisabledLocalIrqGuard`] and
/// [`DisabledPreemptGuard`]. When such guards exist, the CPU executing the
/// current task is pinned. So getting the current CPU ID or CPU-local
/// variables are safe.
///
/// # Safety
///
/// The implementor must ensure that the current task is pinned to the current
/// CPU while any one of the instances of the implemented structure exists.
pub unsafe trait PinCurrentCpu {
    /// Returns the ID of the current CPU.
    fn current_cpu(&self) -> CpuId {
        current_cpu_racy()
    }
}

/// Returns the ID of the current CPU.
///
/// This function is safe to call, but is vulnerable to races. The returned CPU
/// ID may be outdated if the task migrates to another CPU.
///
/// To ensure that the CPU ID is up-to-date, do it under any guards that
/// implements the [`PinCurrentCpu`] trait.
pub fn current_cpu_racy() -> CpuId {
    let id = CURRENT_CPU.load();
    debug_assert_ne!(id, u32::MAX, "This CPU is not initialized");
    CpuId(id)
}

// SAFETY: A guard that enforces the atomic mode requires disabling any
// context switching. So naturally, the current task is pinned on the CPU.
unsafe impl<T: InAtomicMode> PinCurrentCpu for T {}
unsafe impl PinCurrentCpu for dyn InAtomicMode + '_ {}

cpu_local_cell! {
    /// The number of the current CPU.
    static CURRENT_CPU: u32 = u32::MAX;
}
