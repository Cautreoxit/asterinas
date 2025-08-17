// SPDX-License-Identifier: MPL-2.0

use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::{fmt::Debug, iter, mem};

use aster_input::{
    event_type_codes::{EventTypes, KeyEvent, KeyStatus, SynEvent, RelEvent},
    InputCapability, InputDevice as InputDeviceTrait, InputEvent, InputId, RegisteredInputDevice,
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
static REGISTERED_DEVICE: spin::Once<RegisteredInputDevice> = spin::Once::new();

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
    capability: SpinLock<InputCapability>,
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

        let temp_device = Self {
            config: VirtioInputConfig::new(transport.as_mut()),
            event_queue: SpinLock::new(event_queue),
            status_queue,
            event_table,
            transport: SpinLock::new(transport),
            callbacks: RwLock::new(Vec::new()),
            device_name: "virtio_input".to_string(), // Default name, can be queried later
            device_phys: "virtio".to_string(),
            device_id: InputId::new(InputId::BUS_VIRTUAL, 0x1AF4, 0x0012, 0x0001),
            capability: SpinLock::new(capability),
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

        // Query and set device capabilities
        device.query_and_set_capabilities();

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
        let registered_device = aster_input::register_device(device.clone());

        // Store device reference for IRQ handler (convert to trait object)
        let device_trait_object: Arc<dyn InputDeviceTrait> = device;
        VIRTIO_INPUT_DEVICE.call_once(|| device_trait_object);
        REGISTERED_DEVICE.call_once(|| registered_device);

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

                    // Dispatch the key event
                    if let Some(_device) = VIRTIO_INPUT_DEVICE.get() {
                        // Map VirtIO key code to Linux KeyEvent; for now use a simple mapping
                        if let Some(linux_key) = virtio_key_to_key_event(virtio_event.code) {
                            let key_event = InputEvent::key(linux_key, key_status);
                            if let Some(registered_device) = REGISTERED_DEVICE.get() {
                                registered_device.submit_event(&key_event);

                                // Dispatch the synchronization event
                                let syn_event = InputEvent::sync(SynEvent::SynReport);
                                registered_device.submit_event(&syn_event);
                            }
                        } else {
                            debug!(
                                "VirtIO Input: unmapped key code {}, dropped",
                                virtio_event.code
                            );
                        }
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

/// Map VirtIO key codes to Linux KeyEvent enum
/// VirtIO input uses Linux key codes, so this is mostly a direct mapping
fn virtio_key_to_key_event(code: u16) -> Option<KeyEvent> {
    // VirtIO input device uses Linux input event codes directly
    // Reference: https://github.com/torvalds/linux/blob/master/include/uapi/linux/input-event-codes.h
    Some(match code {
        // Letters
        30 => KeyEvent::KeyA,
        48 => KeyEvent::KeyB,
        46 => KeyEvent::KeyC,
        32 => KeyEvent::KeyD,
        18 => KeyEvent::KeyE,
        33 => KeyEvent::KeyF,
        34 => KeyEvent::KeyG,
        35 => KeyEvent::KeyH,
        23 => KeyEvent::KeyI,
        36 => KeyEvent::KeyJ,
        37 => KeyEvent::KeyK,
        38 => KeyEvent::KeyL,
        50 => KeyEvent::KeyM,
        49 => KeyEvent::KeyN,
        24 => KeyEvent::KeyO,
        25 => KeyEvent::KeyP,
        16 => KeyEvent::KeyQ,
        19 => KeyEvent::KeyR,
        31 => KeyEvent::KeyS,
        20 => KeyEvent::KeyT,
        22 => KeyEvent::KeyU,
        47 => KeyEvent::KeyV,
        17 => KeyEvent::KeyW,
        45 => KeyEvent::KeyX,
        21 => KeyEvent::KeyY,
        44 => KeyEvent::KeyZ,

        // Numbers
        2 => KeyEvent::Key1,
        3 => KeyEvent::Key2,
        4 => KeyEvent::Key3,
        5 => KeyEvent::Key4,
        6 => KeyEvent::Key5,
        7 => KeyEvent::Key6,
        8 => KeyEvent::Key7,
        9 => KeyEvent::Key8,
        10 => KeyEvent::Key9,
        11 => KeyEvent::Key0,

        // Function keys
        59 => KeyEvent::KeyF1,
        60 => KeyEvent::KeyF2,
        61 => KeyEvent::KeyF3,
        62 => KeyEvent::KeyF4,
        63 => KeyEvent::KeyF5,
        64 => KeyEvent::KeyF6,
        65 => KeyEvent::KeyF7,
        66 => KeyEvent::KeyF8,
        67 => KeyEvent::KeyF9,
        68 => KeyEvent::KeyF10,
        87 => KeyEvent::KeyF11,
        88 => KeyEvent::KeyF12,

        // Special keys
        57 => KeyEvent::KeySpace,
        28 => KeyEvent::KeyEnter,
        1 => KeyEvent::KeyEsc,
        14 => KeyEvent::KeyBackspace,
        15 => KeyEvent::KeyTab,

        // Punctuation
        12 => KeyEvent::KeyMinus,
        13 => KeyEvent::KeyEqual,
        26 => KeyEvent::KeyLeftBrace,
        27 => KeyEvent::KeyRightBrace,
        43 => KeyEvent::KeyBackslash,
        39 => KeyEvent::KeySemicolon,
        40 => KeyEvent::KeyApostrophe,
        41 => KeyEvent::KeyGrave,
        51 => KeyEvent::KeyComma,
        52 => KeyEvent::KeyDot,
        53 => KeyEvent::KeySlash,

        // Navigation
        103 => KeyEvent::KeyUp,
        108 => KeyEvent::KeyDown,
        105 => KeyEvent::KeyLeft,
        106 => KeyEvent::KeyRight,
        102 => KeyEvent::KeyHome,
        107 => KeyEvent::KeyEnd,
        104 => KeyEvent::KeyPageUp,
        109 => KeyEvent::KeyPageDown,
        110 => KeyEvent::KeyInsert,
        111 => KeyEvent::KeyDelete,

        // Modifiers (these might not be directly mappable as they're handled differently)
        // 42 => KeyEvent::KeyLeftShift,
        // 54 => KeyEvent::KeyRightShift,
        // 29 => KeyEvent::KeyLeftCtrl,
        // 97 => KeyEvent::KeyRightCtrl,
        // 56 => KeyEvent::KeyLeftAlt,
        // 100 => KeyEvent::KeyRightAlt,

        // For now, return None for unsupported codes
        _ => return None,
    })
}

/// Convert Linux key code to our KeyEvent enum
fn linux_key_to_key_event(linux_code: u16) -> Option<KeyEvent> {
    Some(match linux_code {
        1 => KeyEvent::KeyEsc,
        2 => KeyEvent::Key1,
        3 => KeyEvent::Key2,
        4 => KeyEvent::Key3,
        5 => KeyEvent::Key4,
        6 => KeyEvent::Key5,
        7 => KeyEvent::Key6,
        8 => KeyEvent::Key7,
        9 => KeyEvent::Key8,
        10 => KeyEvent::Key9,
        11 => KeyEvent::Key0,
        12 => KeyEvent::KeyMinus,
        13 => KeyEvent::KeyEqual,
        14 => KeyEvent::KeyBackspace,
        15 => KeyEvent::KeyTab,
        16 => KeyEvent::KeyQ,
        17 => KeyEvent::KeyW,
        18 => KeyEvent::KeyE,
        19 => KeyEvent::KeyR,
        20 => KeyEvent::KeyT,
        21 => KeyEvent::KeyY,
        22 => KeyEvent::KeyU,
        23 => KeyEvent::KeyI,
        24 => KeyEvent::KeyO,
        25 => KeyEvent::KeyP,
        26 => KeyEvent::KeyLeftBrace,
        27 => KeyEvent::KeyRightBrace,
        28 => KeyEvent::KeyEnter,
        29 => KeyEvent::KeyLeftCtrl,
        30 => KeyEvent::KeyA,
        31 => KeyEvent::KeyS,
        32 => KeyEvent::KeyD,
        33 => KeyEvent::KeyF,
        34 => KeyEvent::KeyG,
        35 => KeyEvent::KeyH,
        36 => KeyEvent::KeyJ,
        37 => KeyEvent::KeyK,
        38 => KeyEvent::KeyL,
        39 => KeyEvent::KeySemicolon,
        40 => KeyEvent::KeyApostrophe,
        41 => KeyEvent::KeyGrave,
        42 => KeyEvent::KeyLeftShift,
        43 => KeyEvent::KeyBackslash,
        44 => KeyEvent::KeyZ,
        45 => KeyEvent::KeyX,
        46 => KeyEvent::KeyC,
        47 => KeyEvent::KeyV,
        48 => KeyEvent::KeyB,
        49 => KeyEvent::KeyN,
        50 => KeyEvent::KeyM,
        51 => KeyEvent::KeyComma,
        52 => KeyEvent::KeyDot,
        53 => KeyEvent::KeySlash,
        54 => KeyEvent::KeyRightShift,
        55 => KeyEvent::KeyKpAsterisk,
        56 => KeyEvent::KeyLeftAlt,
        57 => KeyEvent::KeySpace,
        58 => KeyEvent::KeyCapsLock,
        59 => KeyEvent::KeyF1,
        60 => KeyEvent::KeyF2,
        61 => KeyEvent::KeyF3,
        62 => KeyEvent::KeyF4,
        63 => KeyEvent::KeyF5,
        64 => KeyEvent::KeyF6,
        65 => KeyEvent::KeyF7,
        66 => KeyEvent::KeyF8,
        67 => KeyEvent::KeyF9,
        68 => KeyEvent::KeyF10,
        69 => KeyEvent::KeyNumLock,
        70 => KeyEvent::KeyScrollLock,
        71 => KeyEvent::KeyKp7,
        72 => KeyEvent::KeyKp8,
        73 => KeyEvent::KeyKp9,
        74 => KeyEvent::KeyKpMinus,
        75 => KeyEvent::KeyKp4,
        76 => KeyEvent::KeyKp5,
        77 => KeyEvent::KeyKp6,
        78 => KeyEvent::KeyKpPlus,
        79 => KeyEvent::KeyKp1,
        80 => KeyEvent::KeyKp2,
        81 => KeyEvent::KeyKp3,
        82 => KeyEvent::KeyKp0,
        83 => KeyEvent::KeyKpDot,
        87 => KeyEvent::KeyF11,
        88 => KeyEvent::KeyF12,
        96 => KeyEvent::KeyKpEnter,
        97 => KeyEvent::KeyRightCtrl,
        98 => KeyEvent::KeyKpSlash,
        100 => KeyEvent::KeyRightAlt,
        102 => KeyEvent::KeyHome,
        103 => KeyEvent::KeyUp,
        104 => KeyEvent::KeyPageUp,
        105 => KeyEvent::KeyLeft,
        106 => KeyEvent::KeyRight,
        107 => KeyEvent::KeyEnd,
        108 => KeyEvent::KeyDown,
        109 => KeyEvent::KeyPageDown,
        110 => KeyEvent::KeyInsert,
        111 => KeyEvent::KeyDelete,
        113 => KeyEvent::KeyMute,
        114 => KeyEvent::KeyVolumeDown,
        115 => KeyEvent::KeyVolumeUp,
        125 => KeyEvent::KeyLeftMeta,
        126 => KeyEvent::KeyRightMeta,
        139 => KeyEvent::KeyMenu,
        _ => return None,
    })
}

/// Convert Linux relative axis code to our RelEvent enum
fn linux_rel_to_rel_event(linux_code: u16) -> Option<RelEvent> {
    
    Some(match linux_code {
        0x00 => RelEvent::RelX,
        0x01 => RelEvent::RelY,
        0x02 => RelEvent::RelZ,
        0x03 => RelEvent::RelRx,
        0x04 => RelEvent::RelRy,
        0x05 => RelEvent::RelRz,
        0x06 => RelEvent::RelHWheel,
        0x07 => RelEvent::RelDial,
        0x08 => RelEvent::RelWheel,
        0x09 => RelEvent::RelMisc,
        0x0a => RelEvent::RelReserved,
        0x0b => RelEvent::RelWheelHiRes,
        0x0c => RelEvent::RelHWheelHiRes,
        _ => return None,
    })
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
        // TODO: This needs to be restructured properly
        // For now, we'll return a reference to the locked capability
        // This is a temporary solution until we can properly restructure the trait
        let capability = self.capability.disable_irq().lock();
        // This is not ideal - we're leaking the lock, but it's temporary
        Box::leak(Box::new(capability))
    }
}

impl InputDevice {
    /// Query device capabilities from VirtIO config space and set them
    fn query_and_set_capabilities(&self) {
        use aster_input::event_type_codes::EventTypes;
        
        // Query supported event types (EV_*)
        let ev_syn = self.query_ev_bits(EventTypes::SYN.as_u16());
        let ev_key = self.query_ev_bits(EventTypes::KEY.as_u16());
        let ev_rel = self.query_ev_bits(EventTypes::REL.as_u16());
        
        let mut capability = self.capability.disable_irq().lock();
        
        // Set event type capabilities
        if ev_syn.is_some() {
            capability.set_supported_event_type(EventTypes::SYN);
        }
        if ev_key.is_some() {
            capability.set_supported_event_type(EventTypes::KEY);
        }
        if ev_rel.is_some() {
            capability.set_supported_event_type(EventTypes::REL);
        }
        
        // Query and set key capabilities
        if let Some(ref key_bits) = ev_key {
            for bit in 0..key_bits.len() * 8 {
                if key_bits[bit / 8] & (1 << (bit % 8)) != 0 {
                    if let Some(key_event) = linux_key_to_key_event(bit as u16) {
                        capability.set_supported_key(key_event);
                    }
                }
            }
        }
        
        // Query and set relative axis capabilities
        if let Some(ref rel_bits) = ev_rel {
            for bit in 0..rel_bits.len() * 8 {
                if rel_bits[bit / 8] & (1 << (bit % 8)) != 0 {
                    if let Some(rel_event) = linux_rel_to_rel_event(bit as u16) {
                        capability.set_supported_relative_axis(rel_event);
                    }
                }
            }
        }
        
        info!("VirtIO input device capabilities set: SYN={}, KEY={}, REL={}", 
              ev_syn.is_some(), ev_key.is_some(), ev_rel.is_some());
    }
    
    /// Query event bits for a specific event type
    fn query_ev_bits(&self, event_type: u16) -> Option<Vec<u8>> {
        let size = self.select_config(InputConfigSelect::EvBits, event_type as u8);
        if size == 0 {
            return None;
        }
        
        let mut bits = Vec::with_capacity(size);
        let data_ptr = field_ptr!(&self.config, VirtioInputConfig, data).cast::<u8>();
        for i in 0..size {
            let mut ptr = data_ptr.clone();
            ptr.byte_add(i);
            bits.push(ptr.read_once().unwrap());
        }
        Some(bits)
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
