use super::{
    dev::{BleConnHandle, BlePeerDevice, BlePeerDeviceAddress},
    SafeBle,
};
use anyhow::Result;
use std::sync::Arc;

pub(crate) enum BleConnectEvent {
    Connected(BleConnHandle),
    Error(u16),
    Disconnected(BleConnHandle),
    Notification(Vec<u8>),
    Indication(Vec<u8>),
}

pub struct BleClient {
    ble: SafeBle,
}

impl BleClient {
    pub fn new(ble: SafeBle) -> Self {
        let client = Self { ble };
        client
    }

    pub fn connect(&mut self, device: &BlePeerDevice) -> Result<()> {
        log::info!("Connecting to device {}", device);

        let rx = device.shared_state_mod(|shared| {
            // Callback.
            let ble = Arc::downgrade(&self.ble);
            let (tx, rx) = std::sync::mpsc::channel();
            shared.callback = Some(Box::new(move |event| {
                match event {
                    BleConnectEvent::Disconnected(conn_handle) => match ble.upgrade() {
                        Some(ble) => {
                            let mut ble = ble.lock();
                            for (_addr, shared) in &mut ble.devices {
                                if let Some(dev_conn_handle) = shared.conn_handle {
                                    if dev_conn_handle == conn_handle {
                                        shared.conn_handle = None;
                                        break;
                                    }
                                }
                            }
                        }
                        None => panic!("Couldn't upgrade BLE weak reference"),
                    },
                    // We only care about disconnects here, the rest of the
                    // events are only queued in the event channel to be handled
                    // by the user.
                    _ => {}
                };
                tx.send(event).unwrap();
            }));

            // Start the connection thread.
            let cb_arg: *mut _ = &mut shared.callback;
            unsafe {
                esp_idf_sys::ble_gap_connect(
                    esp_idf_sys::BLE_OWN_ADDR_PUBLIC as u8,
                    &device.address().0,
                    10000,
                    std::ptr::null(),
                    Some(Self::ble_on_gap_connect_event),
                    cb_arg as *mut esp_idf_sys::c_types::c_void,
                )
            };

            // Return event channel.
            rx
        });

        // Wait until it's finished.
        loop {
            match rx.recv() {
                Ok(BleConnectEvent::Connected(conn_handle)) => {
                    device.shared_state_mod(|shared| {
                        shared.event_rx = Some(rx);
                        shared.conn_handle = Some(conn_handle);
                    });
                    break Ok(());
                }
                Ok(BleConnectEvent::Disconnected(_)) => {
                    device.shared_state_mod(|shared| {
                        shared.conn_handle = None;
                    });
                }
                Ok(BleConnectEvent::Error(rc)) => {
                    device.shared_state_mod(|shared| {
                        shared.conn_handle = None;
                    });
                    anyhow::bail!("Connection failed! rc={}", rc);
                }
                Ok(_) => panic!("Unexpected event received"),
                Err(e) => panic!("Error receving connection event: {}", e),
            }
        }
    }

    fn disconnect(&self, address: &BlePeerDeviceAddress) -> Result<()> {
        log::info!("BLE client: disconnecting from {} ...", address);
        let (conn_handle, event_rx) = {
            let mut ble = self.ble.lock();
            let shared = ble.devices.get_mut(address).unwrap();
            let event_rx = std::mem::take(&mut shared.event_rx).unwrap();
            (shared.conn_handle.unwrap(), event_rx)
        };
        if unsafe { esp_idf_sys::ble_gap_terminate(conn_handle as u16, 19) } == 0 {
            loop {
                match event_rx.recv() {
                    Ok(BleConnectEvent::Disconnected(_)) => break,
                    Ok(_) => continue,
                    Err(_) => panic!("Unexpected error waiting for disconnected"),
                }
            }
        }
        Ok(())
    }

