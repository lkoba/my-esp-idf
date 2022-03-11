use anyhow::Result;
use esp_idf_hal::mutex::Mutex;
use esp_idf_svc::nvs::EspDefaultNvs;
use std::{cell::RefCell, sync::Arc};

pub struct State {
    pub(super) wifi: Option<crate::wifi::Wifi>,
    pub(super) ble: Option<crate::ble::SafeBle>,
    nvs: Option<Arc<EspDefaultNvs>>,
}

static mut STATE: Mutex<RefCell<State>> = Mutex::new(RefCell::new(State {
    wifi: None,
    ble: None,
    nvs: None,
}));

impl State {
    pub(super) fn nvs(&mut self) -> Result<Arc<EspDefaultNvs>> {
        Ok(match &self.nvs {
            Some(nvs) => nvs.clone(),
            None => {
                let nvs = Arc::new(EspDefaultNvs::new()?);
                self.nvs = Some(nvs.clone());
                nvs
            }
        })
    }
}

pub(super) fn with_state<T>(cb: impl FnOnce(&mut State) -> T) -> T {
    let state_cell = unsafe { STATE.lock() };
    let state = &mut *state_cell.borrow_mut();
    cb(state)
}
