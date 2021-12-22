use super::{
    dev::{BlePeerDevice, BlePeerDeviceAddress},
    BlePeerDeviceSharedState, SafeBle,
};
use anyhow::Result;
use std::sync::{
    mpsc::{Receiver, Sender},
    Arc,
};

enum BlePeerDeviceDiscoveryEvent {
    Discovery(String, BlePeerDeviceAddress),
    DiscoveryFinished,
}

pub struct BleScan {
    ble: SafeBle,
    disc_params: esp_idf_sys::ble_gap_disc_params,
    callback: Box<dyn Fn(BlePeerDeviceDiscoveryEvent)>,
    scan_tx: Sender<BlePeerDevice>,
    scan_rx: Receiver<BlePeerDevice>,
}

impl BleScan {
    pub fn new(ble: SafeBle) -> Self {
        let (scan_tx, scan_rx) = std::sync::mpsc::channel();
        let mut scan = Self {
            ble,
            disc_params: esp_idf_sys::ble_gap_disc_params {
                ..Default::default()
            },
            callback: Box::new(|_| {}),
            scan_rx,
            scan_tx,
        };
        scan.set_default_disc_config();
        scan
    }

    fn set_default_disc_config(&mut self) {
        // Tell the controller to filter duplicates; we don't want to process
        // repeated advertisements from the same device.
        self.disc_params.set_filter_duplicates(1);

        // Perform a passive scan.  I.e., don't send follow-up scan requests to
        // each advertiser.
        self.disc_params.set_passive(1);

        // https://esp32.com/viewtopic.php?f=13&t=15985 (ble + wifi)
        self.disc_params.itvl = esp_idf_sys::BLE_GAP_SCAN_SLOW_INTERVAL1 as u16;
        self.disc_params.window = esp_idf_sys::BLE_GAP_SCAN_SLOW_WINDOW1 as u16;
        self.disc_params.filter_policy = esp_idf_sys::BLE_HCI_SCAN_FILT_NO_WL as u8;
        self.disc_params.set_limited(0);
    }

    pub fn start(&mut self) -> Result<&Receiver<BlePeerDevice>> {
        // Figure out address to use while advertising (no privacy for now)
        let mut own_addr_type = 0_u8;
        let rc = unsafe { esp_idf_sys::ble_hs_id_infer_auto(0, &mut own_addr_type) };
        if rc != 0 {
            anyhow::bail!("Error determining address type; rc={}", rc);
        }

        // Callback.
        let ble = self.ble.lock().weak_ref();
        let scan_tx = self.scan_tx.clone();
        self.callback = Box::new(move |event: BlePeerDeviceDiscoveryEvent| match event {
            BlePeerDeviceDiscoveryEvent::Discovery(name, address) => match ble.upgrade() {
                Some(ble) => {
                    let dev = BlePeerDevice::new(address, Arc::downgrade(&ble));
                    let dev_state = BlePeerDeviceSharedState::new(name);
                    let addr = dev.address().clone();
                    ble.lock().devices.insert(addr, dev_state);
                    scan_tx.send(dev).ok();
                }
                None => panic!("Cannot upgrade weak reference to BLE during scan"),
            },
            BlePeerDeviceDiscoveryEvent::DiscoveryFinished => {}
        });
        let cb_arg: *mut _ = &mut self.callback;

        // Start the scanning thread.
        let rc = unsafe {
            esp_idf_sys::ble_gap_disc(
                own_addr_type,
                i32::MAX,
                &self.disc_params,
                Some(BleScan::ble_on_gap_scan_event),
                cb_arg as *mut esp_idf_sys::c_types::c_void,
            )
        };
        if rc != 0 {
            anyhow::bail!("Error initiating GAP discovery procedure; rc={}", rc);
        }

        Ok(&self.scan_rx)
    }

    pub fn stop(&mut self) -> Result<()> {
        if unsafe { esp_idf_sys::ble_gap_disc_cancel() } == 0 {
            Ok(())
        } else {
            anyhow::bail!("Failure stopping discovery");
        }
    }

    pub fn flush_duplicates(&self) -> Result<()> {
        unsafe { esp_idf_sys::esp!(esp_idf_sys::esp_ble_scan_dupilcate_list_flush())? }
        Ok(())
    }

    unsafe extern "C" fn ble_on_gap_scan_event(
        event: *mut esp_idf_sys::ble_gap_event,
        cb_arg: *mut esp_idf_sys::c_types::c_void,
    ) -> esp_idf_sys::c_types::c_int {
        let event = *event;
        let cb_arg = (cb_arg as *mut Box<dyn FnMut(BlePeerDeviceDiscoveryEvent)>)
            .as_mut()
            .unwrap();

        match event.type_ as u32 {
            esp_idf_sys::BLE_GAP_EVENT_DISC => {
                let mut fields = esp_idf_sys::ble_hs_adv_fields {
                    ..Default::default()
                };
                let rc = esp_idf_sys::ble_hs_adv_parse_fields(
                    &mut fields,
                    event.__bindgen_anon_1.disc.data,
                    event.__bindgen_anon_1.disc.length_data,
                );
                if rc != 0 {
                    log::error!("BLE parsing fields failed");
                    return 0;
                }
                let name = if fields.name_is_complete() == 1 {
                    let name = std::str::from_utf8(std::slice::from_raw_parts(
                        fields.name,
                        fields.name_len as usize,
                    ))
                    .unwrap();
                    name
                } else {
                    ""
                }
                .to_owned();
                cb_arg(BlePeerDeviceDiscoveryEvent::Discovery(
                    name,
                    BlePeerDeviceAddress(event.__bindgen_anon_1.disc.addr),
                ));
                0
            }

            esp_idf_sys::BLE_GAP_EVENT_DISC_COMPLETE => {
                log::info!("BLE gap event, BLE_GAP_EVENT_DISC_COMPLETE");
                cb_arg(BlePeerDeviceDiscoveryEvent::DiscoveryFinished);
                0
            }

            _ => panic!("Unexpected event at ble_on_gap_scan_event"),
        }
    }
}

impl Drop for BleScan {
    fn drop(&mut self) {
        log::info!("BleScan dropping ...");
        self.stop().ok();
    }
}
