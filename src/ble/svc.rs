use super::{chr::BlePeerCharacteristic, dev::BleConnHandle, uuid::BleUUID};
use anyhow::Result;

enum BlePeerCharacteristicDiscoveryEvent {
    Discovery(BlePeerCharacteristic),
    DiscoveryFinished,
}

pub struct BlePeerService {
    pub(super) conn_handle: u16,
    pub(super) start_handle: u16,
    pub(super) end_handle: u16,
    pub(super) uuid: BleUUID,
}

impl BlePeerService {
    pub fn uuid(&self) -> &BleUUID {
        &self.uuid
    }

    pub fn get_characteristics(&self) -> Result<Vec<BlePeerCharacteristic>> {
        log::info!("Retrieving characteristics for service {}", self);

        let mut characteristics = vec![];
        {
            // Callback.
            let (tx, rx) = std::sync::mpsc::channel();
            let characteristics = &mut characteristics;
            let mut callback: Box<dyn FnMut(BlePeerCharacteristicDiscoveryEvent)> =
                Box::new(move |event| match event {
                    BlePeerCharacteristicDiscoveryEvent::Discovery(chr) => {
                        characteristics.push(chr);
                    }
                    BlePeerCharacteristicDiscoveryEvent::DiscoveryFinished => tx.send(()).unwrap(),
                });

            // Start the discovery thread.
            let cb_arg: *mut _ = &mut callback;
            let rc = unsafe {
                esp_idf_sys::ble_gattc_disc_all_chrs(
                    self.conn_handle,
                    self.start_handle,
                    self.end_handle,
                    Some(BlePeerService::ble_on_gatt_disc_chrs),
                    cb_arg as *mut esp_idf_sys::c_types::c_void,
                )
            };
            if rc != 0 {
                return Err(anyhow::anyhow!(
                    "Error initiating GAP characteristic discovery procedure; rc={}",
                    rc
                ));
            }

            // Wait for results.
            loop {
                match rx.recv() {
                    Ok(_) => break,
                    Err(e) => anyhow::bail!("Error retrieving device characteristics: {}", e),
                }
            }
        }

        // Configuramos el end handle de las caracteristicas con el def_handle
        // de la proxima characteristica - 1.
        let mut iter = characteristics.iter_mut().peekable();
        while let Some(chr) = iter.next() {
            if let Some(next) = iter.peek() {
                chr.end_handle = next.def_handle - 1;
            } else {
                chr.end_handle = self.end_handle;
            }
            log::info!("Found: {}", chr);
        }

        Ok(characteristics)
    }

    unsafe extern "C" fn ble_on_gatt_disc_chrs(
        conn_handle: u16,
        error: *const esp_idf_sys::ble_gatt_error,
        chr: *const esp_idf_sys::ble_gatt_chr,
        cb_arg: *mut esp_idf_sys::c_types::c_void,
    ) -> esp_idf_sys::c_types::c_int {
        let cb_arg = (cb_arg as *mut Box<dyn FnMut(BlePeerCharacteristicDiscoveryEvent)>)
            .as_mut()
            .unwrap();
        if !chr.is_null() {
            let chr = *chr;
            cb_arg(BlePeerCharacteristicDiscoveryEvent::Discovery(
                BlePeerCharacteristic {
                    conn_handle: conn_handle as BleConnHandle,
                    def_handle: chr.def_handle,
                    val_handle: chr.val_handle,
                    end_handle: 0,
                    properties: chr.properties,
                    uuid: BleUUID::from(chr.uuid),
                },
            ));
        }
        if (if error.is_null() { 0 } else { (*error).status }) == esp_idf_sys::BLE_HS_EDONE as u16 {
            cb_arg(BlePeerCharacteristicDiscoveryEvent::DiscoveryFinished);
        }
        0
    }
}

impl std::fmt::Display for BlePeerService {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "BleService {{ conn_handle={} start_handle={} end_handle={} uuid={} }}",
            self.conn_handle,
            self.start_handle,
            self.end_handle,
            self.uuid.to_string(),
        )
    }
}
