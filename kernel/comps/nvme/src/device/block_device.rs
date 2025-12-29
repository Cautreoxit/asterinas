// SPDX-License-Identifier: MPL-2.0

//! NVMe Block Device implementation.
//!
//! This module implements the block device interface for NVMe storage devices,
//! following the NVM Express Base Specification Revision 2.0.

use alloc::{borrow::ToOwned, format, string::String, sync::Arc, vec::Vec};
use core::{
    ffi::CStr,
    hint::spin_loop,
    sync::atomic::{AtomicU32, Ordering},
};

use aster_block::{
    BlockDeviceMeta, SECTOR_SIZE,
    bio::{BioEnqueueError, BioStatus, BioType, SubmittedBio, bio_segment_pool_init},
    request_queue::{BioRequest, BioRequestSingleQueue},
};
use aster_util::safe_ptr::SafePtr;
use device_id::DeviceId;
use ostd::{
    debug, error, info,
    mm::{HasDaddr, HasSize, dma::DmaStream},
    sync::{LocalIrqDisabled, SpinLock, SpinLockGuard, WaitQueue},
};

use super::namespace::NvmeNamespace;
use crate::{
    NVME_BLOCK_MAJOR_ID, NvmePciTransport, NvmePciTransportLock,
    device::{MAX_NS_NUM, NvmeDeviceError, NvmeStats},
    nvme_cmd,
    nvme_queue::{
        NvmeCompletionQueue, NvmeCompletionQueueGuard, NvmeSubmissionQueue,
        NvmeSubmissionQueueGuard, QUEUE_NUM,
    },
    nvme_regs::{NvmeRegs32, NvmeRegs64},
    nvme_spec::{NvmeCommand, NvmeCompletion},
};

#[derive(Debug)]
pub struct NvmeBlockDevice {
    device: NvmeDeviceInner,
    queue: BioRequestSingleQueue,
    name: String,
    id: DeviceId,
}

impl aster_block::BlockDevice for NvmeBlockDevice {
    fn enqueue(&self, bio: SubmittedBio) -> Result<(), BioEnqueueError> {
        self.queue.enqueue(bio)
    }

