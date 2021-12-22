// https://devzone.nordicsemi.com/guides/short-range-guides/b/bluetooth-low-energy/posts/ble-characteristics-a-beginners-tutorial
// http://www.blesstags.eu/2018/08/services-characteristics-descriptors.html
// https://github.com/espressif/esp-idf/blob/master/examples/bluetooth/nimble/throughput_app/blecent_throughput/main/main.c
// https://github.com/espressif/esp-idf/blob/master/examples/bluetooth/esp_hid_host/main/esp_hid_host_main.c

pub mod chr;
pub mod client;
pub mod dev;
pub mod scan;
pub mod svc;
pub mod uuid;

use self::{
    client::BleConnectEvent,
    dev::{BleConnHandle, BlePeerDeviceAddress},
};
use anyhow::Result;
use esp_idf_hal::mutex::Mutex;
use esp_idf_svc::nvs::EspDefaultNvs;
use std::{
    collections::HashMap,
    sync::{mpsc::Receiver, Arc, Weak},
};

extern "C" {
    pub fn ble_store_config_init();
}

static SYNC_STATUS: Mutex<bool> = Mutex::new(false);

struct BlePeerDeviceSharedState {
    conn_handle: Option<BleConnHandle>,
    name: String,
    callback: Option<Box<dyn FnMut(BleConnectEvent)>>,
    event_rx: Option<Receiver<BleConnectEvent>>,
}

impl BlePeerDeviceSharedState {
    pub fn new(name: String) -> Self {
        Self {
            name,
            conn_handle: None,
            callback: None,
            event_rx: None,
        }
    }
}

pub struct Ble {
    _default_nvs: Arc<EspDefaultNvs>,
    _self_ref: Option<Weak<Mutex<Ble>>>,
    devices: HashMap<BlePeerDeviceAddress, BlePeerDeviceSharedState>,
}

impl Ble {
    pub fn new() -> Result<SafeBle> {
        Ble::new_no_auto(Arc::new(EspDefaultNvs::new()?))
    }

    pub fn new_no_auto(default_nvs: Arc<EspDefaultNvs>) -> Result<SafeBle> {
        let ble = Arc::new(Mutex::new(Self {
            _default_nvs: default_nvs,
            _self_ref: None,
            devices: HashMap::new(),
        }));
        let mut locked = ble.lock();
        locked._self_ref = Some(Arc::downgrade(&ble));
        locked.init()?;
        Ok(SafeBle(ble.clone()))
    }

    fn init(&mut self) -> Result<()> {
        unsafe {
            esp_idf_sys::esp!(esp_idf_sys::esp_nimble_hci_and_controller_init())?;
            esp_idf_sys::nimble_port_init();

            // Initialize the NimBLE host configuration
            esp_idf_sys::ble_hs_cfg.reset_cb = Some(Self::ble_on_reset);
            esp_idf_sys::ble_hs_cfg.sync_cb = Some(Self::ble_on_sync);

            // Enable bonding.
            esp_idf_sys::ble_hs_cfg.set_sm_bonding(1);
            esp_idf_sys::ble_hs_cfg.sm_our_key_dist = 1;
            esp_idf_sys::ble_hs_cfg.sm_their_key_dist = 1;
            ble_store_config_init();

            // Start the task
            esp_idf_sys::nimble_port_freertos_init(Some(Self::ble_host_task));

            // TODO: use on_sync to actually wait for sync.
            log::info!("Waiting for sync ...");
            loop {
                if *SYNC_STATUS.lock() {
                    break;
                };
                crate::delay_ms(100);
            }
        }
        Ok(())
    }

    pub fn weak_ref(&mut self) -> Weak<Mutex<Self>> {
        self._self_ref.as_ref().unwrap().clone()
    }

    unsafe extern "C" fn ble_on_reset(reason: esp_idf_sys::c_types::c_int) {
        log::error!("BLE on reset, reason code: {}", reason);
    }

    unsafe extern "C" fn ble_on_sync() {
        log::info!("BLE on sync");
        let mut sync = SYNC_STATUS.lock();
        *sync = true;
    }

    // unsafe extern "C" fn ble_on_read(
    //     obj_type: esp_idf_sys::c_types::c_int,
    //     key: *const esp_idf_sys::ble_store_key,
    //     dst: *mut esp_idf_sys::ble_store_value,
    // ) -> esp_idf_sys::c_types::c_int {
    //     log::debug!(
    //         "BLE on read: obj_type={} key.sec={:?} key.cccd={:?}",
    //         obj_type,
    //         (*key).sec,
    //         (*key).cccd
    //     );
    //     esp_idf_sys::BLE_HS_ENOENT.try_into().unwrap()
    // }

    // unsafe extern "C" fn ble_on_write(
    //     obj_type: esp_idf_sys::c_types::c_int,
    //     val: *const esp_idf_sys::ble_store_value,
    // ) -> esp_idf_sys::c_types::c_int {
    //     log::debug!(
    //         "BLE on write: obj_type={} val.sec={:?} val.cccd={:?}",
    //         obj_type,
    //         (*val).sec,
    //         (*val).cccd
    //     );
    //     0
    // }

    // unsafe extern "C" fn ble_on_delete(
    //     obj_type: esp_idf_sys::c_types::c_int,
    //     key: *const esp_idf_sys::ble_store_key,
    // ) -> esp_idf_sys::c_types::c_int {
    //     log::debug!("BLE on delete");
    //     0
    // }

    unsafe extern "C" fn ble_host_task(_params: *mut esp_idf_sys::c_types::c_void) {
        log::info!("BLE host task started");
        esp_idf_sys::nimble_port_run(); //This function will return only when nimble_port_stop() is executed.
        esp_idf_sys::nimble_port_freertos_deinit();
    }
}

impl Drop for Ble {
    fn drop(&mut self) {
        log::info!("BLE dropping stack ...");
        unsafe {
            let ret = esp_idf_sys::nimble_port_stop();
            if ret == 0 {
                esp_idf_sys::nimble_port_deinit();
                let ret = esp_idf_sys::esp_nimble_hci_and_controller_deinit();
                if ret != esp_idf_sys::ESP_OK {
                    log::error!(
                        "esp_nimble_hci_and_controller_init() failed with error: {}",
                        ret,
                    );
                }
            }
        }
        let mut sync = SYNC_STATUS.lock();
        *sync = false;
    }
}

unsafe impl Send for Ble {}

#[derive(Clone)]
pub struct SafeBle(Arc<Mutex<Ble>>);

impl std::ops::Deref for SafeBle {
    type Target = Arc<Mutex<Ble>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
