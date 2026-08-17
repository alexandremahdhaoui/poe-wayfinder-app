use std::process::ExitCode;

#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use crate::adapter::game_data_adapter::GameTables;
#[cfg(windows)]
use crate::logging::{Logger, Value};
use crate::types::Hotkey;

#[cfg(windows)]
use poe_wayfinder_core::types::GameVersion;

pub fn list_windows() -> ExitCode {
    #[cfg(windows)]
    {
        let titles = crate::adapter::game_window_adapter::visible_window_titles();

        println!("Visible windows, one per line.");
        println!("Copy the game's line into --window-title, quotes included.\n");

        for title in &titles {
            println!("  {title:?}");
        }

        if titles.is_empty() {
            println!("  (none, which should be impossible on a running desktop)");
        }

        ExitCode::SUCCESS
    }

    #[cfg(not(windows))]
    {
        eprintln!("poe-wayfinder: --list-windows only works on Windows.");

        ExitCode::FAILURE
    }
}

#[cfg(windows)]
pub fn self_test_hotkey() -> ExitCode {
    use crate::driver::hotkey_driver::HotkeyDriver;
    use crate::types::Hotkey;

    const COMBINATION: &str = "Ctrl+Alt+Shift+F24";

    let log = Logger::new("info", "poe-wayfinder");

    let Ok(hotkey) = Hotkey::parse(COMBINATION) else {
        log.error("the self test hotkey does not parse", &[]);

        return ExitCode::FAILURE;
    };

    let hotkeys = match HotkeyDriver::start(&hotkey) {
        Ok(hotkeys) => hotkeys,
        Err(err) => {
            log.error(
                "registering the self test hotkey",
                &[("error", Value::Str(crate::util::error_chain::render(&err)))],
            );

            return ExitCode::FAILURE;
        }
    };

    log.info("registered", &[("hotkey", Value::Str(COMBINATION.into()))]);

    if !hotkeys.simulate_press() {
        log.error("could not post to the hotkey thread", &[]);

        return ExitCode::FAILURE;
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(2);

    while std::time::Instant::now() < deadline {
        if hotkeys.fired() {
            log.info(
                "a press reached the frame loop. Registration, message loop and \
                 channel all work. Whether Windows hands over a real key press \
                 depends on privilege, which the startup check reports.",
                &[],
            );

            return ExitCode::SUCCESS;
        }

        std::thread::sleep(Duration::from_millis(25));
    }

    log.error(
        "the press never reached the frame loop. The message loop or the \
         channel is broken, and the hotkey would do nothing in game.",
        &[],
    );

    ExitCode::FAILURE
}

#[cfg(not(windows))]
pub fn self_test_hotkey() -> ExitCode {
    eprintln!("poe-wayfinder: --self-test-hotkey only works on Windows.");

    ExitCode::FAILURE
}

#[cfg(windows)]
pub fn fake_game(title: &str, seconds: u64, item: &str) -> ExitCode {
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::adapter::clipboard_adapter::{Clipboard, SystemClipboard};

    use crate::driver::hook_driver::HookDriver;
    use poe_wayfinder_core::controller::hotkey_match::Binding;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;

    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, PeekMessageW, RegisterClassW,
        SetForegroundWindow, ShowWindow, TranslateMessage, CW_USEDEFAULT, HWND_TOPMOST, MSG,
        PM_REMOVE, SW_SHOW, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    };

    static COPIED: AtomicBool = AtomicBool::new(false);

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        DefWindowProcW(window, message, wparam, lparam)
    }

    let class = w!("PoeWayfinderFakeGame");

    let module = match unsafe { GetModuleHandleW(None) } {
        Ok(module) => module,
        Err(err) => {
            eprintln!("fakegame: getting the module handle: {err}");

            return ExitCode::FAILURE;
        }
    };

    let wnd_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: module.into(),
        lpszClassName: class,
        ..Default::default()
    };

    if unsafe { RegisterClassW(&wnd_class) } == 0 {
        eprintln!("fakegame: registering the window class");

        return ExitCode::FAILURE;
    }

    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();

    let window = unsafe {
        CreateWindowExW(
            Default::default(),
            class,
            PCWSTR(wide.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            800,
            600,
            None,
            None,
            Some(module.into()),
            None,
        )
    };

    let Ok(window) = window else {
        eprintln!("fakegame: creating the window");

        return ExitCode::FAILURE;
    };

    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(window));
    }

    let _ = HWND_TOPMOST;

    println!("fakegame: window \"{title}\" is up for {seconds}s.");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);

    let copy_bindings: Vec<Binding> = [false, true]
        .into_iter()
        .map(|alt| Binding {
            code: 0x43,
            modifiers: poe_wayfinder_core::controller::hotkey_match::Modifiers {
                ctrl: true,
                alt,
                ..Default::default()
            },
        })
        .collect();

    let mut copy_watch = match HookDriver::start(copy_bindings) {
        Ok(watch) => watch,
        Err(err) => {
            eprintln!("fakegame: watching for Ctrl+C: {err}");

            return ExitCode::FAILURE;
        }
    };

    while std::time::Instant::now() < deadline {
        let mut message = MSG::default();

        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        if copy_watch.fired().is_some() {
            let wrote = SystemClipboard::new().and_then(|mut c| c.write(item));

            match wrote {
                Ok(()) => {
                    COPIED.store(true, Ordering::SeqCst);

                    println!("fakegame: answered Ctrl+C with {} bytes.", item.len());
                }
                Err(err) => eprintln!("fakegame: writing the clipboard: {err}"),
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    copy_watch.stop();

    if COPIED.load(Ordering::SeqCst) {
        println!("fakegame: the overlay asked for a copy.");

        ExitCode::SUCCESS
    } else {
        eprintln!(
            "fakegame: the overlay never pressed Ctrl+C, with or without the show mods key held."
        );

        ExitCode::FAILURE
    }
}

#[cfg(not(windows))]
pub fn fake_game(_title: &str, _seconds: u64, _item: &str) -> ExitCode {
    eprintln!("--fake-game only works on Windows.");

    ExitCode::FAILURE
}

#[cfg(windows)]
pub fn press_hotkey(hotkey: &Hotkey) -> ExitCode {
    use crate::types::Modifier;

    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY,
    };

    let Some(code) = crate::driver::hotkey_driver::virtual_key_code(hotkey.key()) else {
        eprintln!("poe-wayfinder: {hotkey} has no Windows key code.");

        return ExitCode::FAILURE;
    };

    fn key(code: u16, down: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(code),
                    wScan: 0,
                    dwFlags: if down {
                        KEYBD_EVENT_FLAGS(0)
                    } else {
                        KEYEVENTF_KEYUP
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let modifier_code = |m: &Modifier| match m {
        Modifier::Ctrl => 0x11,
        Modifier::Alt => 0x12,
        Modifier::Shift => 0x10,
        Modifier::Meta => 0x5B,
    };

    let mut presses: Vec<INPUT> = hotkey
        .modifiers()
        .iter()
        .map(|m| key(modifier_code(m), true))
        .collect();

    presses.push(key(code, true));
    presses.push(key(code, false));

    for m in hotkey.modifiers().iter().rev() {
        presses.push(key(modifier_code(m), false));
    }

    let sent = unsafe { SendInput(&presses, std::mem::size_of::<INPUT>() as i32) };

    if sent as usize != presses.len() {
        eprintln!(
            "poe-wayfinder: only {sent} of {} events were accepted.",
            presses.len()
        );

        return ExitCode::FAILURE;
    }

    eprintln!("poe-wayfinder: pressed {hotkey}.");

    ExitCode::SUCCESS
}

#[cfg(not(windows))]
pub fn press_hotkey(_hotkey: &Hotkey) -> ExitCode {
    eprintln!("poe-wayfinder: --press-hotkey only works on Windows.");

    ExitCode::FAILURE
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn hook_modifiers(hotkey: &Hotkey) -> poe_wayfinder_core::controller::hotkey_match::Modifiers {
    use crate::types::Modifier;
    use poe_wayfinder_core::controller::hotkey_match::Modifiers;

    let has = |wanted: Modifier| hotkey.modifiers().contains(&wanted);

    Modifiers {
        ctrl: has(Modifier::Ctrl),
        alt: has(Modifier::Alt),
        shift: has(Modifier::Shift),
        meta: has(Modifier::Meta),
    }
}

#[cfg(windows)]
pub fn self_test_hook() -> ExitCode {
    use crate::driver::hook_driver::HookDriver;
    use poe_wayfinder_core::controller::hotkey_match::Binding;
    use poe_wayfinder_core::controller::hotkey_match::Modifiers;

    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY,
    };

    const F24: u16 = 0x87;
    const CTRL: u16 = 0x11;
    const ALT: u16 = 0x12;
    const SHIFT: u16 = 0x10;

    let log = Logger::new("info", "poe-wayfinder");

    let wanted = Modifiers {
        ctrl: true,
        alt: true,
        shift: true,
        meta: false,
    };

    let mut hook = match HookDriver::start(vec![Binding {
        code: F24,
        modifiers: wanted,
    }]) {
        Ok(hook) => hook,
        Err(err) => {
            log.error(
                "installing the keyboard hook",
                &[("error", Value::Str(crate::util::error_chain::render(&err)))],
            );

            return ExitCode::FAILURE;
        }
    };

    log.info(
        "hook installed",
        &[("hotkey", Value::Str("Ctrl+Alt+Shift+F24".into()))],
    );

    fn key(code: u16, down: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(code),
                    wScan: 0,
                    dwFlags: if down {
                        KEYBD_EVENT_FLAGS(0)
                    } else {
                        KEYEVENTF_KEYUP
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let presses = [
        key(CTRL, true),
        key(ALT, true),
        key(SHIFT, true),
        key(F24, true),
        key(F24, false),
        key(SHIFT, false),
        key(ALT, false),
        key(CTRL, false),
    ];

    let sent = unsafe { SendInput(&presses, std::mem::size_of::<INPUT>() as i32) };

    if sent as usize != presses.len() {
        log.error(
            "the key presses were not accepted. Something is blocking injected \
             input, usually another tool or a privilege mismatch.",
            &[("sent", Value::Int(sent as i64))],
        );

        hook.stop();

        return ExitCode::FAILURE;
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(2);

    while std::time::Instant::now() < deadline {
        if hook.fired().is_some() {
            log.info(
                "a real key press reached the hook and matched. The whole press \
                 path works without anyone touching the keyboard.",
                &[],
            );

            hook.stop();

            return ExitCode::SUCCESS;
        }

        std::thread::sleep(Duration::from_millis(25));
    }

    log.error(
        "the keys were pressed and the hook never matched. The hook, the \
         modifier tracking or the match is broken.",
        &[],
    );

    hook.stop();

    ExitCode::FAILURE
}

#[cfg(not(windows))]
pub fn self_test_hook() -> ExitCode {
    eprintln!("poe-wayfinder: --self-test-hook only works on Windows.");

    ExitCode::FAILURE
}

#[cfg(windows)]
fn report_hotkey_outlook(
    window: &crate::adapter::game_window_adapter::GameWindowAdapter,
    log: &Logger,
) {
    use crate::adapter::elevation_adapter::{own_elevation, window_elevation};
    use poe_wayfinder_core::controller::elevation::{
        advice, hotkey_outlook, is_blocking, Elevation,
    };

    let overlay = own_elevation();

    let game = match window.raw_handle() {
        Some(handle) => window_elevation(handle, overlay == Elevation::Elevated),
        None => Elevation::Unknown,
    };

    let outlook = hotkey_outlook(overlay, game);

    let fields = [
        ("overlay", Value::Str(format!("{overlay:?}"))),
        ("game", Value::Str(format!("{game:?}"))),
        ("outlook", Value::Str(format!("{outlook:?}"))),
    ];

    match advice(outlook) {
        Some(text) if is_blocking(outlook) => {
            log.error(text, &fields);
        }
        Some(text) => log.info(text, &fields),
        None => log.info("the hotkey can reach this process", &fields),
    }
}

#[cfg(windows)]
pub fn check_clipboard_now(game: GameVersion, data: &GameTables, log: &Logger) -> ExitCode {
    use crate::adapter::clipboard_adapter::{Clipboard, SystemClipboard};
    use poe_wayfinder_core::controller::overlay::{clipboard_kind, ClipboardKind};
    use poe_wayfinder_core::controller::price_check::{price_check, PriceCheckOptions};

    let mut clipboard = match SystemClipboard::new() {
        Ok(clipboard) => clipboard,
        Err(err) => {
            log.error(
                "opening the clipboard",
                &[("error", Value::Str(crate::util::error_chain::render(&err)))],
            );

            return ExitCode::FAILURE;
        }
    };

    let text = match clipboard.read() {
        Ok(Some(text)) => text,
        Ok(None) => {
            log.warn("the clipboard holds no text", &[]);

            return ExitCode::FAILURE;
        }
        Err(err) => {
            log.error(
                "reading the clipboard",
                &[("error", Value::Str(crate::util::error_chain::render(&err)))],
            );

            return ExitCode::FAILURE;
        }
    };

    let kind = clipboard_kind(&text);

    log.info(
        "read the clipboard",
        &[
            ("bytes", Value::Int(text.len() as i64)),
            ("kind", Value::Str(format!("{kind:?}"))),
        ],
    );

    if kind == ClipboardKind::NotAnItem {
        log.warn(
            "the clipboard does not hold a copied item. Copy one in game first.",
            &[],
        );

        return ExitCode::FAILURE;
    }

    match price_check(&text, data, &PriceCheckOptions::new(game)) {
        Ok(checked) => {
            log.info(
                "priced the clipboard item",
                &[
                    ("name", Value::Str(checked.item.info.name.clone())),
                    (
                        "category",
                        Value::Str(
                            checked
                                .item
                                .category
                                .map(|c| c.as_str().to_string())
                                .unwrap_or_default(),
                        ),
                    ),
                    ("modifiers", Value::Int(checked.item.modifiers.len() as i64)),
                    (
                        "stat_filters",
                        Value::Int(checked.stat_filter_count() as i64),
                    ),
                    (
                        "unknown_modifiers",
                        Value::Int(checked.item.unknown_modifiers.len() as i64),
                    ),
                    (
                        "constrains_something",
                        Value::Bool(checked.constrains_something()),
                    ),
                ],
            );

            ExitCode::SUCCESS
        }
        Err(err) => {
            log.error(
                "parsing the clipboard item",
                &[("error", Value::Str(crate::util::error_chain::render(&err)))],
            );

            ExitCode::FAILURE
        }
    }
}

pub fn documents_dir() -> String {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return format!("{profile}\\Documents");
    }

    std::env::var("HOME").map_or_else(|_| String::new(), |home| format!("{home}/Documents"))
}

#[cfg(windows)]
pub fn report_input(log: &Logger, window: &crate::adapter::game_window_adapter::GameWindowAdapter) {
    report_hotkey_outlook(window, log);

    let sent = crate::adapter::game_window_adapter::self_test_send_input();

    if sent == 2 {
        log.info(
            "keyboard input works",
            &[("events_accepted", Value::Int(2))],
        );
    } else {
        log.error(
            "keyboard input is not working, a price check will not be able to copy the item",
            &[("events_accepted", Value::Int(i64::from(sent)))],
        );
    }
}

#[cfg(windows)]
pub fn report_startup(
    log: &Logger,
    game: GameVersion,
    window_title: &str,
    hotkey: &str,
    data: &GameTables,
) {
    log.info(
        "startup",
        &[
            ("game", Value::Str(game.as_str().to_string())),
            ("window_title", Value::Str(window_title.to_string())),
            ("hotkey", Value::Str(hotkey.to_string())),
            ("stats", Value::Int(data.stat_count() as i64)),
            ("item_names", Value::Int(data.item_name_count() as i64)),
            ("augments", Value::Int(data.augment_count() as i64)),
        ],
    );

    let game_config = crate::adapter::game_config_adapter::read(
        std::path::Path::new(&documents_dir()),
        game,
        crate::adapter::game_config_adapter::load_from_disk,
    );

    log.info(
        "game configuration",
        &[
            (
                "path",
                Value::Str(
                    game_config
                        .path
                        .as_ref()
                        .map_or_else(|| "not found".to_string(), |p| p.display().to_string()),
                ),
            ),
            (
                "show_mods_key",
                Value::Str(game_config.show_mods_key.clone()),
            ),
            ("read", Value::Bool(game_config.read)),
        ],
    );
}

pub fn list_gamepads() -> ExitCode {
    use poe_wayfinder_core::controller::sony_pad::{
        is_known, product_name, GAMEPAD_USAGE, GAMEPAD_USAGE_PAGE,
    };

    let devices = crate::adapter::gamepad_adapter::hid::list(GAMEPAD_USAGE_PAGE, GAMEPAD_USAGE);

    println!("HID gamepads this build can see.");
    println!("An Xbox pad is read through XInput and does not appear here.\n");

    for device in &devices {
        let known = match is_known(device.vendor, device.product) {
            true => product_name(device.product),
            false => "not a pad this build reads",
        };

        println!(
            "  {:#06x}:{:#06x}  {} bytes  {known}",
            device.vendor, device.product, device.report_len
        );
        println!("    {}", device.path);
    }

    if devices.is_empty() {
        println!("  (none. Plug a DualSense in, or the pad is claimed by other software.)");
    }

    ExitCode::SUCCESS
}

pub fn watch_pad(seconds: u64) -> ExitCode {
    use crate::adapter::gamepad_adapter::known_devices;
    use poe_wayfinder_core::controller::gamepad_match::{describe_for, PadFamily};
    use poe_wayfinder_core::controller::sony_pad::{parse_report, product_name};

    let Some(device) = known_devices().into_iter().next() else {
        eprintln!("poe-wayfinder: no playstation pad found. Try --list-gamepads.");

        return ExitCode::FAILURE;
    };

    let Some(mut reader) = crate::adapter::gamepad_adapter::hid::HidReader::open(&device) else {
        eprintln!("poe-wayfinder: the pad was found but could not be opened.");

        return ExitCode::FAILURE;
    };

    println!(
        "Watching a {} for {seconds} seconds. Press buttons.",
        product_name(device.product)
    );
    println!("Each line is the raw report, then what this build decodes.\n");

    let until = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    let mut last = None;

    while std::time::Instant::now() < until {
        if let Some(report) = reader.newest() {
            let mask = parse_report(device.product, &report);

            if mask != last {
                last = mask;

                println!(
                    "  {}  mask {:#06x}  {}",
                    hex_line(&report[..report.len().min(12)]),
                    mask.unwrap_or(0),
                    match mask {
                        Some(mask) => describe_for(PadFamily::PlayStation, mask),
                        None => "unreadable report".to_string(),
                    }
                );
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(8));
    }

    ExitCode::SUCCESS
}

pub fn hex_line(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<String>>()
        .join(" ")
}

pub fn pad_walkthrough(path: &str) -> ExitCode {
    use crate::adapter::gamepad_adapter::known_devices;
    use poe_wayfinder_core::controller::sony_pad::{product_name, WALKTHROUGH};

    let Some(device) = known_devices().into_iter().next() else {
        eprintln!("poe-wayfinder: no playstation pad found. Try --list-gamepads.");

        return ExitCode::FAILURE;
    };

    let Some(mut reader) = crate::adapter::gamepad_adapter::hid::HidReader::open(&device) else {
        eprintln!("poe-wayfinder: the pad was found but could not be opened.");

        return ExitCode::FAILURE;
    };

    println!(
        "Walkthrough for a {}. Press each button once, alone.",
        product_name(device.product)
    );
    println!("Every raw report is written to {path}.\n");

    let mut presses = Vec::new();

    for label in WALKTHROUGH {
        println!("  press {label}");

        let Some(press) = press_once(&mut reader, device.product) else {
            println!("    nothing arrived in time. Stopping here.");

            break;
        };

        presses.push((*label, press));
    }

    finish_walkthrough(path, device.product, device.report_len, presses)
}

#[cfg(unix)]
pub fn record_hidraw(node: &str, out: &str) -> ExitCode {
    use std::io::Read;

    use poe_wayfinder_core::controller::sony_pad::{
        is_known, parse_report, product_name, WALKTHROUGH,
    };

    let Some((vendor, product)) = hidraw_ids(node) else {
        eprintln!("poe-wayfinder: could not read the vendor and product of {node}.");
        eprintln!("               Try: ls /dev/hidraw*");

        return ExitCode::FAILURE;
    };

    if !is_known(vendor, product) {
        eprintln!(
            "poe-wayfinder: {node} is {vendor:#06x}:{product:#06x}, not a pad this build reads."
        );

        return ExitCode::FAILURE;
    }

    let mut file = match std::fs::File::open(node) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("poe-wayfinder: opening {node}: {err}");
            eprintln!("               hidraw usually needs sudo or a udev rule.");

            return ExitCode::FAILURE;
        }
    };

    println!(
        "Recording a {} from {node}. Press each button once, alone.",
        product_name(product)
    );
    println!("This reads the pad directly and shares none of the windows code.\n");

    let mut buffer = [0u8; 128];
    let mut presses = Vec::new();
    let mut report_len = 0;

    for label in WALKTHROUGH {
        println!("  press {label}");

        let mut held: Option<PadPress> = None;

        while let Ok(got) = file.read(&mut buffer) {
            if got == 0 {
                break;
            }

            report_len = report_len.max(got);

            let report = buffer[..got].to_vec();

            let Some(mask) = parse_report(product, &report) else {
                continue;
            };

            match (mask, &held) {
                (0, Some(_)) => break,
                (0, None) => {}
                (mask, _) => {
                    held = Some(PadPress {
                        mask,
                        descriptor: None,
                        report,
                    })
                }
            }
        }

        let Some(press) = held else {
            println!("    nothing arrived. Stopping here.");

            break;
        };

        presses.push((*label, press));
    }

    finish_walkthrough(out, product, report_len, presses)
}

#[cfg(unix)]
fn hidraw_ids(node: &str) -> Option<(u16, u16)> {
    let name = std::path::Path::new(node).file_name()?.to_str()?;
    let uevent = format!("/sys/class/hidraw/{name}/device/uevent");
    let body = std::fs::read_to_string(uevent).ok()?;

    for line in body.lines() {
        let Some(ids) = line.strip_prefix("HID_ID=") else {
            continue;
        };

        let mut parts = ids.split(':').skip(1);
        let vendor = u32::from_str_radix(parts.next()?.trim(), 16).ok()?;
        let product = u32::from_str_radix(parts.next()?.trim(), 16).ok()?;

        return Some((vendor as u16, product as u16));
    }

    None
}

#[cfg(not(unix))]
pub fn record_hidraw(_node: &str, _out: &str) -> ExitCode {
    eprintln!("poe-wayfinder: --record-hidraw only works on linux.");

    ExitCode::FAILURE
}

pub struct PadPress {
    pub mask: u16,
    pub descriptor: Option<u16>,
    pub report: Vec<u8>,
}

fn finish_walkthrough(
    path: &str,
    product: u16,
    report_len: usize,
    presses: Vec<(&'static str, PadPress)>,
) -> ExitCode {
    use poe_wayfinder_core::controller::gamepad_match::{describe_for, PadFamily};
    use poe_wayfinder_core::controller::sony_pad::{expected_bit, WALKTHROUGH};

    let mut captured = format!("# product {product:#06x}\n# report_len {report_len}\n");

    println!("\n  label      expected  read      descriptor  verdict");

    let mut failed = 0;
    let mut disagreed = 0;
    let mut asked = 0;

    for (label, press) in &presses {
        let wanted = expected_bit(label);
        let ok = wanted == press.mask;

        captured.push_str(&format!("{label} {}\n", hex_line(&press.report)));

        if !ok {
            failed += 1;
        }

        let oracle = match press.descriptor {
            Some(mask) => {
                asked += 1;

                if mask != press.mask {
                    disagreed += 1;
                }

                format!("{mask:#06x}")
            }
            None => "not read".to_string(),
        };

        println!(
            "  {label:<10} {wanted:#06x}    {:#06x}    {oracle:<10}  {}",
            press.mask,
            match ok {
                true => "PASS".to_string(),
                false => format!(
                    "FAIL, read {}",
                    describe_for(PadFamily::PlayStation, press.mask)
                ),
            }
        );
    }

    println!(
        "\n{} of {} buttons decode as this build expects.",
        presses.len() - failed,
        presses.len()
    );

    println!(
        "{asked} of {} checked against the pad's own report descriptor, {disagreed} disagreed.",
        presses.len()
    );

    captured.push_str(&format!(
        "# descriptor_checked {asked}\n# descriptor_disagreed {disagreed}\n"
    ));

    if disagreed > 0 {
        println!(
            "A disagreement means our hardcoded offsets and what the pad says \n\
             about itself do not match. Trust the descriptor."
        );
    }

    if let Err(err) = std::fs::write(path, &captured) {
        eprintln!("poe-wayfinder: writing {path}: {err}");
    }

    match failed == 0 && disagreed == 0 && presses.len() == WALKTHROUGH.len() {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    }
}

fn press_once(
    reader: &mut crate::adapter::gamepad_adapter::hid::HidReader,
    product: u16,
) -> Option<PadPress> {
    use poe_wayfinder_core::controller::sony_pad::parse_report;

    let until = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut held: Option<PadPress> = None;

    while std::time::Instant::now() < until {
        if let Some(report) = reader.newest() {
            let Some(mask) = parse_report(product, &report) else {
                continue;
            };

            match (mask, &held) {
                (0, Some(_)) => return held,
                (0, None) => {}
                (mask, _) => {
                    held = Some(PadPress {
                        mask,
                        descriptor: reader.decode_by_descriptor(&report),
                        report,
                    })
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(8));
    }

    None
}

pub fn run_subcommand(args: &[String]) -> Option<ExitCode> {
    if args.iter().any(|a| a == "--list-gamepads") {
        return Some(list_gamepads());
    }

    if args.first().map(String::as_str) == Some("--watch-pad") {
        let seconds = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20);

        return Some(watch_pad(seconds));
    }

    if args.first().map(String::as_str) == Some("--record-hidraw") {
        let (Some(node), Some(out)) = (args.get(1), args.get(2)) else {
            eprintln!("usage: --record-hidraw /dev/hidrawN <file to write>");

            return Some(ExitCode::FAILURE);
        };

        return Some(record_hidraw(node, out));
    }

    if args.first().map(String::as_str) == Some("--pad-walkthrough") {
        let Some(path) = args.get(1) else {
            eprintln!("usage: --pad-walkthrough <file to write the raw reports to>");

            return Some(ExitCode::FAILURE);
        };

        return Some(pad_walkthrough(path));
    }

    if args.iter().any(|a| a == "--list-windows") {
        return Some(list_windows());
    }

    if args.first().map(String::as_str) == Some("--fake-game") {
        let title = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| "Path of Exile 2".into());
        let seconds = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(30);

        let Some(path) = args.get(3) else {
            eprintln!("usage: --fake-game <title> <seconds> <item-file>");

            return Some(ExitCode::FAILURE);
        };

        let item = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("poe-wayfinder: reading {path}: {err}");

                return Some(ExitCode::FAILURE);
            }
        };

        return Some(fake_game(&title, seconds, &item));
    }

    if args.first().map(String::as_str) == Some("--move-mouse") {
        let x = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
        let y = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);

        return Some(move_mouse(x, y));
    }

    if args.first().map(String::as_str) == Some("--focus-window") {
        let Some(title) = args.get(1).cloned() else {
            eprintln!("usage: --focus-window <exact title>");

            return Some(ExitCode::FAILURE);
        };

        return Some(focus_window(&title));
    }

    if args.iter().any(|a| a == "--self-test-hook") {
        return Some(self_test_hook());
    }

    if args.iter().any(|a| a == "--self-test-hotkey") {
        return Some(self_test_hotkey());
    }

    None
}

#[cfg(not(windows))]
pub fn report_startup(
    _log: &crate::logging::Logger,
    _game: poe_wayfinder_core::types::GameVersion,
    _window_title: &str,
    _hotkey: &str,
    _data: &crate::adapter::game_data_adapter::GameTables,
) {
}

#[cfg(windows)]
pub fn move_mouse(x: i32, y: i32) -> ExitCode {
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;

    match unsafe { SetCursorPos(x, y) } {
        Ok(()) => {
            println!("poe-wayfinder: cursor moved to {x},{y}");

            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("poe-wayfinder: moving the cursor: {err}");

            ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
pub fn move_mouse(_x: i32, _y: i32) -> ExitCode {
    eprintln!("poe-wayfinder: --move-mouse only works on Windows.");

    ExitCode::FAILURE
}

#[cfg(windows)]
pub fn attach_console() {
    use windows::Win32::System::Console::{
        AttachConsole, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };

    if handle_is_set(STD_OUTPUT_HANDLE) {
        return;
    }

    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_err() {
        return;
    }

    bind_to_console(STD_OUTPUT_HANDLE);
    bind_to_console(STD_ERROR_HANDLE);
}

#[cfg(windows)]
fn handle_is_set(which: windows::Win32::System::Console::STD_HANDLE) -> bool {
    use windows::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows::Win32::System::Console::GetStdHandle;

    match unsafe { GetStdHandle(which) } {
        Ok(handle) => !handle.is_invalid() && handle != INVALID_HANDLE_VALUE,
        Err(_) => false,
    }
}

#[cfg(windows)]
fn bind_to_console(which: windows::Win32::System::Console::STD_HANDLE) {
    use windows::core::w;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Console::SetStdHandle;

    const GENERIC_READ_WRITE: u32 = 0xC000_0000;

    let opened = unsafe {
        CreateFileW(
            w!("CONOUT$"),
            GENERIC_READ_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    };

    if let Ok(handle) = opened {
        let _ = unsafe { SetStdHandle(which, handle) };
    }
}

#[cfg(not(windows))]
pub fn attach_console() {}

#[cfg(windows)]
pub fn focus_window(title: &str) -> ExitCode {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::SW_RESTORE;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, ShowWindow};

    let wanted = HSTRING::from(title);

    let Ok(handle) = (unsafe { FindWindowW(None, &wanted) }) else {
        eprintln!("poe-wayfinder: no window is titled {title:?}");

        return ExitCode::FAILURE;
    };

    if handle.is_invalid() {
        eprintln!("poe-wayfinder: no window is titled {title:?}");

        return ExitCode::FAILURE;
    }

    unsafe {
        let _ = ShowWindow(handle, SW_RESTORE);
    }

    let raised = raise_to_front(handle);

    println!("poe-wayfinder: {title:?} raised={raised}");

    match raised {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    }
}

#[cfg(windows)]
fn raise_to_front(handle: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{SetActiveWindow, SetFocus};
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    };

    if unsafe { SetForegroundWindow(handle) }.as_bool() {
        return true;
    }

    let front = unsafe { GetForegroundWindow() };
    let owner = unsafe { GetWindowThreadProcessId(front, None) };
    let ours = unsafe { GetCurrentThreadId() };

    if owner == 0 || owner == ours {
        return false;
    }

    let attached = unsafe { AttachThreadInput(ours, owner, true) }.as_bool();

    let raised = unsafe {
        let _ = BringWindowToTop(handle);
        let _ = SetActiveWindow(handle);
        let _ = SetFocus(Some(handle));

        SetForegroundWindow(handle).as_bool()
    };

    if attached {
        unsafe {
            let _ = AttachThreadInput(ours, owner, false);
        }
    }

    raised
}

#[cfg(not(windows))]
pub fn focus_window(_title: &str) -> ExitCode {
    eprintln!("poe-wayfinder: --focus-window only works on Windows.");

    ExitCode::FAILURE
}