    fn metadata(&self) -> BlockDeviceMeta {
        let ns = &self.device.namespace;

        BlockDeviceMeta {
            max_nr_segments_per_bio: self.queue.max_nr_segments_per_bio(),
            nr_sectors: (ns.nsze * ns.lba_size / SECTOR_SIZE as u64) as usize,
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn id(&self) -> DeviceId {
        self.id
    }
}

static NR_NVME_DEVICE: AtomicU32 = AtomicU32::new(0);

impl NvmeBlockDevice {
    pub(crate) fn init(transport: NvmePciTransport) -> Result<(), NvmeDeviceError> {
        let device = NvmeDeviceInner::init(transport)?;

        let index = NR_NVME_DEVICE.fetch_add(1, Ordering::Relaxed);
        let name = formatted_device_name(index);
        let id = {
            // Use the allocated major ID for the NVMe device
            let major_id = NVME_BLOCK_MAJOR_ID.get().unwrap().get();
            DeviceId::new(major_id, device_id::MinorId::new(index))
        };

        let block_device = Arc::new(Self {
            device,
            queue: BioRequestSingleQueue::new(),
            name,
            id,
        });

        block_device.device.setup_msix_handlers(&block_device);

        aster_block::register(block_device).unwrap();

        bio_segment_pool_init();
        Ok(())
    }

    /// Dequeues a `BioRequest` from the software staging queue and
    /// processes the request.
    pub fn handle_requests(&self) {
        let request = self.queue.dequeue();
        info!("[NVMe]: Handle Request: {:?}", request);
        match request.type_() {
            BioType::Read => self.device.read(request),
            BioType::Write => self.device.write(request),
            BioType::Flush => self.device.flush(request),
        }
    }
}

fn formatted_device_name(index: u32) -> String {
    format!("nvme{}n1", index)
}

#[derive(Copy, Clone, Pod)]
#[repr(C)]
struct IdentifyControllerData {
    _reserved: [u8; 4],
    serial: [u8; 20],
    model: [u8; 40],
    firmware: [u8; 8],
    _rest: [u8; 56],
}

#[derive(Copy, Clone, Pod)]
#[repr(C)]
struct IdentifyNamespaceListData {
    nsids: [u32; MAX_NS_NUM],
}

/// Identify Namespace data structure returned for CNS 00h (NVM Command Set Specification Figure 114).
///
/// Only the fields needed to determine the active LBA format are captured here;
/// the remaining bytes of the 4096-byte response are not used.
#[derive(Copy, Clone, Pod)]
#[repr(C)]
struct IdentifyNamespaceData {
    /// NSZE: total number of logical blocks.
    nsze: u64,
    /// NCAP: namespace capacity in logical blocks.
    _ncap: u64,
    /// NUSE: namespace utilization in logical blocks.
    _nuse: u64,
    /// NSFEAT (byte 24).
    _nsfeat: u8,
    /// NLBAF: number of LBA formats supported minus one (byte 25).
    _nlbaf: u8,
    /// FLBAS: formatted LBA size; bits[3:0] index into `lbaf[]` (byte 26).
    flbas: u8,
    /// RESERVED: bytes 27–127.
    _reserved: [u8; 101],
    /// LBAF[0..15]: LBA format support descriptors (bytes 128–191).
    ///
    /// Each entry is a u32: bits[23:16] = LBADS (LBA data-size exponent,
    /// so actual size = 2^LBADS bytes).
    lbaf: [u32; 16],
}

struct InitContext {
    submission_queues: [NvmeSubmissionQueue; QUEUE_NUM],
    completion_queues: [NvmeCompletionQueue; QUEUE_NUM],
    transport: NvmePciTransport,
    namespace: Option<NvmeNamespace>,
    dstrd: u16,
    io_msix_vectors: [Option<u16>; QUEUE_NUM],
}

struct NvmeDeviceInner {
    submission_queues: [SpinLock<NvmeSubmissionQueue, LocalIrqDisabled>; QUEUE_NUM],
    completion_queues: [SpinLock<NvmeCompletionQueue, LocalIrqDisabled>; QUEUE_NUM],
    completion_wait_queues: [WaitQueue; QUEUE_NUM],
    dstrd: u16,
    transport: NvmePciTransportLock,
    namespace: NvmeNamespace,
    stats: NvmeStats,
    io_msix_vectors: [Option<u16>; QUEUE_NUM],
}

impl core::fmt::Debug for NvmeDeviceInner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NvmeDeviceInner")
            .field("dstrd", &self.dstrd)
            .finish_non_exhaustive()
    }
}

impl NvmeDeviceInner {
    const CAP_REG_HIGH_DWORD_SHIFT: u32 = 32;
    const DSTRD_MASK: u64 = 0b1111;

    fn init(transport: NvmePciTransport) -> Result<Self, NvmeDeviceError> {
        let mut transport = transport;
        let dstrd = ((transport.regs().read64(NvmeRegs64::Cap) >> Self::CAP_REG_HIGH_DWORD_SHIFT)
            & Self::DSTRD_MASK) as u16;

        let mut init_ctx = InitContext::new(transport, dstrd)?;

        // NVMe controller initialization sequence.
        //
        // See NVMe Spec 2.0, Section 3.5 (Controller Initialization).
        //   1. Wait for CSTS.RDY to become '0' (controller ready to be reset)
        //   2. Configure Admin Queue by setting AQA, ASQ, and ACQ
        //   3. Set I/O queue entry sizes (CC.IOSQES and CC.IOCQES)
        //   4. Enable the controller by setting CC.EN to '1'
        //   5. Wait for CSTS.RDY to become '1' (controller ready to process commands)
        //   6. Configure MSI-X interrupts for the Admin Queue
        init_ctx.reset_controller();
        init_ctx.configure_admin_queue();
        init_ctx.set_entry_size();
        init_ctx.enable_controller();
        init_ctx.identify_controller()?;

        let nsids = init_ctx.identify_ns_list()?;
        if nsids.is_empty() {
            error!("[NVMe]: No namespaces found on this device");
            return Err(NvmeDeviceError::NoNamespace);
        }

        init_ctx.identify_ns(nsids[0])?;
        init_ctx.create_io_queues()?;

        let namespace = init_ctx.namespace.ok_or(NvmeDeviceError::NoNamespace)?;

        Ok(NvmeDeviceInner {
            submission_queues: init_ctx.submission_queues.map(SpinLock::new),
            completion_queues: init_ctx.completion_queues.map(SpinLock::new),
            completion_wait_queues: [WaitQueue::new(), WaitQueue::new()],
            dstrd: init_ctx.dstrd,
            transport: NvmePciTransportLock::new(init_ctx.transport),
            namespace,
            stats: NvmeStats::new(),
            io_msix_vectors: init_ctx.io_msix_vectors,
        })
    }

