use super::{dev::BleConnHandle, uuid::BleUUID};
use anyhow::Result;

pub struct BlePeerDescriptor {
    conn_handle: BleConnHandle,
    chr_val_handle: u16,
    handle: u16,
    uuid: BleUUID,
}
impl std::fmt::Display for BlePeerDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "BlePeerDescriptor {{ conn_handle={}, chr_val_handle={} handle={} uuid={}}}",
            self.conn_handle,
            self.chr_val_handle,
            self.handle,
            self.uuid.to_string(),
        )
    }
}

impl BlePeerDescriptor {
    pub fn uuid(&self) -> &BleUUID {
        &self.uuid
    }
    fn write(&self, data: [u8; 1]) -> Result<()> {
        write(self.conn_handle, self.handle, &data)
    }
    pub fn write_no_response(&self, data: [u8; 1]) -> Result<()> {
        write_no_response(self.conn_handle, self.handle, &data)
    }
}

enum BlePeerDescriptorDiscoveryEvent {
    Discovery(BlePeerDescriptor),
    DiscoveryFinished,
}

pub struct BlePeerCharacteristic {
    pub(super) conn_handle: BleConnHandle,
    pub(super) def_handle: u16,
    pub(super) val_handle: u16,
    pub(super) end_handle: u16,
    pub(super) properties: u8,
    pub(super) uuid: BleUUID,
    // pub (super) service: BleService,
}

impl BlePeerCharacteristic {
    pub fn uuid(&self) -> &BleUUID {
        &self.uuid
    }

    pub fn can_broadcast(&self) -> bool {
        return (self.properties & esp_idf_sys::BLE_GATT_CHR_PROP_BROADCAST as u8) != 0;
    }

    pub fn can_indicate(&self) -> bool {
        return (self.properties & esp_idf_sys::BLE_GATT_CHR_PROP_INDICATE as u8) != 0;
    }

    pub fn can_notify(&self) -> bool {
        return (self.properties & esp_idf_sys::BLE_GATT_CHR_PROP_NOTIFY as u8) != 0;
    }

    pub fn can_read(&self) -> bool {
        return (self.properties & esp_idf_sys::BLE_GATT_CHR_PROP_READ as u8) != 0;
    }

    pub fn can_write(&self) -> bool {
        return (self.properties & esp_idf_sys::BLE_GATT_CHR_PROP_WRITE as u8) != 0;
    }

    pub fn can_write_no_response(&self) -> bool {
        return (self.properties & esp_idf_sys::BLE_GATT_CHR_PROP_WRITE_NO_RSP as u8) != 0;
    }

    pub fn write(&self, data: &[u8]) -> Result<()> {
        if !self.can_write() {
            anyhow::bail!("BLE chr: characteristic doesn't support writes");
        }
        write(self.conn_handle as u32, self.val_handle, data)
    }

    pub fn write_no_response(&self, data: &[u8]) -> Result<()> {
        if !self.can_write_no_response() {
            anyhow::bail!("BLE chr: characteristic doesn't support writes without response");
        }
        write_no_response(self.conn_handle, self.val_handle, data)
    }

    pub fn get_descriptor_by_uuid(&self, uuid: &BleUUID) -> Result<Option<BlePeerDescriptor>> {
        let descriptors = self.get_descriptors()?;
        let dsc = match descriptors.into_iter().find(|dsc| dsc.uuid() == uuid) {
            Some(dsc) => dsc,
            None => return Ok(None),
        };
        Ok(Some(dsc))
    }

    pub fn get_descriptors(&self) -> Result<Vec<BlePeerDescriptor>> {
        log::info!("Retrieving descriptors for service {}", self);

        let mut descriptors = vec![];
        {
            // Callback.
            let (tx, rx) = std::sync::mpsc::channel();
            let descriptors = &mut descriptors;
            let mut callback: Box<dyn FnMut(BlePeerDescriptorDiscoveryEvent)> =
                Box::new(move |event| match event {
                    BlePeerDescriptorDiscoveryEvent::Discovery(dsc) => {
                        log::info!("Found: {}", dsc);
                        descriptors.push(dsc);
                    }
                    BlePeerDescriptorDiscoveryEvent::DiscoveryFinished => tx.send(()).unwrap(),
                });

            // Start the discovery thread.
            let cb_arg: *mut _ = &mut callback;
            let rc = unsafe {
                esp_idf_sys::ble_gattc_disc_all_dscs(
                    self.conn_handle as u16,
                    self.val_handle,
                    self.end_handle,
                    Some(BlePeerCharacteristic::ble_on_gatt_disc_dscs),
                    cb_arg as *mut esp_idf_sys::c_types::c_void,
                )
            };
            if rc != 0 {
                return Err(anyhow::anyhow!(
                    "Error initiating GAP descriptor discovery procedure; rc={}",
                    rc
                ));
            }

            // Wait for results.
            loop {
                match rx.recv() {
                    Ok(_) => break,
                    Err(e) => anyhow::bail!("Error retrieving device descriptors: {}", e),
                }
            }
        }

        Ok(descriptors)
    }

