use std::time::{Duration, Instant};

use poe_wayfinder_core::controller::gamepad_match::PadFamily;
use poe_wayfinder_core::controller::gamepad_nav;
use poe_wayfinder_core::controller::pad_script::Script;
use poe_wayfinder_core::controller::sony_pad;

const SLOTS: u32 = 4;
const RESCAN: Duration = Duration::from_secs(5);

#[cfg_attr(test, mockall::automock)]
pub trait Gamepad {
    fn buttons(&mut self) -> Option<u16>;

    fn direction(&self) -> u16;

    fn family(&self) -> PadFamily;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub buttons: u16,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug)]
struct Slots {
    connected: [bool; SLOTS as usize],
    next_scan: Option<Instant>,
    direction: u16,
}

impl Slots {
    fn new() -> Self {
        Self {
            connected: [false; SLOTS as usize],
            next_scan: None,
            direction: 0,
        }
    }

    fn poll(&mut self, now: Instant, mut read: impl FnMut(u32) -> Option<Reading>) -> Option<u16> {
        let scanning = match self.next_scan {
            Some(at) => now >= at,
            None => true,
        };

        if scanning {
            self.next_scan = Some(now + RESCAN);
        }

        let mut held: Option<u16> = None;

        self.direction = 0;

        for slot in 0..SLOTS {
            if !self.connected[slot as usize] && !scanning {
                continue;
            }

            match read(slot) {
                Some(reading) => {
                    self.connected[slot as usize] = true;
                    held = Some(held.unwrap_or(0) | reading.buttons);
                    self.direction |= gamepad_nav::stick_direction(reading.x, reading.y);
                }
                None => self.connected[slot as usize] = false,
            }
        }

        held
    }
}

pub struct XInputPads {
    slots: Slots,
}

impl XInputPads {
    pub fn new() -> Self {
        Self {
            slots: Slots::new(),
        }
    }
}

impl Default for XInputPads {
    fn default() -> Self {
        Self::new()
    }
}

impl Gamepad for XInputPads {
    fn buttons(&mut self) -> Option<u16> {
        self.slots.poll(Instant::now(), read_slot)
    }

    fn direction(&self) -> u16 {
        self.slots.direction
    }

    fn family(&self) -> PadFamily {
        PadFamily::Xbox
    }
}

#[cfg(windows)]
fn read_slot(slot: u32) -> Option<Reading> {
    use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};

    let mut state = XINPUT_STATE::default();

    if unsafe { XInputGetState(slot, &mut state) } != 0 {
        return None;
    }

    Some(Reading {
        buttons: state.Gamepad.wButtons.0,
        x: f32::from(state.Gamepad.sThumbLX) / 32767.0,
        y: -f32::from(state.Gamepad.sThumbLY) / 32767.0,
    })
}

#[cfg(not(windows))]
fn read_slot(_slot: u32) -> Option<Reading> {
    None
}

pub mod hid {
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct HidDevice {
        pub path: String,
        pub vendor: u16,
        pub product: u16,
        pub report_len: usize,
    }

    pub const INPUT_BUFFERS: u32 = 2;

    #[cfg(windows)]
    mod win {
        use super::{HidDevice, INPUT_BUFFERS};