    /// Registers MSI-X handlers that wake completion wait queues for admin and I/O queues.
    fn setup_msix_handlers(&self, block_device: &Arc<NvmeBlockDevice>) {
        let mut transport = self.transport.lock();
        let msix_manager = transport.msix_manager_mut();

        // Admin queue interrupt (vector 0)
        let (_admin_vec, admin_irq) = msix_manager.admin_irq();
        let device_weak = Arc::downgrade(block_device);
        admin_irq.on_active(move |_| {
            if let Some(block_device) = device_weak.upgrade() {
                block_device.device.completion_wait_queues[0].wake_all();
            }
        });

        // I/O queues
        for io_qid in 1..QUEUE_NUM {
            if let Some(vector) = self.io_msix_vectors[io_qid]
                && let Some(io_irq) = msix_manager.irq_for_vector_mut(vector)
            {
                let device_weak = Arc::downgrade(block_device);
                let handler_qid = io_qid;
                io_irq.on_active(move |_| {
                    if let Some(block_device) = device_weak.upgrade() {
                        block_device.device.completion_wait_queues[handler_qid].wake_all();
                    }
                });
            }
        }
    }
}

impl InitContext {
    /// Controller Configuration Enable bit.
    const NVME_CC_ENABLE: u32 = 0x1;
    /// Controller Status Ready bit.
    const NVME_CSTS_RDY: u32 = 0x1;
    /// I/O Submission Queue Entry Size bits.
    const IOSQES_BITS: u32 = 20;
    /// I/O Submission Queue Entry Size value.
    const IOSQES_VALUE: u32 = 4;
    /// I/O Completion Queue Entry Size bits.
    const IOCQES_BITS: u32 = 16;
    /// I/O Completion Queue Entry Size value.
    const IOCQES_VALUE: u32 = 6;

    fn new(transport: NvmePciTransport, dstrd: u16) -> Result<Self, NvmeDeviceError> {
        let sq0 = NvmeSubmissionQueue::new().ok_or(NvmeDeviceError::QueueAllocationFailed)?;
        let sq1 = NvmeSubmissionQueue::new().ok_or(NvmeDeviceError::QueueAllocationFailed)?;
        let cq0 = NvmeCompletionQueue::new().ok_or(NvmeDeviceError::QueueAllocationFailed)?;
        let cq1 = NvmeCompletionQueue::new().ok_or(NvmeDeviceError::QueueAllocationFailed)?;
        Ok(Self {
            submission_queues: [sq0, sq1],
            completion_queues: [cq0, cq1],
            transport,
            namespace: None,
            dstrd,
            io_msix_vectors: [None; QUEUE_NUM],
        })
    }