    pub fn set_notify(&self, value: bool) -> Result<()> {
        if !self.can_notify() {
            anyhow::bail!("Characteristic doesn't support notifications");
        }

        let uuid = BleUUID::parse("0229")?; // 0x2902 al reves.
        let dsc = self.get_descriptor_by_uuid(&uuid)?;

        match dsc {
            Some(dsc) => {
                log::info!("Found descriptor for set_notify: {}", dsc);
                let data = match value {
                    // 1 notifications (push sin ack)
                    // 2 indications (push con ack)
                    true => [1],
                    // off.
                    false => [0],
                };
                dsc.write(data)?;
            }
            None => anyhow::bail!(
                "Invalid characteristic, supports notifications \
                but descriptor to configure them wasn't found"
            ),
        }

        Ok(())
    }

    unsafe extern "C" fn ble_on_gatt_disc_dscs(
        conn_handle: u16,
        error: *const esp_idf_sys::ble_gatt_error,
        chr_val_handle: u16,
        dsc: *const esp_idf_sys::ble_gatt_dsc,
        cb_arg: *mut esp_idf_sys::c_types::c_void,
    ) -> esp_idf_sys::c_types::c_int {
        let cb_arg = (cb_arg as *mut Box<dyn FnMut(BlePeerDescriptorDiscoveryEvent)>)
            .as_mut()
            .unwrap();
        if !dsc.is_null() {
            let dsc = *dsc;
            cb_arg(BlePeerDescriptorDiscoveryEvent::Discovery(
                BlePeerDescriptor {
                    conn_handle: conn_handle as BleConnHandle,
                    chr_val_handle,
                    handle: dsc.handle,
                    uuid: BleUUID::from(dsc.uuid),
                },
            ));
        }
        if (if error.is_null() { 0 } else { (*error).status }) == esp_idf_sys::BLE_HS_EDONE as u16 {
            cb_arg(BlePeerDescriptorDiscoveryEvent::DiscoveryFinished);
        }
        0
    }
}

impl std::fmt::Display for BlePeerCharacteristic {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "BleCharacteristic {{ conn_handle={} def_handle={} val_handle={} end_handle={} uuid={} }}",
            self.conn_handle,
            self.def_handle,
            self.val_handle,
            self.end_handle,
            self.uuid.to_string(),
        )
    }
}

type BlePeerWriteResult = u16;

pub fn write(conn_handle: BleConnHandle, attr_handle: u16, data: &[u8]) -> Result<()> {
    let mtu = unsafe { esp_idf_sys::ble_att_mtu(conn_handle as u16) };
    if data.len() > mtu.into() {
        anyhow::bail!("BLE chr: data ({}) exceeds MTU size ({})", data.len(), mtu);
    }

    // Convert data into a raw pointer that we will later cast to c_void.
    let data_len = data.len();
    let data: *const _ = data;

    // Create the callback that sends back the result through a channel.
    let (tx, rx) = std::sync::mpsc::channel();
    let mut callback: Box<dyn FnMut(BlePeerWriteResult)> =
        Box::new(move |event| tx.send(event).unwrap());

    // Write.
    let cb_arg: *mut _ = &mut callback;
    let rc = unsafe {
        esp_idf_sys::ble_gattc_write_flat(
            conn_handle as u16,
            attr_handle,
            data as *const esp_idf_sys::c_types::c_void,
            data_len as u16,
            Some(ble_gattc_on_write),
            cb_arg as *mut esp_idf_sys::c_types::c_void,
        )
    };
    if rc != 0 {
        anyhow::bail!(
            "BLE write: error writing conn_handle={} attr_handle={} rc={}",
            conn_handle,
            attr_handle,
            rc
        );
    }

    // Wait for results.
    let rc = loop {
        match rx.recv() {
            Ok(rc) => break rc,
            Err(e) => anyhow::bail!("BLE write: error waiting for response {}", e),
        }
    };
    if rc != 0 as u16 {
        anyhow::bail!(
            "BLE write: unexpected response conn_handle={} attr_handle={} rc={}",
            conn_handle,
            attr_handle,
            rc
        );
    }

    Ok(())
}

unsafe extern "C" fn ble_gattc_on_write(
    conn_handle: u16,
    error: *const esp_idf_sys::ble_gatt_error,
    attr: *mut esp_idf_sys::ble_gatt_attr,
    cb_arg: *mut esp_idf_sys::c_types::c_void,
) -> esp_idf_sys::c_types::c_int {
    log::info!(
        "ble_gattc_on_write conn_handle={} error={:?} attr={:?}",
        conn_handle,
        (*error),
        (*attr),
    );
    let cb_arg = (cb_arg as *mut Box<dyn FnMut(BlePeerWriteResult)>)
        .as_mut()
        .unwrap();
    cb_arg((*error).status);
    0
}

pub fn write_no_response(conn_handle: BleConnHandle, attr_handle: u16, data: &[u8]) -> Result<()> {
    let mtu = unsafe { esp_idf_sys::ble_att_mtu(conn_handle as u16) };
    if data.len() > mtu.into() {
        anyhow::bail!("BLE chr: data ({}) exceeds MTU size ({})", data.len(), mtu);
    }

    // Convert data into a raw pointer that we will later cast to c_void.
    let data_len = data.len();
    let data: *const _ = data;

    // Write.
    let rc = unsafe {
        esp_idf_sys::ble_gattc_write_no_rsp_flat(
            conn_handle as u16,
            attr_handle,
            data as *const esp_idf_sys::c_types::c_void,
            data_len as u16,
        )
    };
    if rc != 0 {
        anyhow::bail!(
            "BLE write_no_response: error writing conn_handle={} attr_handle={} rc={}",
            conn_handle,
            attr_handle,
            rc
        );
    }

    Ok(())
}