        use windows::core::PCWSTR;
        use windows::Win32::Devices::DeviceAndDriverInstallation::{
            SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
            SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
            SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
        };
        use windows::Win32::Devices::HumanInterfaceDevice::{
            HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetHidGuid, HidD_GetPreparsedData,
            HidD_SetNumInputBuffers, HidP_GetCaps, HidP_GetUsageValue, HidP_GetUsages, HidP_Input,
            HidP_MaxUsageListLength, HIDD_ATTRIBUTES, HIDP_CAPS, PHIDP_PREPARSED_DATA,
        };
        use windows::Win32::Foundation::{CloseHandle, ERROR_IO_PENDING, HANDLE};
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, ReadFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING,
        };
        use windows::Win32::System::Threading::CreateEventW;
        use windows::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};

        const GENERIC_READ_ACCESS: u32 = 0x8000_0000;

        pub fn list(usage_page: u16, usage: u16) -> Vec<HidDevice> {
            let guid = unsafe { HidD_GetHidGuid() };

            let Ok(set) = (unsafe {
                SetupDiGetClassDevsW(
                    Some(&guid),
                    PCWSTR::null(),
                    None,
                    DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
                )
            }) else {
                return Vec::new();
            };

            let mut found = Vec::new();
            let mut index = 0;

            loop {
                let mut interface = SP_DEVICE_INTERFACE_DATA {
                    cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                    ..Default::default()
                };

                if unsafe { SetupDiEnumDeviceInterfaces(set, None, &guid, index, &mut interface) }
                    .is_err()
                {
                    break;
                }

                index += 1;

                let Some(path) = detail_path(set, &interface) else {
                    continue;
                };

                if let Some(device) = describe(&path, usage_page, usage) {
                    found.push(device);
                }
            }

            let _ = unsafe { SetupDiDestroyDeviceInfoList(set) };

            found
        }

        fn detail_path(
            set: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
            interface: &SP_DEVICE_INTERFACE_DATA,
        ) -> Option<Vec<u16>> {
            let mut needed = 0u32;

            let _ = unsafe {
                SetupDiGetDeviceInterfaceDetailW(set, interface, None, 0, Some(&mut needed), None)
            };

            if needed == 0 {
                return None;
            }

            let mut buffer = vec![0u8; needed as usize];
            let detail = buffer.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;

            unsafe {
                (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            }

            unsafe {
                SetupDiGetDeviceInterfaceDetailW(set, interface, Some(detail), needed, None, None)
            }
            .ok()?;

            let start = unsafe { (*detail).DevicePath.as_ptr() };
            let mut wide = Vec::new();
            let mut at = 0isize;

            loop {
                let unit = unsafe { *start.offset(at) };

                wide.push(unit);

                if unit == 0 {
                    break;
                }

                at += 1;
            }

            Some(wide)
        }

        fn describe(path: &[u16], usage_page: u16, usage: u16) -> Option<HidDevice> {
            let handle = open(path)?;

            let mut attributes = HIDD_ATTRIBUTES {
                Size: size_of::<HIDD_ATTRIBUTES>() as u32,
                ..Default::default()
            };

            let read = unsafe { HidD_GetAttributes(handle, &mut attributes) };
            let caps = caps_of(handle);

            let _ = unsafe { CloseHandle(handle) };

            let caps = caps?;

            if !read || caps.UsagePage != usage_page || caps.Usage != usage {
                return None;
            }

            Some(HidDevice {
                path: String::from_utf16_lossy(&path[..path.len().saturating_sub(1)]),
                vendor: attributes.VendorID,
                product: attributes.ProductID,
                report_len: caps.InputReportByteLength as usize,
            })
        }

        fn caps_of(handle: HANDLE) -> Option<HIDP_CAPS> {
            let mut preparsed = PHIDP_PREPARSED_DATA::default();

            if !unsafe { HidD_GetPreparsedData(handle, &mut preparsed) } {
                return None;
            }

            let mut caps = HIDP_CAPS::default();
            let status = unsafe { HidP_GetCaps(preparsed, &mut caps) };

            let _ = unsafe { HidD_FreePreparsedData(preparsed) };

            match status.is_ok() {
                true => Some(caps),
                false => None,
            }
        }

        fn open(path: &[u16]) -> Option<HANDLE> {
            let wide = PCWSTR(path.as_ptr());

            let opened = |access: u32| unsafe {
                CreateFileW(
                    wide,
                    access,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    None,
                )
            };

            match opened(GENERIC_READ_ACCESS) {
                Ok(handle) => Some(handle),
                Err(_) => opened(0).ok(),
            }
        }

        pub struct HidReader {
            handle: HANDLE,
            overlapped: Box<OVERLAPPED>,
            buffer: Vec<u8>,
            pending: bool,
            preparsed: PHIDP_PREPARSED_DATA,
        }

        impl HidReader {
            pub fn open(device: &HidDevice) -> Option<Self> {
                let mut path: Vec<u16> = device.path.encode_utf16().collect();

                path.push(0);

                let handle = open(&path)?;
                let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.ok()?;

                let _ = unsafe { HidD_SetNumInputBuffers(handle, INPUT_BUFFERS) };

                let mut overlapped = Box::new(OVERLAPPED::default());

                overlapped.hEvent = event;

                let mut preparsed = PHIDP_PREPARSED_DATA::default();

                if !unsafe { HidD_GetPreparsedData(handle, &mut preparsed) } {
                    preparsed = PHIDP_PREPARSED_DATA::default();
                }

                let mut reader = Self {
                    handle,
                    overlapped,
                    buffer: vec![0u8; device.report_len.max(64)],
                    pending: false,
                    preparsed,
                };

                reader.queue()?;

                Some(reader)
            }

            fn queue(&mut self) -> Option<()> {
                if self.pending {
                    return Some(());
                }

                let result = unsafe {
                    ReadFile(
                        self.handle,
                        Some(&mut self.buffer),
                        None,
                        Some(self.overlapped.as_mut()),
                    )
                };

                match result {
                    Ok(()) => {
                        self.pending = true;

                        Some(())
                    }
                    Err(err) if err.code() == ERROR_IO_PENDING.to_hresult() => {
                        self.pending = true;

                        Some(())
                    }
                    Err(_) => None,
                }
            }

            pub fn newest(&mut self) -> Option<Vec<u8>> {
                let mut latest = None;

                for _ in 0..INPUT_BUFFERS + 1 {
                    let mut got = 0u32;

                    if !self.pending {
                        self.queue()?;
                    }

                    if unsafe {
                        GetOverlappedResult(self.handle, self.overlapped.as_ref(), &mut got, false)
                    }
                    .is_err()
                    {
                        break;
                    }

                    self.pending = false;

                    if got > 0 {
                        latest = Some(self.buffer[..got as usize].to_vec());
                    }

                    if self.queue().is_none() {
                        break;
                    }
                }

                latest
            }
        }

        impl HidReader {
            pub fn decode_by_descriptor(&self, report: &[u8]) -> Option<u16> {
                use poe_wayfinder_core::controller::sony_pad;

                if self.preparsed.0 == 0 {
                    return None;
                }

                let room = unsafe {
                    HidP_MaxUsageListLength(HidP_Input, Some(sony_pad::BUTTON_PAGE), self.preparsed)
                };

                if room == 0 {
                    return None;
                }

                let mut usages = vec![0u16; room as usize];
                let mut held = room;
                let mut copy = report.to_vec();

                let status = unsafe {
                    HidP_GetUsages(
                        HidP_Input,
                        sony_pad::BUTTON_PAGE,
                        None,
                        usages.as_mut_ptr(),
                        &mut held,
                        self.preparsed,
                        &mut copy,
                    )
                };

                if status.is_err() {
                    return None;
                }

                let mut mask = 0u16;

                for usage in &usages[..held.min(room) as usize] {
                    mask |= sony_pad::bit_for_button(*usage);
                }

                let mut hat = 0u32;

                let status = unsafe {
                    HidP_GetUsageValue(
                        HidP_Input,
                        sony_pad::GAMEPAD_USAGE_PAGE,
                        None,
                        sony_pad::HAT_USAGE,
                        &mut hat,
                        self.preparsed,
                        report,
                    )
                };

                if status.is_ok() {
                    mask |= sony_pad::hat_to_mask(hat as u8);
                }

                Some(mask)
            }
        }

        impl Drop for HidReader {
            fn drop(&mut self) {
                if self.preparsed.0 != 0 {
                    let _ = unsafe { HidD_FreePreparsedData(self.preparsed) };
                }

                let _ = unsafe { CancelIo(self.handle) };
                let _ = unsafe { CloseHandle(self.overlapped.hEvent) };
                let _ = unsafe { CloseHandle(self.handle) };
            }
        }
    }

    #[cfg(windows)]
    pub use win::{list, HidReader};

    #[cfg(not(windows))]
    pub fn list(_usage_page: u16, _usage: u16) -> Vec<HidDevice> {
        Vec::new()
    }

    #[cfg(not(windows))]
    pub struct HidReader;

    #[cfg(not(windows))]
    impl HidReader {
        pub fn open(_device: &HidDevice) -> Option<Self> {
            None
        }

        pub fn newest(&mut self) -> Option<Vec<u8>> {
            None
        }

        pub fn decode_by_descriptor(&self, _report: &[u8]) -> Option<u16> {
            None
        }
    }
}

