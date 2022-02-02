use std::sync::Arc;

use anyhow::Result;
use embedded_svc::wifi::Wifi as _Wifi;
use esp_idf_svc::{
    netif::EspNetifStack, nvs::EspDefaultNvs, sysloop::EspSysLoopStack, wifi::EspWifi,
};

pub struct Wifi {
    wifi: EspWifi,
    power_save_mode: esp_idf_sys::wifi_ps_type_t,
}

impl Wifi {
    pub fn new() -> Result<Self> {
        Wifi::new_no_auto(
            Arc::new(EspNetifStack::new()?),
            Arc::new(EspSysLoopStack::new()?),
            Arc::new(EspDefaultNvs::new()?),
            esp_idf_sys::wifi_ps_type_t_WIFI_PS_NONE,
        )
    }

    pub fn new_no_auto(
        netif_stack: Arc<EspNetifStack>,
        sys_loop_stack: Arc<EspSysLoopStack>,
        default_nvs: Arc<EspDefaultNvs>,
        power_save_mode: esp_idf_sys::wifi_ps_type_t,
    ) -> Result<Self> {
        let wifi = EspWifi::new(
            netif_stack.clone(),
            sys_loop_stack.clone(),
            default_nvs.clone(),
        )?;
        Ok(Wifi {
            wifi,
            power_save_mode,
        })
    }

    pub fn begin(&mut self, ssid: &str, password: &str) -> Result<()> {
        let channel = None;

        // STA Mode.
        self.wifi
            .set_configuration(&embedded_svc::wifi::Configuration::Client(
                embedded_svc::wifi::ClientConfiguration {
                    ssid: ssid.into(),
                    password: password.into(),
                    channel,
                    ..Default::default()
                },
            ))?;

        // STA + AP Mode.
        // self.wifi
        //     .set_configuration(&embedded_svc::wifi::Configuration::Mixed(
        //         embedded_svc::wifi::ClientConfiguration {
        //             ssid: ssid.into(),
        //             password: password.into(),
        //             channel,
        //             ..Default::default()
        //         },
        //         embedded_svc::wifi::AccessPointConfiguration {
        //             ssid: "aptest".into(),
        //             channel: channel.unwrap_or(1),
        //             ..Default::default()
        //         },
        //     ))?;

        // wifi.set_configuration(&embedded_svc::wifi::Configuration::Client(
        //     embedded_svc::wifi::ClientConfiguration {
        //         ssid: ssid.into(),
        //         password: pass.into(),
        //         channel,
        //         ..Default::default()
        //     },
        // ))?;

        unsafe {
            esp_idf_sys::esp!(esp_idf_sys::esp_wifi_set_ps(self.power_save_mode))?;
        }

        if self.wifi.get_status().is_operating() {
            Ok(())
        } else {
            Err(anyhow::Error::msg("status is not operational"))
        }
    }

    pub fn get_gateway_ip(&self) -> Result<std::net::Ipv4Addr> {
        let status = self.wifi.get_status();
        if let embedded_svc::wifi::Status(
            embedded_svc::wifi::ClientStatus::Started(
                embedded_svc::wifi::ClientConnectionStatus::Connected(
                    embedded_svc::wifi::ClientIpStatus::Done(ip_settings),
                ),
            ),
            embedded_svc::wifi::ApStatus::Started(embedded_svc::wifi::ApIpStatus::Done),
        ) = status
        {
            Ok(ip_settings.subnet.gateway)
        } else {
            Err(anyhow::Error::msg(format!(
                "Unexpected Wifi status: {:?}",
                status
            )))
        }
    }

    pub fn set_power_save_mode(
        &mut self,
        power_save_mode: esp_idf_sys::wifi_ps_type_t,
    ) -> Result<()> {
        self.power_save_mode = power_save_mode;
        unsafe {
            esp_idf_sys::esp!(esp_idf_sys::esp_wifi_set_ps(self.power_save_mode))?;
        }
        Ok(())
    }
}

impl Drop for Wifi {
    fn drop(&mut self) {
        log::info!("Wifi dropping ...");
    }
}
