// SPDX-License-Identifier: MPL-2.0

use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::{fmt::Debug, iter, mem};

use aster_input::{
    event_type_codes::{EventType, KeyStatus},
    InputCapability, InputDevice as InputDeviceTrait,
    InputEvent, InputId,
};
use aster_time::read_monotonic_time;
use aster_util::{field_ptr, safe_ptr::SafePtr};
use bitflags::bitflags;
use log::{debug, info};
use ostd::{
    arch::trap::TrapFrame,
    io::IoMem,
    mm::{DmaDirection, DmaStream, FrameAllocOptions, HasDaddr, VmIo, PAGE_SIZE},
    sync::{LocalIrqDisabled, RwLock, SpinLock},
};

use super::{InputConfigSelect, VirtioInputConfig, VirtioInputEvent, QUEUE_EVENT, QUEUE_STATUS};
use crate::{
    device::VirtioDeviceError, dma_buf::DmaBuf, queue::VirtQueue, transport::VirtioTransport,
};

bitflags! {
    /// The properties of input device.
    ///
    /// Ref: Linux input-event-codes.h
    pub struct InputProp : u8 {
        /// Needs a pointer
        const POINTER           = 1 << 0;
        /// Direct input devices
        const DIRECT            = 1 << 1;
        /// Has button(s) under pad
        const BUTTONPAD         = 1 << 2;
        /// Touch rectangle only
        const SEMI_MT           = 1 << 3;
        /// Softbuttons at top of pad
        const TOPBUTTONPAD      = 1 << 4;
        /// Is a pointing stick
        const POINTING_STICK    = 1 << 5;
        /// Has accelerometer
        const ACCELEROMETER     = 1 << 6;
    }
}

pub const SYN: u8 = 0x00;
pub const KEY: u8 = 0x01;
pub const REL: u8 = 0x02;
pub const ABS: u8 = 0x03;
pub const MSC: u8 = 0x04;
pub const SW: u8 = 0x05;
pub const LED: u8 = 0x11;
pub const SND: u8 = 0x12;
pub const REP: u8 = 0x14;
pub const FF: u8 = 0x15;
pub const PWR: u8 = 0x16;
pub const FF_STATUS: u8 = 0x17;

const QUEUE_SIZE: u16 = 64;

/// Global device reference for IRQ handler
static VIRTIO_INPUT_DEVICE: spin::Once<Arc<dyn InputDeviceTrait>> = spin::Once::new();

/// Virtual human interface devices such as keyboards, mice and tablets.
///
/// An instance of the virtio device represents one such input device.
/// Device behavior mirrors that of the evdev layer in Linux,
/// making pass-through implementations on top of evdev easy.
pub struct InputDevice {
    config: SafePtr<VirtioInputConfig, IoMem>,
    event_queue: SpinLock<VirtQueue>,
    status_queue: VirtQueue,
    event_table: EventTable,
    #[expect(clippy::type_complexity)]
    callbacks: RwLock<Vec<Arc<dyn Fn(InputEvent) + Send + Sync + 'static>>, LocalIrqDisabled>,
    transport: SpinLock<Box<dyn VirtioTransport>>,
    // New fields for InputDev trait
    device_name: String,
    device_phys: String,
    device_id: InputId,
    capability: InputCapability,
}