pub struct ScriptedPad {
    script: Script,
    poll: usize,
    held: u16,
}

impl ScriptedPad {
    pub fn new(script: Script) -> Self {
        Self {
            script,
            poll: 0,
            held: 0,
        }
    }
}

impl Gamepad for ScriptedPad {
    fn buttons(&mut self) -> Option<u16> {
        let held = self.script.at(self.poll).unwrap_or(0);

        self.poll += 1;
        self.held = held;

        Some(held)
    }

    fn direction(&self) -> u16 {
        0
    }

    fn family(&self) -> PadFamily {
        PadFamily::PlayStation
    }
}

pub fn known_devices() -> Vec<hid::HidDevice> {
    hid::list(sony_pad::GAMEPAD_USAGE_PAGE, sony_pad::GAMEPAD_USAGE)
        .into_iter()
        .filter(|device| sony_pad::is_known(device.vendor, device.product))
        .collect()
}

struct Pad {
    path: String,
    product: u16,
    reader: hid::HidReader,
    held: u16,
    pushed: u16,
}

pub struct SonyPads {
    pads: Vec<Pad>,
    next_scan: Option<std::time::Instant>,
    direction: u16,
}

impl SonyPads {
    pub fn new() -> Self {
        Self {
            pads: Vec::new(),
            next_scan: None,
            direction: 0,
        }
    }

