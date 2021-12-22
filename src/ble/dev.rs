use super::client::BleConnectEvent;
use super::svc::BlePeerService;
use super::uuid::BleUUID;
use super::{Ble, BlePeerDeviceSharedState};
use anyhow::Result;
use esp_idf_hal::mutex::Mutex;
use std::sync::mpsc::Receiver;
use std::sync::Weak;

pub type BleConnHandle = u32;

enum BlePeerServiceDiscoveryEvent {
    Discovery(BlePeerService),
    DiscoveryFinished,
}

pub struct BlePeerDeviceAddress(pub esp_idf_sys::ble_addr_t);

impl std::hash::Hash for BlePeerDeviceAddress {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.type_.hash(state);
        self.0.val.hash(state);
    }
}

impl std::cmp::PartialEq for BlePeerDeviceAddress {
    fn eq(&self, other: &Self) -> bool {
        self.0.type_ == other.0.type_ && self.0.val == other.0.val
    }
}

impl std::cmp::Eq for BlePeerDeviceAddress {}

impl std::fmt::Display for BlePeerDeviceAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.0.val[5],
            self.0.val[4],
            self.0.val[3],
            self.0.val[2],
            self.0.val[1],
            self.0.val[0],
        )
    }
}

impl Clone for BlePeerDeviceAddress {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub struct BlePeerDevice {
    address: BlePeerDeviceAddress,
    ble: Weak<Mutex<Ble>>,
}

impl BlePeerDevice {
    pub fn new(address: BlePeerDeviceAddress, ble: Weak<Mutex<Ble>>) -> Self {
        Self { address, ble }
    }

    pub(super) fn shared_state_get<T>(
        &self,
        getter: impl FnOnce(&BlePeerDeviceSharedState) -> T,
    ) -> T {
        let arc = self.ble.upgrade().unwrap();
        let ble = arc.lock();
        let shared = ble.devices.get(&self.address).unwrap();
        getter(shared)
    }

    pub(super) fn shared_state_mod<T>(
        &self,
        setter: impl FnOnce(&mut BlePeerDeviceSharedState) -> T,
    ) -> T {
        let arc = self.ble.upgrade().unwrap();
        let mut ble = arc.lock();
        let shared = ble.devices.get_mut(&self.address).unwrap();
        setter(shared)
    }

    pub fn conn_handle(&self) -> Option<BleConnHandle> {
        self.shared_state_get(|shared| shared.conn_handle.clone())
    }

    pub fn name(&self) -> String {
        self.shared_state_get(|shared| shared.name.clone())
    }

    pub fn address(&self) -> &BlePeerDeviceAddress {
        &self.address
    }

    pub(crate) fn use_events_channel(&self, handler: impl FnOnce(&Receiver<BleConnectEvent>)) {
        let event_rx =
            self.shared_state_mod(|shared| std::mem::take(&mut shared.event_rx).unwrap());
        handler(&event_rx);
        self.shared_state_mod(|shared| {
            if shared.conn_handle.is_some() {
                shared.event_rx = Some(event_rx);
            }
        });
    }

    pub fn is_connected(&self) -> bool {
        self.conn_handle().is_some()
    }

    pub fn get_service_by_uuid(&mut self, uuid: &BleUUID) -> Result<Option<BlePeerService>> {
        let services = self.get_services()?;
        let svc = match services.into_iter().find(|svc| svc.uuid() == uuid) {
            Some(svc) => svc,
            None => return Ok(None),
        };
        Ok(Some(svc))
    }

    pub fn get_services(&mut self) -> Result<Vec<BlePeerService>> {
        log::info!("Retrieving services for device {}", self);

        if !self.is_connected() {
            anyhow::bail!("Device not connected");
        }

        let mut services = vec![];
        {
            // Callback.
            let (tx, rx) = std::sync::mpsc::channel();
            let services = &mut services;
            let mut callback: Box<dyn FnMut(BlePeerServiceDiscoveryEvent)> =
                Box::new(move |event| match event {
                    BlePeerServiceDiscoveryEvent::Discovery(svc) => {
                        log::info!("Found: {}", svc);
                        services.push(svc);
                    }
                    BlePeerServiceDiscoveryEvent::DiscoveryFinished => tx.send(()).unwrap(),
                });

            // Start the discovery thread.
            let cb_arg: *mut _ = &mut callback;
            unsafe {
                esp_idf_sys::ble_gattc_disc_all_svcs(
                    self.conn_handle().unwrap() as u16,
                    Some(BlePeerDevice::ble_on_gatt_disc_svc),
                    cb_arg as *mut esp_idf_sys::c_types::c_void,
                )
            };

            // Wait for results.
            loop {
                match rx.recv() {
                    Ok(_) => break,
                    Err(e) => anyhow::bail!("Error retrieving device services: {}", e),
                }
            }
        }

        Ok(services)
    }

    unsafe extern "C" fn ble_on_gatt_disc_svc(
        conn_handle: u16,
        error: *const esp_idf_sys::ble_gatt_error,
        svc: *const esp_idf_sys::ble_gatt_svc,
        cb_arg: *mut esp_idf_sys::c_types::c_void,
    ) -> esp_idf_sys::c_types::c_int {
        let cb_arg = (cb_arg as *mut Box<dyn FnMut(BlePeerServiceDiscoveryEvent)>)
            .as_mut()
            .unwrap();

        if !svc.is_null() {
            let svc = *svc;
            cb_arg(BlePeerServiceDiscoveryEvent::Discovery(BlePeerService {
                conn_handle,
                start_handle: svc.start_handle,
                end_handle: svc.end_handle,
                uuid: BleUUID::from(svc.uuid),
            }));
        }
        if (if error.is_null() { 0 } else { (*error).status }) == esp_idf_sys::BLE_HS_EDONE as u16 {
            cb_arg(BlePeerServiceDiscoveryEvent::DiscoveryFinished);
        }
        0
    }
}

impl std::fmt::Display for BlePeerDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "BlePeerDevice {{ addr={} name={} }}",
            self.address,
            self.name(),
        )
    }
}