impl InputDevice {
    /// Create a new VirtIO-Input driver.
    /// msix_vector_left should at least have one element or n elements where n is the virtqueue amount
    pub fn init(mut transport: Box<dyn VirtioTransport>) -> Result<(), VirtioDeviceError> {
        let mut event_queue = VirtQueue::new(QUEUE_EVENT, QUEUE_SIZE, transport.as_mut())
            .expect("create event virtqueue failed");
        let status_queue = VirtQueue::new(QUEUE_STATUS, QUEUE_SIZE, transport.as_mut())
            .expect("create status virtqueue failed");

        let event_table = EventTable::new(QUEUE_SIZE as usize);
        for i in 0..event_table.num_events() {
            let event_buf = event_table.get(i);
            let token = event_queue.add_dma_buf(&[], &[&event_buf]);
            match token {
                Ok(value) => {
                    assert_eq!(value, i as u16);
                }
                Err(_) => {
                    return Err(VirtioDeviceError::QueueUnknownError);
                }
            }
        }

        // Create initial capability and metadata
        let mut capability = InputCapability::new();
        // We'll set the actual capabilities after querying the device

        let temp_device = Self {
            config: VirtioInputConfig::new(transport.as_mut()),
            event_queue: SpinLock::new(event_queue),
            status_queue,
            event_table,
            transport: SpinLock::new(transport),
            callbacks: RwLock::new(Vec::new()),
            device_name: "virtio_input".to_string(), // Default name, can be queried later
            device_phys: "virtio".to_string(),
            device_id: InputId {
                bustype: InputId::BUS_VIRTUAL,
                vendor: 0x1AF4,  // VirtIO vendor ID
                product: 0x0012, // VirtIO input device ID
                version: 0x0001,
            },
            capability,
        };

        let device = Arc::new(temp_device);

        let name = device.query_config_id_name();
        info!("Virtio input device name:{}", name);

        let input_prop = device.query_config_prop_bits();
        if let Some(prop) = input_prop {
            debug!("input device prop: {:?}", prop);
        } else {
            debug!("input device has no properties or the properties is not defined");
        }

        let mut transport = device.transport.disable_irq().lock();
        fn config_space_change(_: &TrapFrame) {
            debug!("input device config space change");
        }
        transport
            .register_cfg_callback(Box::new(config_space_change))
            .unwrap();

        let handle_input = {
            let device = device.clone();
            move |_: &TrapFrame| device.handle_irq()
        };
        transport
            .register_queue_callback(QUEUE_EVENT, Box::new(handle_input), false)
            .unwrap();

        transport.finish_init();
        drop(transport);

        // Register with the new input subsystem
        aster_input::register_device(device.clone()).map_err(|_| VirtioDeviceError::QueueUnknownError)?;

        // Store device reference for IRQ handler (convert to trait object)
        let device_trait_object: Arc<dyn InputDeviceTrait> = device;
        VIRTIO_INPUT_DEVICE.call_once(|| device_trait_object);

        Ok(())
    }

    /// Pop the pending event.
    fn pop_pending_events(&self, handle_event: &impl Fn(&EventBuf) -> bool) {
        let mut event_queue = self.event_queue.disable_irq().lock();

        // one interrupt may contain several input events, so it should loop
        while let Ok((token, _)) = event_queue.pop_used() {
            debug_assert!(token < QUEUE_SIZE);
            let ptr = self.event_table.get(token as usize);
            let res = handle_event(&ptr);
            let new_token = event_queue.add_dma_buf(&[], &[&ptr]).unwrap();
            // This only works because nothing happen between `pop_used` and `add` that affects
            // the list of free descriptors in the queue, so `add` reuses the descriptor which
            // was just freed by `pop_used`.
            assert_eq!(new_token, token);

            if !res {
                break;
            }
        }
    }

    pub fn query_config_id_name(&self) -> String {
        let size = self.select_config(InputConfigSelect::IdName, 0);

        let out = {
            // TODO: Add a general API to read this byte-by-byte.
            let mut out = Vec::with_capacity(size);
            let mut data_ptr = field_ptr!(&self.config, VirtioInputConfig, data).cast::<u8>();
            for _ in 0..size {
                out.push(data_ptr.read_once().unwrap());
                data_ptr.byte_add(1);
            }
            out
        };

        String::from_utf8(out).unwrap()
    }

    pub fn query_config_prop_bits(&self) -> Option<InputProp> {
        let size = self.select_config(InputConfigSelect::PropBits, 0);
        if size == 0 {
            return None;
        }
        assert!(size == 1);

        let data_ptr = field_ptr!(&self.config, VirtioInputConfig, data);
        InputProp::from_bits(data_ptr.cast::<u8>().read_once().unwrap())
    }

    /// Query a specific piece of information by `select` and `subsel`, return the result size.
    fn select_config(&self, select: InputConfigSelect, subsel: u8) -> usize {
        field_ptr!(&self.config, VirtioInputConfig, select)
            .write_once(&(select as u8))
            .unwrap();
        field_ptr!(&self.config, VirtioInputConfig, subsel)
            .write_once(&subsel)
            .unwrap();
        field_ptr!(&self.config, VirtioInputConfig, size)
            .read_once()
            .unwrap() as usize
    }

