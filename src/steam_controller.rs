use anyhow::Result;

use crate::{
    ble::{
        client::{BleClient, BleConnectEvent},
        scan::BleScan,
        uuid::BleUUID,
        SafeBle,
    },
    get_preference, write_preference,
};

static BONDED_MAC_PREFERENCE_KEY: &str = "sc_bonded_mac";
static STEAM_CONTROLLER_BUTTON_A: u32 = 0x800000;
static STEAM_CONTROLLER_BUTTON_X: u32 = 0x400000;
static STEAM_CONTROLLER_BUTTON_B: u32 = 0x200000;
static STEAM_CONTROLLER_BUTTON_Y: u32 = 0x100000;
static STEAM_CONTROLLER_BUTTON_LEFT_BUMPER: u32 = 0x080000;
static STEAM_CONTROLLER_BUTTON_RIGHT_BUMPER: u32 = 0x040000;
static STEAM_CONTROLLER_BUTTON_LEFT_TRIGGER: u32 = 0x020000;
static STEAM_CONTROLLER_BUTTON_RIGHT_TRIGGER: u32 = 0x010000;
static STEAM_CONTROLLER_BUTTON_LEFT_PADDLE: u32 = 0x008000;
static STEAM_CONTROLLER_BUTTON_RIGHT_PADDLE: u32 = 0x000001;
static STEAM_CONTROLLER_BUTTON_NAV_RIGHT: u32 = 0x004000;
static STEAM_CONTROLLER_BUTTON_NAV_LEFT: u32 = 0x001000;
static STEAM_CONTROLLER_BUTTON_STEAM: u32 = 0x002000;
static STEAM_CONTROLLER_BUTTON_JOYSTICK: u32 = 0x000040;
static STEAM_CONTROLLER_BUTTON_RIGHT_PAD_TOUCH: u32 = 0x000010;
static STEAM_CONTROLLER_BUTTON_RIGHT_PAD_CLICK: u32 = 0x000004;
static STEAM_CONTROLLER_BUTTON_LEFT_PAD_TOUCH: u32 = 0x000008;
static STEAM_CONTROLLER_BUTTON_LEFT_PAD_CLICK: u32 = 0x000002;
static STEAM_CONTROLLER_FLAG_REPORT: u16 = 0x0004;
static STEAM_CONTROLLER_FLAG_BUTTONS: u16 = 0x0010;
static STEAM_CONTROLLER_FLAG_PADDLES: u16 = 0x0020;
static STEAM_CONTROLLER_FLAG_JOYSTICK: u16 = 0x0080;
static STEAM_CONTROLLER_FLAG_LEFT_PAD: u16 = 0x0100;
static STEAM_CONTROLLER_FLAG_RIGHT_PAD: u16 = 0x0200;

// static HID_UUID: &str = "00001812-0000-1000-8000-00805f9b34fb";
static SERVICE_UUID: &str = "100f6c32-1735-4313-b402-38567131e5f3";
static EVENTS_CHR_UUID: &str = "100F6C33-1735-4313-B402-38567131E5F3";
static STEAM_MODE_CHR_UUID: &str = "100F6C34-1735-4313-B402-38567131E5F3";
static STEAM_MODE_COMMAND: &[u8] = &[0xc0, 0x87, 0x03, 0x08, 0x07, 0x00];

#[derive(Debug)]
pub enum Button {
    South,
    North,
    East,
    West,
    LeftTrigger,
    LeftTrigger2,
    RightTrigger,
    RightTrigger2,
    LeftBumper,
    RightBumper,
    LeftPaddle,
    RightPaddle,
    NavLeft,
    NavRight,
    Steam,
    LeftStick,
    LeftPad,
    LeftPad2,
    RightPad,
    RightPad2,
}
#[derive(Debug)]
pub enum Axis {
    LeftPadX,
    LeftPadY,
    RightPadX,
    RightPadY,
    LeftStickX,
    LeftStickY,
}
#[derive(Debug)]
pub enum SteamControllerEvent {
    ButtonChanged(Button, f32),
    AxisChanged(Axis, f32),
    Connected,
    Disconnected,
}

pub fn connect<F>(ble: SafeBle, mut cb: F) -> Result<()>
where
    F: FnMut(SteamControllerEvent) + 'static + Send,
{
    std::thread::Builder::new()
        .stack_size(4096)
        .spawn(move || loop {
            match inner_loop(ble.clone(), &mut cb) {
                // match inner_loop(&mut cb) {
                Ok(_) => log::info!("Connection ended"),
                Err(e) => log::error!("Connection failed: {}", e),
            }
            log::info!("Reconnecting soon ...");
            crate::delay_ms(3000);
        })?;
    Ok(())
}