    fn rescan(&mut self, now: std::time::Instant) {
        let due = match self.next_scan {
            Some(at) => now >= at,
            None => true,
        };

        if !due {
            return;
        }

        self.next_scan = Some(now + RESCAN);

        let present = known_devices();

        self.pads
            .retain(|pad| present.iter().any(|device| device.path == pad.path));

        for device in present {
            if self.pads.iter().any(|pad| pad.path == device.path) {
                continue;
            }

            let Some(reader) = hid::HidReader::open(&device) else {
                continue;
            };

            self.pads.push(Pad {
                path: device.path,
                product: device.product,
                reader,
                held: 0,
                pushed: 0,
            });
        }
    }
}

impl Default for SonyPads {
    fn default() -> Self {
        Self::new()
    }
}

impl Gamepad for SonyPads {
    fn buttons(&mut self) -> Option<u16> {
        self.rescan(std::time::Instant::now());

        if self.pads.is_empty() {
            return None;
        }

        let mut held = 0u16;

        let mut direction = 0;

        for pad in &mut self.pads {
            if let Some(report) = pad.reader.newest() {
                if let Some(mask) = sony_pad::parse_report(pad.product, &report) {
                    pad.held = mask;
                }

                if let Some((x, y)) = sony_pad::parse_left_stick(pad.product, &report) {
                    pad.pushed = gamepad_nav::stick_direction(x, y);
                }
            }

            held |= pad.held;
            direction |= pad.pushed;
        }

        self.direction = direction;

        Some(held)
    }

    fn direction(&self) -> u16 {
        self.direction
    }

