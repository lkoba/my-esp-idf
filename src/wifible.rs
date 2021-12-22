// Since there's a tight configuration dependency between wifi and ble on the
// ESP32 we created this very opinionated module to organize connections to
// either or both.
// Wifi credentiales are configured by setting the environment variables
// RUST_ESP32_WIFI_SSID and RUST_ESP32_WIFI_PASS when compiling.

use anyhow::Result;
use esp_idf_hal::mutex::Mutex;
use esp_idf_svc::nvs::EspDefaultNvs;
use std::{cell::RefCell, sync::Arc};

static WIFI_SSID: &str = env!("RUST_ESP32_WIFI_SSID");
static WIFI_PASS: &str = env!("RUST_ESP32_WIFI_PASS");
static mut STATE: Mutex<RefCell<State>> = Mutex::new(RefCell::new(State {
    wifi: None,
    ble: None,
    nvs: None,
}));

struct State {
    wifi: Option<crate::wifi::Wifi>,
    ble: Option<crate::ble::SafeBle>,
    nvs: Option<Arc<EspDefaultNvs>>,
}

impl State {
    fn nvs(&mut self) -> Arc<EspDefaultNvs> {
        match &self.nvs {
            Some(nvs) => nvs.clone(),
            None => {
                let nvs = Arc::new(EspDefaultNvs::new().unwrap());
                self.nvs = Some(nvs.clone());
                nvs
            }
        }
    }
}

fn with_state<T>(cb: impl FnOnce(&mut State) -> T) -> T {
    let state_cell = unsafe { STATE.lock() };
    let state = &mut *state_cell.borrow_mut();
    cb(state)
}

pub fn connect_wifi() -> Result<()> {
    with_state(|state| {
        match &state.wifi {
            Some(_) => {}
            None => {
                match crate::wifi::Wifi::new_no_auto(
                    Arc::new(esp_idf_svc::netif::EspNetifStack::new()?),
                    Arc::new(esp_idf_svc::sysloop::EspSysLoopStack::new()?),
                    state.nvs(),
                    // Power saving mode is required for concurrent wifi and ble.
                    if state.ble.is_none() {
                        esp_idf_sys::wifi_ps_type_t_WIFI_PS_NONE
                    } else {
                        esp_idf_sys::wifi_ps_type_t_WIFI_PS_MIN_MODEM
                    },
                ) {
                    Ok(mut w) => match w.begin(WIFI_SSID, WIFI_PASS) {
                        Ok(_) => {
                            state.wifi = Some(w);
                        }
                        Err(e) => anyhow::bail!("Error connecting wifi: {}", e),
                    },
                    Err(e) => anyhow::bail!("Error initializing wifi: {}", e),
                }
            }
        }

        Ok(())
    })
}

pub fn connect_ble() -> Result<crate::ble::SafeBle> {
    with_state(|state| match &state.ble {
        Some(ble) => Ok(ble.clone()),
        None => {
            match &mut state.wifi {
                Some(wifi) => {
                    wifi.set_power_save_mode(esp_idf_sys::wifi_ps_type_t_WIFI_PS_MIN_MODEM)?;
                }
                None => {}
            };
            match crate::ble::Ble::new_no_auto(state.nvs()) {
                Ok(b) => {
                    state.ble = Some(b.clone());
                    Ok(b)
                }
                Err(e) => anyhow::bail!("Error initializing BLE stack: {}", e),
            }
        }
    })
}

pub fn connect_wifi_and_ble() -> Result<()> {
    connect_ble()?;
    connect_wifi()?;
    Ok(())
}