fn inner_loop<F>(ble: SafeBle, cb: &mut F) -> Result<()>
where
    F: FnMut(SteamControllerEvent) + 'static + Send,
{
    let svc_uuid = &BleUUID::parse(SERVICE_UUID)?;
    let events_chr_uuid = &BleUUID::parse(EVENTS_CHR_UUID)?;
    let steam_mode_chr_uuid = &BleUUID::parse(STEAM_MODE_CHR_UUID)?;
    let mut client = BleClient::new(ble.clone());

    // Find the steam controller device and connect to it.
    let mut dev = {
        let mut scan = BleScan::new(ble.clone());
        let paired_address: Option<String> = get_preference(BONDED_MAC_PREFERENCE_KEY)?;
        let scan_rx = scan.start()?;
        match &paired_address {
            Some(addr) => log::info!(
                "Scanning for previously bonded device {} or a controller in pairing mode ...",
                addr,
            ),
            None => log::info!("Scanning for a controller in pairing mode ..."),
        }
        loop {
            match scan_rx.recv() {
                Ok(dev) => {
                    log::info!("Found device: {}", dev);
                    let dev_addr = dev.address().to_string();
                    let is_bonded = match &paired_address {
                        Some(addr) => dev_addr == *addr,
                        None => false,
                    };
                    if is_bonded || dev.name() == "SteamController" {
                        scan.stop()?;
                        client.connect(&dev)?;
                        if !is_bonded {
                            // If it's a new connection save the address so we
                            // can bond without pairing mode.
                            write_preference(BONDED_MAC_PREFERENCE_KEY, dev_addr)?;
                        }
                        break dev;
                    }
                }
                Err(e) => anyhow::bail!("Error scanning for devices: {}", e.to_string()),
            }
        }
    };
    log::info!(
        "Connected to device addr={} conn_handle={}",
        dev.address(),
        dev.conn_handle().unwrap_or(u32::MAX),
    );

    // Search for the ble service that reports controller events.
    let svc = match dev.get_service_by_uuid(svc_uuid)? {
        Some(svc) => svc,
        None => {
            anyhow::bail!("Service not found on steam controller");
        }
    };

    // Register for notifications on the events characteristic.
    let chrs = svc.get_characteristics()?;
    let events_chr = match chrs.iter().find(|chr| chr.uuid() == events_chr_uuid) {
        Some(chr) => chr,
        None => {
            anyhow::bail!("Gamepad events charateristic not found on steam controller");
        }
    };
    events_chr.set_notify(true)?;

    // Set the controller into steam mode (faster updates and ???).
    let steam_mode_chr = match chrs.iter().find(|chr| chr.uuid() == steam_mode_chr_uuid) {
        Some(chr) => chr,
        None => {
            anyhow::bail!("Steam mode charateristic not found on steam controller");
        }
    };
    steam_mode_chr.write(STEAM_MODE_COMMAND)?;

    // Wait for steam controller events, decode and forward them to the
    // callback.
    dev.use_events_channel(move |event_rx| {
        let mut prev_buttons: u32 = 0;
        loop {
            match event_rx.recv() {
                Ok(BleConnectEvent::Notification(data)) => {
                    for e in decode_steam_controller_packet(data, &mut prev_buttons) {
                        cb(e);
                    }
                }
                Ok(BleConnectEvent::Disconnected(_)) => break,
                Ok(_) => {}
                Err(e) => {
                    log::error!("Steam controller event channel error: {}", e);
                    break;
                }
            }
        }
    });

    Ok(())
}