    unsafe extern "C" fn ble_on_gap_connect_event(
        event: *mut esp_idf_sys::ble_gap_event,
        cb_arg: *mut esp_idf_sys::c_types::c_void,
    ) -> esp_idf_sys::c_types::c_int {
        let event = *event;
        let cb_arg = (cb_arg as *mut Box<dyn FnMut(BleConnectEvent)>)
            .as_mut()
            .unwrap();

        match event.type_ as u32 {
            esp_idf_sys::BLE_GAP_EVENT_CONNECT => {
                log::info!("BLE gap event, BLE_GAP_EVENT_CONNECT");
                if event.__bindgen_anon_1.connect.status == 0 {
                    let rc = esp_idf_sys::ble_hs_hci_util_set_data_len(
                        event.__bindgen_anon_1.connect.conn_handle,
                        251,
                        2120,
                    );
                    if rc != 0 {
                        log::error!("Set packet length failed; rc = {}", rc);
                        cb_arg(BleConnectEvent::Error(rc.try_into().unwrap()));
                    }
                    let rc = esp_idf_sys::ble_att_set_preferred_mtu(512);
                    if rc != 0 {
                        log::error!("Failed to set preferred MTU; rc = {}", rc);
                        cb_arg(BleConnectEvent::Error(rc.try_into().unwrap()));
                    }
                    let rc = esp_idf_sys::ble_gattc_exchange_mtu(
                        event.__bindgen_anon_1.connect.conn_handle,
                        None,
                        std::mem::MaybeUninit::zeroed().assume_init(),
                    );
                    if rc != 0 {
                        log::error!("MTU exchange error: rc={}", rc);
                        cb_arg(BleConnectEvent::Error(rc.try_into().unwrap()));
                    }
                } else {
                    log::error!(
                        "unexpected connection status: {}",
                        event.__bindgen_anon_1.connect.status
                    );
                    cb_arg(BleConnectEvent::Error(
                        event.__bindgen_anon_1.connect.status.try_into().unwrap(),
                    ));
                }

                0
            }

            esp_idf_sys::BLE_GAP_EVENT_DISCONNECT => {
                log::info!("BLE gap event, BLE_GAP_EVENT_DISCONNECT");
                cb_arg(BleConnectEvent::Disconnected(
                    event.__bindgen_anon_1.disconnect.conn.conn_handle.into(),
                ));
                0
            }

            esp_idf_sys::BLE_GAP_EVENT_ENC_CHANGE => {
                log::info!("BLE gap event, BLE_GAP_EVENT_ENC_CHANGE");
                cb_arg(BleConnectEvent::Connected(
                    event.__bindgen_anon_1.enc_change.conn_handle as BleConnHandle,
                ));
                0
            }

            esp_idf_sys::BLE_GAP_EVENT_NOTIFY_RX => {
                let data_p = (*event.__bindgen_anon_1.notify_rx.om).om_data;
                let data_len = (*event.__bindgen_anon_1.notify_rx.om).om_len;
                let data = std::slice::from_raw_parts(data_p, data_len as usize);

                if event.__bindgen_anon_1.notify_rx.indication() == 1 {
                    cb_arg(BleConnectEvent::Indication(data.to_vec()));
                } else {
                    cb_arg(BleConnectEvent::Notification(data.to_vec()));
                }
                0
            }

            esp_idf_sys::BLE_GAP_EVENT_MTU => {
                log::info!("BLE gap event, BLE_GAP_EVENT_MTU");
                let rc =
                    esp_idf_sys::ble_gap_security_initiate(event.__bindgen_anon_1.mtu.conn_handle);
                if rc != 0 {
                    log::error!("Error initiating ble_gap_security_initiate: rc={}", rc);
                    cb_arg(BleConnectEvent::Error(rc.try_into().unwrap()));
                }
                0
            }

            _ => 0,
        }
    }
}

impl Drop for BleClient {
    fn drop(&mut self) {
        log::info!("BLE client: dropping ...");

        unsafe { esp_idf_sys::ble_gap_conn_cancel() };

        let connected: Vec<BlePeerDeviceAddress> = {
            let ble = self.ble.lock();
            (&ble.devices)
                .into_iter()
                .filter(|(_, state)| state.conn_handle.is_some())
                .map(|(addr, _)| addr.clone())
                .collect()
        };

        for addr in connected {
            self.disconnect(&addr).ok();
        }

        log::info!("BLE client: drop finished ...");
    }
}