    fn sq_mut(&mut self, qid: usize) -> NvmeSubmissionQueueGuard<'_, &mut NvmeSubmissionQueue> {
        NvmeSubmissionQueueGuard::new(
            qid as u16,
            self.dstrd,
            &mut self.submission_queues[qid],
            self.transport.dbregs(),
        )
    }

    fn cq_mut(&mut self, qid: usize) -> NvmeCompletionQueueGuard<'_, &mut NvmeCompletionQueue> {
        NvmeCompletionQueueGuard::new(
            qid as u16,
            self.dstrd,
            &mut self.completion_queues[qid],
            self.transport.dbregs(),
        )
    }

    fn reset_controller(&mut self) {
        let mut cc = self.transport.regs().read32(NvmeRegs32::Cc);
        cc &= !Self::NVME_CC_ENABLE;
        self.transport.regs().write32(NvmeRegs32::Cc, cc);

        loop {
            let csts = self.transport.regs().read32(NvmeRegs32::Csts);
            if (csts & Self::NVME_CSTS_RDY) == 0 {
                break;
            }
            spin_loop();
        }
    }

    fn configure_admin_queue(&mut self) {
        let acq = &self.completion_queues[0];
        let asq = &self.submission_queues[0];

        self.transport.regs().write32(
            NvmeRegs32::Aqa,
            ((acq.length() - 1) << 16) | (asq.length() - 1),
        );
        self.transport
            .regs()
            .write64(NvmeRegs64::Asq, asq.sq_daddr() as u64);
        self.transport
            .regs()
            .write64(NvmeRegs64::Acq, acq.cq_daddr() as u64);
    }

    fn set_entry_size(&mut self) {
        let mut cc = self.transport.regs().read32(NvmeRegs32::Cc);
        cc = cc
            | (Self::IOSQES_VALUE << Self::IOSQES_BITS)
            | (Self::IOCQES_VALUE << Self::IOCQES_BITS);
        self.transport.regs().write32(NvmeRegs32::Cc, cc);
    }

    fn enable_controller(&mut self) {
        let mut cc = self.transport.regs().read32(NvmeRegs32::Cc);
        cc |= Self::NVME_CC_ENABLE;
        self.transport.regs().write32(NvmeRegs32::Cc, cc);

        loop {
            let csts = self.transport.regs().read32(NvmeRegs32::Csts);
            if (csts & Self::NVME_CSTS_RDY) == 1 {
                break;
            }
            spin_loop();
        }
    }

    fn submit_and_wait_polling(
        &mut self,
        qid: usize,
        entry: NvmeCommand,
    ) -> Result<(), NvmeDeviceError> {
        let qid_u16 = qid as u16;
        {
            let mut sq = self.sq_mut(qid);
            if sq.submit(entry).is_none() {
                error!(
                    "[NVMe]: Submission queue {} is full (admin or I/O SQ)",
                    qid_u16
                );
                return Err(NvmeDeviceError::SubmissionQueueFull);
            }
        }

        let mut cq = self.cq_mut(qid);
        loop {
            if let Some(cqe) = cq.complete() {
                self.submission_queues[qid].update_sq_head(&cqe);
                break;
            }
            spin_loop();
        }
        Ok(())
    }

    fn identify_controller(&mut self) -> Result<(), NvmeDeviceError> {
        let stream = DmaStream::alloc(1, false).map_err(|_| {
            error!("[NVMe]: Failed to allocate DMA buffer for Identify Controller");
            NvmeDeviceError::DmaAllocationFailed
        })?;
        let data: SafePtr<IdentifyControllerData, DmaStream> = SafePtr::new(stream, 0);

        let qid = 0;
        let entry = nvme_cmd::identify_controller(data.daddr());
        self.submit_and_wait_polling(qid, entry)?;

        let result = data.read().unwrap();

        let serial = bytes_to_cstr_string(&result.serial);
        let model = bytes_to_cstr_string(&result.model);
        let firmware = bytes_to_cstr_string(&result.firmware);

        info!(
            "[NVMe]: Controller identified - Serial: {}, Model: {}, Firmware: {}",
            serial, model, firmware
        );
        Ok(())
    }

    fn identify_ns_list(&mut self) -> Result<Vec<u32>, NvmeDeviceError> {
        let stream = DmaStream::alloc(1, false).map_err(|_| {
            error!("[NVMe]: Failed to allocate DMA buffer for Identify Namespace List");
            NvmeDeviceError::DmaAllocationFailed
        })?;
        let data: SafePtr<IdentifyNamespaceListData, DmaStream> = SafePtr::new(stream, 0);

        let qid = 0;
        let entry = nvme_cmd::identify_namespace_list(data.daddr(), 0);
        self.submit_and_wait_polling(qid, entry)?;

        let result = data.read().unwrap();

        let mut nsids = Vec::new();
        for &nsid in result.nsids.iter() {
            if nsid != 0 {
                nsids.push(nsid);
            }
        }
        Ok(nsids)
    }

    fn identify_ns(&mut self, nsid: u32) -> Result<(), NvmeDeviceError> {
        let stream = DmaStream::alloc(1, false).map_err(|_| {
            error!("[NVMe]: Failed to allocate DMA buffer for Identify Namespace");
            NvmeDeviceError::DmaAllocationFailed
        })?;
        let data: SafePtr<IdentifyNamespaceData, DmaStream> = SafePtr::new(stream, 0);

        let qid = 0;
        let entry = nvme_cmd::identify_namespace(data.daddr(), nsid);
        self.submit_and_wait_polling(qid, entry)?;

        let result = data.read().unwrap();

        // Parse the active LBA format to obtain the logical block size.
        // FLBAS bits[3:0] select the current format entry in `lbaf[]`.
        // LBADS (bits[23:16] of that entry) is the base-2 exponent of the block size.
        let fmt_idx = (result.flbas & 0x0f) as usize;
        let lbads = (result.lbaf[fmt_idx] >> 16) & 0xff;
        let lba_size = 1u64 << lbads;

        debug!(
            "[NVMe]: Namespace {} - NSZE={}, LBA size={} bytes (LBADS={})",
            nsid, result.nsze, lba_size, lbads
        );

        self.namespace = Some(NvmeNamespace {
            id: nsid,
            nsze: result.nsze,
            lba_size,
        });
        Ok(())
    }

    fn create_io_queues(&mut self) -> Result<(), NvmeDeviceError> {
        let qid = 0;

        // Pre-allocate MSI-X vectors for I/O queues
        let msix_manager = self.transport.msix_manager_mut();
        for io_qid in 1..QUEUE_NUM {
            let (vector, _io_irq) = msix_manager.alloc_io_queue_irq().ok_or_else(|| {
                error!(
                    "[NVMe]: Failed to allocate MSI-X vector for I/O queue {}",
                    io_qid
                );
                NvmeDeviceError::MsixAllocationFailed
            })?;
            self.io_msix_vectors[io_qid] = Some(vector);
        }

        for io_qid in 1..QUEUE_NUM {
            let (cptr, clength) = {
                let cqueue = &self.completion_queues[io_qid];
                (cqueue.cq_daddr(), cqueue.length())
            };

            let msix_vector = self.io_msix_vectors[io_qid];

            let entry = nvme_cmd::create_io_completion_queue(
                io_qid as u16,
                cptr,
                (clength - 1) as u16,
                msix_vector,
            );
            self.submit_and_wait_polling(qid, entry)?;

            let (sptr, slen) = {
                let squeue = &self.submission_queues[io_qid];
                (squeue.sq_daddr(), squeue.length())
            };

            let entry = nvme_cmd::create_io_submission_queue(
                io_qid as u16,
                sptr,
                (slen - 1) as u16,
                io_qid as u16,
            );
            self.submit_and_wait_polling(qid, entry)?;
        }
        Ok(())
    }
}