// Decode BLE data packet from the Steam Controller and return the corresponding
// events.
// https://github.com/g3gg0/LegoRemote/blob/master/BLE.ino
fn decode_steam_controller_packet(
    data: Vec<u8>,
    prev_buttons: &mut u32,
) -> Vec<SteamControllerEvent> {
    let mut pos = 0;
    let mut events = vec![];

    if data[pos] != 0xc0 {
        log::error!("Invalid steam controller packet: {:?}", data);
        return events;
    }

    pos += 1;

    if data[pos] & 0x0f == 0x05 {
        return events;
    }

    if u16::from_le_bytes(data[pos..pos + 2].try_into().unwrap()) & 0x0f
        == STEAM_CONTROLLER_FLAG_REPORT
    {
        let mut flags: u16 = (((data[pos + 1] as u16) << 8) | (data[pos] as u16)) & !0x0f;
        pos += 2;
        if (flags & STEAM_CONTROLLER_FLAG_BUTTONS) != 0 {
            let buttons: u32 = ((data[pos + 0] as u32) << 16)
                | ((data[pos + 1] as u32) << 8)
                | (data[pos + 2] as u32);
            pos += 3;
            flags &= !STEAM_CONTROLLER_FLAG_BUTTONS;

            if (buttons & STEAM_CONTROLLER_BUTTON_A) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::South, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_A) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::South, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_B) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::East, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_B) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::East, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_X) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::West, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_X) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::West, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_Y) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::North, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_Y) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::North, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_Y) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::North, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_Y) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::North, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_LEFT_BUMPER) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::LeftBumper, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_LEFT_BUMPER) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::LeftBumper, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_RIGHT_BUMPER) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(
                    Button::RightBumper,
                    1.0,
                ));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_RIGHT_BUMPER) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(
                    Button::RightBumper,
                    0.0,
                ));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_LEFT_TRIGGER) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(
                    Button::LeftTrigger,
                    1.0,
                ));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_LEFT_TRIGGER) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(
                    Button::LeftTrigger,
                    0.0,
                ));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_RIGHT_TRIGGER) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(
                    Button::RightTrigger,
                    1.0,
                ));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_RIGHT_TRIGGER) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(
                    Button::RightTrigger,
                    0.0,
                ));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_LEFT_PADDLE) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::LeftPaddle, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_LEFT_PADDLE) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::LeftPaddle, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_RIGHT_PADDLE) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(
                    Button::RightPaddle,
                    1.0,
                ));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_RIGHT_PADDLE) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(
                    Button::RightPaddle,
                    0.0,
                ));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_NAV_LEFT) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::NavLeft, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_NAV_LEFT) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::NavLeft, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_NAV_RIGHT) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::NavRight, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_NAV_RIGHT) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::NavRight, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_STEAM) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::Steam, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_STEAM) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::Steam, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_JOYSTICK) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::LeftStick, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_JOYSTICK) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::LeftStick, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_LEFT_PAD_CLICK) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::LeftPad2, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_LEFT_PAD_CLICK) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::LeftPad2, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_LEFT_PAD_TOUCH) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::LeftPad, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_LEFT_PAD_TOUCH) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::LeftPad, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_RIGHT_PAD_CLICK) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::RightPad2, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_RIGHT_PAD_CLICK) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::RightPad2, 0.0));
            }

            if (buttons & STEAM_CONTROLLER_BUTTON_RIGHT_PAD_TOUCH) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::RightPad, 1.0));
            } else if (*prev_buttons & STEAM_CONTROLLER_BUTTON_RIGHT_PAD_TOUCH) != 0 {
                events.push(SteamControllerEvent::ButtonChanged(Button::RightPad, 0.0));
            }

            *prev_buttons = buttons;
        }

        if (flags & STEAM_CONTROLLER_FLAG_PADDLES) != 0 {
            let left = data[pos];
            let right = data[pos + 1];
            pos += 2;
            flags &= !STEAM_CONTROLLER_FLAG_PADDLES;
            events.push(SteamControllerEvent::ButtonChanged(
                Button::LeftTrigger2,
                left as f32 / 255.0,
            ));
            events.push(SteamControllerEvent::ButtonChanged(
                Button::RightTrigger2,
                right as f32 / 255.0,
            ));
        }

        if (flags & STEAM_CONTROLLER_FLAG_JOYSTICK) != 0 {
            let joy_x: i16 = (data[pos + 1] as i16) << 8 | (data[pos] as i16);
            let joy_y: i16 = (data[pos + 3] as i16) << 8 | (data[pos + 2] as i16);
            let joy_x: f32 = joy_x as f32 / 32760.0;
            let joy_y: f32 = joy_y as f32 / 32760.0;
            events.push(SteamControllerEvent::AxisChanged(Axis::LeftStickX, joy_x));
            events.push(SteamControllerEvent::AxisChanged(Axis::LeftStickY, joy_y));
        }

        if (flags & STEAM_CONTROLLER_FLAG_LEFT_PAD) != 0 {
            let joy_x: i16 = (data[pos + 1] as i16) << 8 | (data[pos] as i16);
            let joy_y: i16 = (data[pos + 3] as i16) << 8 | (data[pos + 2] as i16);
            let joy_x: f32 = joy_x as f32 / 32760.0;
            let joy_y: f32 = joy_y as f32 / 32760.0;
            pos += 4;
            flags &= !STEAM_CONTROLLER_FLAG_LEFT_PAD;
            events.push(SteamControllerEvent::AxisChanged(Axis::LeftPadX, joy_x));
            events.push(SteamControllerEvent::AxisChanged(Axis::LeftPadY, joy_y));
        }

        if (flags & STEAM_CONTROLLER_FLAG_RIGHT_PAD) != 0 {
            let joy_x: i16 = (data[pos + 1] as i16) << 8 | (data[pos] as i16);
            let joy_y: i16 = (data[pos + 3] as i16) << 8 | (data[pos + 2] as i16);
            let joy_x: f32 = joy_x as f32 / 32760.0;
            let joy_y: f32 = joy_y as f32 / 32760.0;
            pos += 4;
            flags &= !STEAM_CONTROLLER_FLAG_RIGHT_PAD;
            events.push(SteamControllerEvent::AxisChanged(Axis::RightPadX, joy_x));
            events.push(SteamControllerEvent::AxisChanged(Axis::RightPadY, joy_y));
        }

        drop(flags); // prevent unused var warning.
    }

    drop(pos); // prevent unused var warning.

    events
}