    fn family(&self) -> PadFamily {
        PadFamily::PlayStation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_hid_devices_answers_rather_than_panicking_on_any_machine() {
        let found = hid::list(0x01, 0x05);

        for device in &found {
            assert_ne!(device.path, "", "a device with no path cannot be opened");
            assert_ne!(device.report_len, 0, "a zero length report reads nothing");
        }

        assert!(found.is_empty() || cfg!(windows), "only windows enumerates");
    }

    #[test]
    fn the_input_buffer_count_is_small_so_a_press_is_not_read_stale() {
        let accepted = 2..=4;

        assert!(
            accepted.contains(&hid::INPUT_BUFFERS),
            "windows refuses fewer than two and a deep buffer hands us old reports"
        );
    }

    fn reading(buttons: u16) -> Reading {
        Reading {
            buttons,
            x: 0.0,
            y: 0.0,
        }
    }

    fn at(seconds: u64) -> Instant {
        Instant::now() + Duration::from_secs(seconds)
    }

    #[test]
    fn a_sony_pad_is_named_by_the_buttons_printed_on_it() {
        assert_eq!(SonyPads::new().family(), PadFamily::PlayStation);
    }

    #[test]
    fn a_pad_source_reads_nothing_exactly_when_no_pad_is_plugged_in() {
        let plugged_in = !known_devices().is_empty();

        assert_eq!(
            SonyPads::new().buttons().is_some(),
            plugged_in,
            "this must hold on a machine with a pad and on one without"
        );
    }

    #[test]
    fn only_sony_gamepads_are_claimed_out_of_every_hid_device() {
        for device in known_devices() {
            assert!(
                sony_pad::is_known(device.vendor, device.product),
                "{device:?}"
            );
        }
    }

    #[test]
    fn an_xinput_pad_is_named_by_its_own_button_names() {
        assert_eq!(XInputPads::new().family(), PadFamily::Xbox);
    }

    #[test]
    fn no_pad_in_any_slot_reads_as_nothing_connected() {
        let mut slots = Slots::new();

        assert_eq!(slots.poll(at(0), |_| None), None);
    }

    #[test]
    fn a_pad_in_one_slot_reports_the_buttons_it_holds() {
        let mut slots = Slots::new();

        let held = slots.poll(at(0), |slot| match slot {
            1 => Some(reading(0x1000)),
            _ => None,
        });

        assert_eq!(held, Some(0x1000));
    }

    #[test]
    fn two_pads_are_read_as_one_set_of_buttons() {
        let mut slots = Slots::new();

        let held = slots.poll(at(0), |slot| match slot {
            0 => Some(reading(0x0100)),
            2 => Some(reading(0x8000)),
            _ => None,
        });

        assert_eq!(held, Some(0x0100 | 0x8000));
    }

    #[test]
    fn a_connected_pad_at_rest_is_still_connected_rather_than_gone() {
        let mut slots = Slots::new();

        slots.poll(at(0), |_| Some(reading(0x1000)));

        assert_eq!(slots.poll(at(1), |_| Some(reading(0))), Some(0));
    }

    #[test]
    fn an_empty_slot_is_left_alone_between_scans_because_reading_one_is_expensive() {
        let mut slots = Slots::new();
        let mut reads = 0;

        slots.poll(at(0), |_| None);
        slots.poll(at(1), |_| {
            reads += 1;

            None
        });

        assert_eq!(reads, 0);
    }

    #[test]
    fn every_slot_is_read_again_once_the_rescan_is_due() {
        let mut slots = Slots::new();
        let mut reads = 0u32;

        slots.poll(at(0), |_| None);
        slots.poll(at(6), |_| {
            reads += 1;

            None
        });

        assert_eq!(reads, SLOTS);
    }

    #[test]
    fn a_known_pad_is_read_every_poll_without_waiting_for_a_scan() {
        let mut slots = Slots::new();
        let mut reads = 0;

        slots.poll(at(0), |slot| match slot {
            3 => Some(reading(0)),
            _ => None,
        });

        slots.poll(at(1), |_| {
            reads += 1;

            Some(reading(0))
        });

        assert_eq!(reads, 1);
    }

    #[test]
    fn a_pad_unplugged_mid_session_drops_back_to_nothing_connected() {
        let mut slots = Slots::new();

        slots.poll(at(0), |_| Some(reading(0x1000)));

        assert_eq!(slots.poll(at(1), |_| None), None);
        assert_eq!(slots.poll(at(2), |_| Some(reading(0x1000))), None);
    }
}