impl NvmeDeviceInner {
    fn lock_sq(
        &self,
        qid: usize,
    ) -> NvmeSubmissionQueueGuard<'_, SpinLockGuard<'_, NvmeSubmissionQueue, LocalIrqDisabled>>
    {
        NvmeSubmissionQueueGuard::new(
            qid as u16,
            self.dstrd,
            self.submission_queues[qid].lock(),
            self.transport.dbregs(),
        )
    }

    fn lock_cq(
        &self,
        qid: usize,
    ) -> NvmeCompletionQueueGuard<'_, SpinLockGuard<'_, NvmeCompletionQueue, LocalIrqDisabled>>
    {
        NvmeCompletionQueueGuard::new(
            qid as u16,
            self.dstrd,
            self.completion_queues[qid].lock(),
            self.transport.dbregs(),
        )
    }

    /// Submits a command to the submission queue and waits for its completion.
    ///
    /// This helper assumes that only a single thread calls `submit_and_wait`
    /// for a given `qid` at a time. Calling it concurrently on the same queue
    /// is not supported now.
    ///
    /// Returns `Ok(())` if the command completed successfully,
    /// `Err(NvmeDeviceError::SubmissionQueueFull)` if the SQ has no free slots, or
    /// `Err(NvmeDeviceError::CommandFailed)` if the device reported a non-zero status.
    fn submit_and_wait(&self, qid: usize, entry: NvmeCommand) -> Result<(), NvmeDeviceError> {
        let wait_queue = &self.completion_wait_queues[qid];

        let cid = {
            let mut sq = self.lock_sq(qid);
            match sq.submit(entry) {
                Some(cid) => cid,
                None => {
                    error!("[NVMe]: Submission queue {} is full", qid as u16);
                    return Err(NvmeDeviceError::SubmissionQueueFull);
                }
            }
        };

        wait_queue.wait_until(|| {
            let mut cq = self.lock_cq(qid);
            if let Some(completion) = cq.complete() {
                let result = self.process_completion(qid, completion, cid);
                drop(cq);
                self.submission_queues[qid]
                    .lock()
                    .update_sq_head(&completion);
                result
            } else {
                None
            }
        })
    }

