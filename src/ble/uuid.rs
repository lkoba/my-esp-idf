use anyhow::Result;
use esp_idf_sys::{
    ble_uuid128_t, ble_uuid16_t, ble_uuid_any_t, ble_uuid_t, BLE_UUID_TYPE_128, BLE_UUID_TYPE_16,
};

#[derive(Clone, Copy)]
pub struct BleUUID {
    ble_uuid: ble_uuid_any_t,
}

impl BleUUID {
    pub fn parse(value: &str) -> Result<BleUUID> {
        let value = value.replace("-", "");
        let ble_uuid = match value.len() {
            4 => ble_uuid_any_t {
                u16_: ble_uuid16_t {
                    u: ble_uuid_t {
                        type_: BLE_UUID_TYPE_16 as u8,
                    },
                    value: {
                        let value = &value
                            .as_bytes()
                            .chunks(2)
                            .map(|i| u8::from_str_radix(std::str::from_utf8(i).unwrap(), 16))
                            .rev()
                            .collect::<Result<Vec<u8>, _>>()?[0..2];
                        ((value[0] as u16) << 8) | value[1] as u16
                    },
                },
            },
            32 => ble_uuid_any_t {
                u128_: ble_uuid128_t {
                    u: ble_uuid_t {
                        type_: BLE_UUID_TYPE_128 as u8,
                    },
                    value: value
                        .as_bytes()
                        .chunks(2)
                        .map(|i| u8::from_str_radix(std::str::from_utf8(i).unwrap(), 16))
                        .rev()
                        .collect::<Result<Vec<u8>, _>>()?[0..16]
                        .try_into()?,
                },
            },
            _ => {
                anyhow::bail!("Unrecognized UUID string length");
            }
        };
        Ok(Self { ble_uuid })
    }

    pub fn native(&self) -> &ble_uuid_any_t {
        &self.ble_uuid
    }
}

impl From<ble_uuid_any_t> for BleUUID {
    fn from(value: ble_uuid_any_t) -> Self {
        Self { ble_uuid: value }
    }
}

impl ToString for BleUUID {
    fn to_string(&self) -> String {
        unsafe {
            match self.ble_uuid {

                ble_uuid_any_t { u16_ } if u16_.u.type_ == BLE_UUID_TYPE_16 as u8 => format!(
                    "{:04x}", u16_.value,
                ),
                ble_uuid_any_t { u128_ } if u128_.u.type_ == BLE_UUID_TYPE_128 as u8 => format!(
                    "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                    u128_.value[15],
                    u128_.value[14],
                    u128_.value[13],
                    u128_.value[12],
                    u128_.value[11],
                    u128_.value[10],
                    u128_.value[9],
                    u128_.value[8],
                    u128_.value[7],
                    u128_.value[6],
                    u128_.value[5],
                    u128_.value[4],
                    u128_.value[3],
                    u128_.value[2],
                    u128_.value[1],
                    u128_.value[0],
                ),
                _ => {
                    panic!("unsupported uuid type");
                }
            }
        }
    }
}

impl PartialEq for BleUUID {
    fn eq(&self, other: &Self) -> bool {
        unsafe {
            if self.ble_uuid.u.type_ != other.ble_uuid.u.type_ {
                return false;
            }
            match self.ble_uuid {
                ble_uuid_any_t { u16_: a } if a.u.type_ == BLE_UUID_TYPE_16 as u8 => {
                    match other.ble_uuid {
                        ble_uuid_any_t { u16_: b } => a.value == b.value,
                    }
                }
                ble_uuid_any_t { u128_: a } if a.u.type_ == BLE_UUID_TYPE_128 as u8 => {
                    match other.ble_uuid {
                        ble_uuid_any_t { u128_: b } => a.value == b.value,
                    }
                }
                _ => {
                    panic!("unsupported uuid type");
                }
            }
        }
    }
}

impl std::fmt::Debug for BleUUID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BleUUID").field("ble_uuid", &0).finish()
    }
}
