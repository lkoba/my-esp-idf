#![feature(trait_alias)]

pub mod ble;
pub mod event;
pub mod l298_motor_controller;
pub mod servo;
pub mod steam_controller;
pub mod wifi;
pub mod wifible;

pub use crate::wifible::*;
use anyhow::Result;
pub use embedded_hal_0_2::digital::v2::PinState;

pub trait OutputPin = embedded_hal_0_2::digital::v2::OutputPin<Error = esp_idf_sys::EspError>;
pub trait PwmPin = embedded_hal::pwm::blocking::PwmPin<Duty = esp_idf_hal::gpio::PwmDuty>;
pub trait PwmPinWithMicros = esp_idf_hal::gpio::PwmPinWithMicros<Duty = esp_idf_hal::gpio::PwmDuty>;

#[macro_export]
macro_rules! output_pin {
    ($x:expr) => {{
        $x.into_output().unwrap()
    }};
}

// The difference mcpwm has with ledcpwm is that it allows to control the
// duty cycle by milliseconds, which simplified the implementation of the servo
// control library.
#[macro_export]
macro_rules! mcpwm_pin {
    ($gpio:expr, $channel:expr) => {{
        $gpio.into_mcpwm($channel).unwrap()
    }};
}

#[macro_export]
macro_rules! ledcpwm_pin {
    ($gpio:expr, $channel:expr) => {{
        $gpio.into_ledcpwm($channel).unwrap()
    }};
}

#[macro_export]
macro_rules! input_pin {
    ( $x:expr ) => {
        $x.into_input().unwrap()
    };
}

pub fn enable_watchdog(timeout_s: u32) -> Result<()> {
    unsafe {
        esp_idf_sys::esp!(esp_idf_sys::esp_task_wdt_init(timeout_s, true))?;
        let handle = esp_idf_sys::xTaskGetCurrentTaskHandle();
        esp_idf_sys::esp!(esp_idf_sys::esp_task_wdt_add(handle))?;
    }
    Ok(())
}

// Without callling vTaskDelay the watchdog timer does NOT reset.
// Even calling delay.delay_ms doesn't reset it.
// So, to reset the watchdog correct we need to call:
// esp_task_wdt_reset + vTaskDelay.
#[inline]
pub fn reset_watchdog() -> Result<()> {
    unsafe {
        esp_idf_sys::esp!(esp_idf_sys::esp_task_wdt_reset())?;
        esp_idf_sys::vTaskDelay(1);
    }
    Ok(())
}

const PORT_TICK_PERIOD_MS: u32 = 1000 / esp_idf_sys::configTICK_RATE_HZ;
#[inline]
pub fn delay_ms(delay: u32) {
    unsafe {
        esp_idf_sys::vTaskDelay(delay / PORT_TICK_PERIOD_MS);
    }
}

#[inline]
pub fn get_time_millis() -> i64 {
    unsafe { esp_idf_sys::esp_timer_get_time() / 1000 }
}

pub fn vec_i8_into_u8(v: Vec<i8>) -> Vec<u8> {
    let mut v = std::mem::ManuallyDrop::new(v);
    let p = v.as_mut_ptr();
    let len = v.len();
    let cap = v.capacity();
    unsafe { Vec::from_raw_parts(p as *mut u8, len, cap) }
}

pub fn c_i8_to_string(buffer: &[i8]) -> Result<String, std::string::FromUtf8Error> {
    match String::from_utf8(buffer.iter().map(|&c| c as u8).collect()) {
        Ok(txt) => Ok(txt.trim_end_matches(char::from(0)).to_owned()),
        Err(e) => Err(e),
    }
}

pub fn print_partitions() {
    log::info!("print_partitions");
    unsafe {
        let running_partition = esp_idf_sys::esp_ota_get_running_partition();
        let update_partition = esp_idf_sys::esp_ota_get_next_update_partition(running_partition);
        for &p in &[running_partition, update_partition] {
            let p = *p;
            println!(
                "label={} subtype={} address={}",
                c_i8_to_string(&p.label).unwrap_or(String::from("?????")),
                p.subtype,
                p.address
            );
        }
    }
    log::info!("/print_partitions");
}

pub fn ping(ip: std::net::Ipv4Addr) -> Result<()> {
    log::info!("PING {:?}", ip);
    let ping_summary = embedded_svc::ping::Ping::ping(
        &mut esp_idf_svc::ping::EspPing::default(),
        ip,
        &embedded_svc::ping::Configuration {
            count: 1,
            ..Default::default()
        },
    )?;
    if ping_summary.transmitted != ping_summary.received {
        Err(anyhow::Error::msg("PING failed"))
    } else {
        log::info!("PING ok");
        Ok(())
    }
}