    /// Interprets a completion queue entry for the command identified by `expected_cid`.
    ///
    /// Returns `None` if the completion does not match `expected_cid` (not our command),
    /// `Some(Ok(()))` if it matches and the device reports success, or
    /// `Some(Err(NvmeDeviceError::CommandFailed))` if it matches but the device reports an error.
    fn process_completion(
        &self,
        qid: usize,
        completion: NvmeCompletion,
        expected_cid: u16,
    ) -> Option<Result<(), NvmeDeviceError>> {
        let is_target = completion.cid == expected_cid;
        if qid > 0 {
            self.stats.increment_completed();
        }

        if completion.has_error() {
            error!(
                "[NVMe]: Command failed: CID={}, Status={:04X} (SC={}), SQID={}, QID={}",
                completion.cid,
                completion.status,
                completion.status_code(),
                completion.sq_id,
                qid
            );
        }

        if is_target {
            if completion.has_error() {
                Some(Err(NvmeDeviceError::CommandFailed))
            } else {
                Some(Ok(()))
            }
        } else {
            None
        }
    }

    /// Performs read or write I/O for a `BioRequest` on I/O queue 1.
    ///
    /// Splits work into chunks; each chunk is built with `build_cmd` and submitted synchronously.
    fn io_rw_request(
        &self,
        request: BioRequest,
        build_cmd: fn(u32, u64, u16, u64, u64) -> NvmeCommand,
    ) {
        // TODO: Support PRP lists / `ptr1` so larger transfers do not require this fixed cap. For
        // now we only use `ptr0` and keep `ptr1` at 0, so each command is limited to 8 sectors.
        const MAX_HW_SECTORS_PER_CHUNK: u64 = 8;

        let nsid = self.namespace.id;
        let sectors_per_lba = self.namespace.lba_size / SECTOR_SIZE as u64;

        let mut lba = request.sid_range().start.to_raw() / sectors_per_lba;
        let qid = 1;

        for bio in request.bios() {
            bio.complete({
                let mut status = BioStatus::Complete;
                for segment in bio.segments() {
                    let dma_slice = segment.inner_dma_slice();
                    let seg_sectors = (dma_slice.size() / SECTOR_SIZE) as u64;
                    let mut remaining = seg_sectors;
                    let mut ptr0: u64 = dma_slice.daddr().try_into().unwrap();

                    while remaining > 0 {
                        let once_sectors = remaining.min(MAX_HW_SECTORS_PER_CHUNK);
                        let once_lbas = once_sectors / sectors_per_lba;
                        let ptr1 = 0u64;

                        let entry = build_cmd(nsid, lba, (once_lbas - 1) as u16, ptr0, ptr1);
                        // TODO: This path submits and waits synchronously, which may block.
                        if self.submit_and_wait(qid, entry).is_err() {
                            status = BioStatus::IoError;
                        }
                        self.stats.increment_submitted();

                        lba += once_lbas;
                        remaining -= once_sectors;
                        ptr0 += SECTOR_SIZE as u64 * once_sectors;
                    }
                }
                status
            });
        }
    }

    fn read(&self, request: BioRequest) {
        self.io_rw_request(request, nvme_cmd::io_read);
    }

    fn write(&self, request: BioRequest) {
        self.io_rw_request(request, nvme_cmd::io_write);
    }

