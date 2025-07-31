use std::time::Duration;
use std::thread;

use esp_idf_hal::peripheral;
use esp_idf_svc::{eventloop::EspSystemEventLoop, wifi::EspWifi};
use esp_idf_svc::wifi::{ClientConfiguration, Configuration};
use anyhow::bail;
use anyhow::Result;
use log::*;

pub fn wifi_connect<'d> (
    modem: impl peripheral::Peripheral<P = esp_idf_hal::modem::Modem> + 'static,
    ssid: &'d str,
    pass: &'d str,
) -> Result<Box<EspWifi<'d>>> {
  
    let sys_event_loop = EspSystemEventLoop::take().unwrap();
    let mut wifi = Box::new(EspWifi::new(modem, sys_event_loop.clone(), None).unwrap());

    info!("Setting WiFi configuration...");
    
    // Set configuration first, then start
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid.try_into().map_err(|_| anyhow::anyhow!("Failed to convert SSID"))?,
        password: pass.try_into().map_err(|_| anyhow::anyhow!("Failed to convert password"))?,
        ..Default::default()
    })).map_err(|e| anyhow::anyhow!("Failed to set WiFi configuration: {:?}", e))?;

    info!("Starting WiFi...");
    wifi.start().map_err(|e| anyhow::anyhow!("Failed to start WiFi: {:?}", e))?;
    
    // Small delay to let WiFi initialize
    thread::sleep(Duration::from_millis(100));
    
    info!("Connecting to WiFi network: {}", ssid);

    info!("Connecting to WiFi network: {}", ssid);
    wifi.connect().map_err(|e| anyhow::anyhow!("Failed to connect to WiFi: {:?}", e))?;
    
    let mut timeout = 0;
    while !wifi.is_connected().map_err(|e| anyhow::anyhow!("Failed to check WiFi status: {:?}", e))? {
        thread::sleep(Duration::from_secs(1));
        timeout += 1;
        info!("Waiting for WiFi connection... ({}/30)", timeout);
        if timeout > 30 {
            bail!("WiFi connection timeout after 30 seconds");
        }
    }

    info!("WiFi connected successfully");
    Ok(wifi)
}

pub fn get_rssi() -> i32 {
    unsafe {
        let mut rssi : i32 = 0;
        esp_idf_sys::esp_wifi_sta_get_rssi(&mut rssi);
        rssi
    }
}