// Needs adding esp_https_ota.h into bindings.h in esp-idf-sys and then
// adding the modified lib to Cargo.toml in the [patch.crates-io] section.
// https://github.com/espressif/esp-idf/blob/master/components/esp_https_ota/src/esp_https_ota.c
// https://github.com/espressif/esp-idf/blob/master/examples/system/ota/simple_ota_example/main/simple_ota_example.c
pub fn https_ota(url: &str, cert_pem: &str) -> Result<()> {
    let url = std::ffi::CString::new(url)?;
    let cert_pem = std::ffi::CString::new(cert_pem)?;
    let config = esp_idf_sys::esp_http_client_config_t {
        url: url.as_ptr(),
        cert_pem: cert_pem.as_ptr(),
        keep_alive_enable: true,
        ..Default::default()
    };
    log::info!("Connecting to OTA server at {:?}", url);
    unsafe {
        match esp_idf_sys::esp!(esp_idf_sys::esp_https_ota(&config)) {
            Ok(_) => esp_idf_sys::esp_restart(),
            Err(err) => {
                return Err(anyhow::Error::msg(format!(
                    "OTA failed with error code: {}",
                    err
                )))
            }
        }
    }
    Ok(())
}

pub fn start_udp_listener<F>(port: u16, handler: F) -> Result<()>
where
    F: Fn(std::net::SocketAddr, &[u8]) + Send + 'static,
{
    std::thread::Builder::new()
        .stack_size(4096)
        .spawn(move || {
            let socket = std::net::UdpSocket::bind(("0.0.0.0", port)).unwrap();
            loop {
                let mut buffer = vec![0; 500];
                let (size, src) = socket.recv_from(&mut buffer).unwrap();
                handler(src, &buffer[..size]);
            }
        })?;
    Ok(())
}

static OTA_SERVER_URL: &str = env!("RUST_ESP32_OTA_SERVER_URL");
static OTA_SERVER_CERT: &str = env!("RUST_ESP32_OTA_SERVER_CERT");
pub fn run_https_ota() -> Result<()> {
    crate::wifible::connect_wifi()?;
    https_ota(OTA_SERVER_URL, OTA_SERVER_CERT)
}

pub fn init() {
    // These notes are copied from a newly bootstrapped project:
    // // Temporary. Will disappear once ESP-IDF 4.4 is released, but for now it is necessary to call this function once,
    // // or else some patches to the runtime implemented by esp-idf-sys might not link properly.
    esp_idf_sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
}

// fn read_touch() {
//     // https://docs.espressif.com/projects/esp-idf/en/latest/esp32/api-reference/peripherals/touch_pad.html
//     // https://github.com/espressif/esp-idf/blob/279c8aeb8a312a178c73cf4b50a30798332ea79b/examples/peripherals/touch_sensor/touch_sensor_v1/touch_pad_read/main/tp_read_main.c
//     // https://github.com/espressif/esp-idf/blob/279c8aeb8a312a178c73cf4b50a30798332ea79b/examples/peripherals/touch_sensor/touch_sensor_v1/touch_pad_interrupt/main/tp_interrupt_main.c
//     log::info!("test_read_touch start...");
//     // Estos dos son equivalentes.
//     // let sensor_pin = esp_idf_sys::touch_pad_t_TOUCH_PAD_NUM5;
//     let sensor_pin = <esp_idf_hal::gpio::Gpio12<esp_idf_hal::gpio::Unknown> as esp_idf_hal::gpio::TouchPin>::touch_channel();
//     unsafe {
//         esp_idf_sys::esp!(esp_idf_sys::touch_pad_init()).unwrap();
//         // GPIO12 en esp32 segun bindings.rs
//         esp_idf_sys::touch_pad_config(sensor_pin, 0);
//     }
//     log::info!("reading gpio12 ...");
//     let mut touch_value: u16 = 0;
//     let touch_value_ptr: *mut u16 = &mut touch_value;
//     for _ in 0..100 {
//         unsafe {
//             // para cuando esta actuvo el modo software, solo touch_pad_init.
//             esp_idf_sys::esp!(esp_idf_sys::touch_pad_read(
//                 esp_idf_sys::touch_pad_t_TOUCH_PAD_NUM5,
//                 touch_value_ptr,
//             ))
//             .unwrap();
//             // // para cuando esta actuvo el modo filtrado.
//             // esp!(esp_idf_sys::touch_pad_read_raw_data(
//             //     esp_idf_sys::touch_pad_t_TOUCH_PAD_NUM5,
//             //     touch_value_ptr,
//             // )).unwrap();
//         }
//         log::info!("value = {}", touch_value);
//         crate::delay_ms(20_u32);
//     }
//     log::info!("test_read_touch done!");
// }