    fn flush(&self, request: BioRequest) {
        let nsid = self.namespace.id;
        let qid = 1;

        let entry = nvme_cmd::io_flush(nsid);
        // TODO: This path submits and waits synchronously, which may block.
        let status = self
            .submit_and_wait(qid, entry)
            .map_or(BioStatus::IoError, |_| BioStatus::Complete);
        self.stats.increment_submitted();
        self.stats.increment_completed();
        request.bios().for_each(|bio| {
            bio.complete(status);
        });
    }
}

fn bytes_to_cstr_string(bytes: &[u8]) -> String {
    if let Ok(cstr) = CStr::from_bytes_until_nul(bytes) {
        let s = cstr.to_string_lossy();
        s.trim_end().to_owned()
    } else {
        String::new()
    }
}

#[cfg(ktest)]
mod test {
    use alloc::{sync::Arc, vec};

    use aster_block::{
        BLOCK_SIZE,
        bio::{Bio, BioDirection, BioSegment},
        id::{Bid, Sid},
    };
    use ostd::{
        info,
        mm::{FrameAllocOptions, VmIo, VmReader, io::util::HasVmReaderWriter},
        prelude::ktest,
    };

    use super::{BioType, NvmeBlockDevice};
    use crate::nvme_init;

    const TEST_CHAR: u8 = b'B';
    const TEST_BUF_LENGTH: usize = 8192;

    #[derive(Copy, Clone)]
    enum RequestType {
        Read,
        Write,
    }

    #[ktest]
    fn initialize() {
        ensure_initialized();
    }

    fn ensure_initialized() {
        if aster_block::collect_all().is_empty() {
            component::init_all(
                component::InitStage::Bootstrap,
                component::parse_metadata!(),
            )
            .unwrap();

            nvme_init().expect("`nvme_init` returned an error");
        }
    }

    #[ktest]
    fn write_then_read() {
        ensure_initialized();

        let device = match aster_block::collect_all()
            .into_iter()
            .find(|d| d.name() == "nvme0n1")
        {
            Some(device) => device,
            None => {
                info!("Skip nvme ktest: NVMe device not found");
                return;
            }
        };
        let device_arc = Arc::clone(&device);

        let nvme_block_device = device_arc
            .downcast_ref::<NvmeBlockDevice>()
            .expect("Failed to downcast device");

        let mut read_buf = [0u8; TEST_BUF_LENGTH];
        let val = TEST_CHAR;
        create_and_submit_bio_request(nvme_block_device, RequestType::Write, TEST_BUF_LENGTH, val);
        nvme_block_device.handle_requests();
        let read_bio_segment = create_and_submit_bio_request(
            nvme_block_device,
            RequestType::Read,
            TEST_BUF_LENGTH,
            val,
        );
        nvme_block_device.handle_requests();

        read_bio_segment
            .inner_dma_slice()
            .read_bytes(0, &mut read_buf)
            .unwrap();
        assert!(read_buf.iter().all(|&x| x == TEST_CHAR));
    }

    fn create_and_submit_bio_request(
        device: &NvmeBlockDevice,
        req_type: RequestType,
        buf_len: usize,
        val: u8,
    ) -> BioSegment {
        let buf_nblocks = buf_len / BLOCK_SIZE;
        let segment = FrameAllocOptions::new()
            .zeroed(false)
            .alloc_segment(buf_nblocks)
            .unwrap();

        if matches!(req_type, RequestType::Write) {
            let mut writer = segment.writer();
            let fill_buf = [val; BLOCK_SIZE];
            for _ in 0..buf_nblocks {
                let mut reader = VmReader::from(fill_buf.as_slice());
                writer.write(&mut reader);
            }
        }

        let (bio_type, direction) = match req_type {
            RequestType::Write => (BioType::Write, BioDirection::ToDevice),
            RequestType::Read => (BioType::Read, BioDirection::FromDevice),
        };
        let bio_segment = BioSegment::new_from_segment(segment.into(), direction);

        let bio = Bio::new(
            bio_type,
            Sid::from(Bid::from_offset(0)),
            vec![bio_segment.clone()],
            None,
        );
        let _ = bio.submit(device).unwrap();
        bio_segment
    }
}