    fn handle_irq(&self) {
        // Returns true if there may be more events to handle
        let handle_event = |event: &EventBuf| -> bool {
            event.sync().unwrap();
            let virtio_event: VirtioInputEvent = event.read().unwrap();

            match virtio_event.event_type {
                0 => return false, // End of events
                // Keyboard events
                1 => {
                    let key_status = match virtio_event.value {
                        1 => KeyStatus::Pressed,
                        0 => KeyStatus::Released,
                        _ => return true, // Skip invalid values, continue processing
                    };

                    // Get current system time in microseconds
                    let time_in_microseconds = read_monotonic_time().as_micros() as u64;

                    // Dispatch the key event using new API
                    if let Some(device) = VIRTIO_INPUT_DEVICE.get() {
                        let key_event = InputEvent::new(
                            time_in_microseconds,
                            EventType::EvKey as u16,
                            virtio_event.code,
                            match key_status {
                                KeyStatus::Pressed => 1,
                                KeyStatus::Released => 0,
                            },
                        );
                        aster_input::submit_event(device, &key_event);

                        // Dispatch the synchronization event
                        let syn_event =
                            InputEvent::new(time_in_microseconds, EventType::EvSyn as u16, 0, 0);
                        aster_input::submit_event(device, &syn_event);
                    }

                    debug!(
                        "VirtIO Input Event: type={}, code={}, value={}, status={:?}",
                        virtio_event.event_type, virtio_event.code, virtio_event.value, key_status
                    );
                }
                // Other event types (mouse, buttons, etc.)
                _ => {
                    debug!(
                        "VirtIO Input: Unsupported event type {}, skipping",
                        virtio_event.event_type
                    );
                    return true; // Continue processing other events
                }
            }

            true
        };

        self.pop_pending_events(&handle_event);
    }

    /// Negotiate features for the device specified bits 0~23
    pub(crate) fn negotiate_features(features: u64) -> u64 {
        assert_eq!(features, 0);
        0
    }
}

impl InputDeviceTrait for InputDevice {
    fn name(&self) -> &str {
        &self.device_name
    }

    fn phys(&self) -> &str {
        &self.device_phys
    }

    fn uniq(&self) -> &str {
        "" // VirtIO devices don't have unique serial numbers
    }

    fn id(&self) -> InputId {
        self.device_id
    }

    fn capability(&self) -> &InputCapability {
        &self.capability
    }
}

/// A event table consists of many event buffers,
/// each of which is large enough to contain a `VirtioInputEvent`.
#[derive(Debug)]
struct EventTable {
    stream: DmaStream,
    num_events: usize,
}

impl EventTable {
    fn new(num_events: usize) -> Self {
        assert!(num_events * mem::size_of::<VirtioInputEvent>() <= PAGE_SIZE);

        let segment = FrameAllocOptions::new().alloc_segment(1).unwrap();

        let default_event = VirtioInputEvent::default();
        let iter = iter::repeat_n(&default_event, EVENT_SIZE);
        let nr_written = segment.write_vals(0, iter, 0).unwrap();
        assert_eq!(nr_written, EVENT_SIZE);

        let stream = DmaStream::map(segment.into(), DmaDirection::FromDevice, false).unwrap();
        Self { stream, num_events }
    }

    fn get(&self, idx: usize) -> EventBuf<'_> {
        assert!(idx < self.num_events);

        let offset = idx * EVENT_SIZE;
        SafePtr::new(&self.stream, offset)
    }

    const fn num_events(&self) -> usize {
        self.num_events
    }
}

const EVENT_SIZE: usize = core::mem::size_of::<VirtioInputEvent>();
type EventBuf<'a> = SafePtr<VirtioInputEvent, &'a DmaStream>;

impl<T, M: HasDaddr> DmaBuf for SafePtr<T, M> {
    fn len(&self) -> usize {
        core::mem::size_of::<T>()
    }
}

// impl aster_input::InputDevice for InputDevice {
//     fn register_callbacks(&self, function: &'static (dyn Fn(InputEvent) + Send + Sync)) {
//         self.callbacks.write().push(Arc::new(function))
//     }
// }

impl Debug for InputDevice {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InputDevice")
            .field("config", &self.config)
            .field("event_queue", &self.event_queue)
            .field("status_queue", &self.status_queue)
            .field("event_buf", &self.event_table)
            .field("transport", &self.transport)
            .finish()
    }
}
