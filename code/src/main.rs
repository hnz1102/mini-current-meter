// This is mini-current-meter main program for ESP32-C3-WROOM.
// SPDX-License-Identifier: MIT
// Copyright (c) 2025 Hiroshi Nakajima

use std::{thread, time::Duration, sync::{Arc, Mutex}};
use esp_idf_hal::{prelude::*, i2c, gpio::*};
use esp_idf_hal::delay::BLOCK;
use esp_idf_hal::peripherals::Peripherals;
use log::*;
use std::time::SystemTime;
use esp_idf_hal::adc::oneshot::config::AdcChannelConfig;
use esp_idf_hal::adc::oneshot::config::Calibration;
use esp_idf_hal::adc::oneshot::*;
use esp_idf_hal::adc::attenuation::DB_11;
use esp_idf_hal::gpio::PinDriver;
use esp_idf_svc::sntp::{EspSntp, SyncStatus, SntpConf, OperatingMode, SyncMode};
use esp_idf_svc::wifi::EspWifi;
use esp_idf_svc::nvs::{EspNvs, EspNvsPartition, NvsDefault};
use chrono::{DateTime, Utc};

mod displayctl;
mod currentlogs;
mod wifi;
mod transfer;

use displayctl::{DisplayPanel, LoggingStatus, WifiStatus};
use currentlogs::{CurrentRecord, CurrentLog};
use transfer::Transfer;
use transfer::ServerInfo;

#[toml_cfg::toml_config]
pub struct Config {
    #[default("")]
    wifi_ssid: &'static str,
    #[default("")]
    wifi_psk: &'static str,
    #[default("")]
    influxdb_server: &'static str,
    #[default("0.005")]
    shunt_resistance: &'static str,
    #[default("")]
    influxdb_api_key: &'static str,
    #[default("")]
    influxdb_api: &'static str,
    #[default("")]
    influxdb_measurement: &'static str,
    #[default("")]
    influxdb_tag: &'static str,
    #[default("1023")]
    max_records: &'static str,
}

fn main() -> anyhow::Result<()> {
    esp_idf_sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // Initialize nvs
    unsafe {
        esp_idf_sys::nvs_flash_init();
    }
    
    // Parse configuration values
    let max_records = CONFIG.max_records.parse::<usize>().unwrap_or(1023);
    info!("Max records set to: {}", max_records);

    // Peripherals Initialize
    let peripherals = Peripherals::take().unwrap();
    
    // Shared I2C for both SSD1306 display and INA228 sensor
    let i2c = peripherals.i2c0;
    let scl = peripherals.pins.gpio7;
    let sda = peripherals.pins.gpio8;
    let config = i2c::I2cConfig::new().baudrate(100.kHz().into());
    let i2c_driver = i2c::I2cDriver::new(i2c, sda, scl, &config)?;
    
    // Clone the I2C driver for shared use (using Arc and Mutex for thread safety)
    use std::sync::{Arc, Mutex};
    let shared_i2c = Arc::new(Mutex::new(i2c_driver));
    
    // Create display with shared I2C
    let mut dp = DisplayPanel::new();
    let display_i2c = shared_i2c.clone();
    dp.start(display_i2c);

    // Initialize NVS
    let nvs_default_partition = EspNvsPartition::<NvsDefault>::take().unwrap();
    let nvs = match EspNvs::new(nvs_default_partition, "storage", true) {
        Ok(nvs) => { 
            info!("NVS storage area initialized"); 
            nvs 
        },
        Err(ref e) => {
            info!("NVS initialization failed {:?}", e);
            panic!("NVS initialization failed {:?}", e); 
        }
    };
    
    // Load current channel from NVS
    let mut channel: u8 = match nvs.get_u8("channel") {
        Ok(Some(ch)) => {
            info!("Loaded channel {} from NVS", ch);
            if ch >= 1 && ch <= 4 { ch } else { 1 } // Validate range
        },
        Ok(None) => {
            info!("No channel found in NVS, using default channel 1");
            1
        },
        Err(e) => {
            info!("Failed to read channel from NVS: {:?}, using default channel 1", e);
            1
        }
    };

    // Load configuration
    let server_info = ServerInfo::new(CONFIG.influxdb_server.to_string(), 
        CONFIG.influxdb_api_key.to_string(),
        CONFIG.influxdb_api.to_string(),
        CONFIG.influxdb_measurement.to_string(),
        CONFIG.influxdb_tag.to_string());

    // Use the shared I2C for INA sensor
    let sensor_i2c = shared_i2c.clone();


    let mut config_read_buf = [0u8; 2];
    let mut config_write_buf = [0u8; 3];
    // Config
    {
        let mut i2c_lock = sensor_i2c.lock().unwrap();
        i2c_lock.write(0x40, &[0x01u8; 1], BLOCK)?;
        i2c_lock.read(0x40, &mut config_read_buf, BLOCK)?;
    }
    config_write_buf[0] = 0x01;
    config_write_buf[1] = config_read_buf[0];
    config_write_buf[2] = (config_read_buf[1] & 0xF8) | 0x03; // 0x00: 1avg, 0x02: 16avg, 0x03: 64avg
    {
        let mut i2c_lock = sensor_i2c.lock().unwrap();
        i2c_lock.write(0x40, &config_write_buf, BLOCK)?;
    }
    // SHUNT_CAL
    let shunt_resistance = CONFIG.shunt_resistance.parse::<f32>().unwrap();
    let current_lsb = 16.384 / 524_288.0;
    let shunt_cal_val = 13107.2 * current_lsb * 1000_000.0 * shunt_resistance;
    let shunt_cal = shunt_cal_val as u32;
    info!("current_lsb={:?} shunt_cal_val={:?} shunt_cal={:?}", current_lsb, shunt_cal_val, shunt_cal);
    let mut shunt_cal_buf = [0u8; 3];
    shunt_cal_buf[0] = 0x02;
    shunt_cal_buf[1] = (shunt_cal >> 8) as u8;
    shunt_cal_buf[2] = (shunt_cal & 0xFF) as u8;
    {
        let mut i2c_lock = sensor_i2c.lock().unwrap();
        i2c_lock.write(0x40, &shunt_cal_buf, BLOCK)?;
    }
    // calibration read
    let average_current_offset :f32 = 0.0;
    let average_voltage_offset :f32 = 0.0;
    // let (current_offset, voltage_offset) = calibration(&mut i2cdrv, current_lsb)?;
    // average_current_offset = current_offset;
    // average_voltage_offset = voltage_offset;    
    // read back
    let mut shunt_cal_read_buf = [0u8; 2];
    {
        let mut i2c_lock = sensor_i2c.lock().unwrap();
        i2c_lock.write(0x40, &[0x02u8; 1], BLOCK)?;
        i2c_lock.read(0x40, &mut shunt_cal_read_buf, BLOCK)?;
    }
    if shunt_cal_read_buf[0] != shunt_cal_buf[1] || shunt_cal_read_buf[1] != shunt_cal_buf[2] {
        info!("shunt_cal_write_buf={:?}", shunt_cal_buf);
        info!("shunt_cal_read_buf={:?}", shunt_cal_read_buf);
        info!("Shunt Calibration Error");
        dp.set_err_message("Shunt Calibration Error".to_string());
    }

    // GPIO9 Button for channel selection (polling method)
    let channel_select_pin = peripherals.pins.gpio9;
    let mut channel_select_button = PinDriver::input(channel_select_pin)?;
    channel_select_button.set_pull(Pull::Up)?;

    // Temperature Logs
    let mut clogs = CurrentRecord::new();

    // WiFi
    let mut wifi_enable : bool;
    let mut wifi_device: Option<Box<EspWifi>>;
    match wifi::wifi_connect(peripherals.modem, CONFIG.wifi_ssid, CONFIG.wifi_psk) {
        Ok(wifi) => { 
            wifi_device = Some(wifi);
        },
        Err(ref e) => { 
            info!("{:?}", e); 
            wifi_device = None;
        }
    }

    // NTP Server
    let sntp_conf = SntpConf {
        servers: ["time.aws.com",
                    "time.google.com",
                    "time.cloudflare.com",
                    "ntp.nict.jp"],
        operating_mode: OperatingMode::Poll,
        sync_mode: SyncMode::Immediate,
    };
    let ntp = EspSntp::new(&sntp_conf).unwrap();

    // NTP Sync
    info!("NTP Sync Start..");

    // wait for sync
    let mut sync_count = 0;
    while ntp.get_sync_status() != SyncStatus::Completed {
        sync_count += 1;
        if sync_count > 1000 {
            info!("NTP Sync Timeout");
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let now = SystemTime::now();
    let dt_now : DateTime<Utc> = now.into();
    let formatted = format!("{}", dt_now.format("%Y-%m-%d %H:%M:%S"));
    info!("NTP Sync Completed: {}", formatted);

    let mut txd =  Transfer::new(server_info);
    txd.start()?;
    
    // Initialize with loaded channel tag
    let mut tag = format!("ch{}", channel);
    txd.set_tag(tag.clone());
    info!("Using channel {} (tag: {})", channel, tag);
    
    // Set initial channel on display
    dp.set_channel(channel as u32);
    
    // ADC GPIO0
    let mut adc = AdcDriver::new(peripherals.adc1)?;
    let mut adc_config = AdcChannelConfig {
        attenuation: DB_11,
        calibration: Calibration::Curve, // Use curve calibration for better accuracy
        ..Default::default()
    };
    let mut adc_pin = AdcChannelDriver::new(&mut adc, peripherals.pins.gpio3, &mut adc_config)?;

    // loop
    let mut logging_start = true;
    let mut logging_stopped_by_buffer_full = false;  // Track if logging was stopped due to buffer full
    let mut rssi : i32;
    loop {
        thread::sleep(Duration::from_millis(100));

        // Get RSSI
        rssi = wifi::get_rssi();
        dp.set_wifi_rssi(rssi);
        if rssi == 0 {
            if let Some(ref mut wifi) = wifi_device {
                if wifi_reconnect(wifi, &mut dp) {
                    wifi_enable = true;
                } else {
                    wifi_enable = false;
                }
            } else {
                dp.set_wifi_status(WifiStatus::Disconnected);
                wifi_enable = false;
            }
        }
        else {
            dp.set_wifi_status(WifiStatus::Connected);
            wifi_enable = true;
        }

        // Button polling with debounce
        static mut LAST_BUTTON_STATE: bool = true;
        static mut BUTTON_PRESS_TIME: u64 = 0;
        
        let current_button_state = channel_select_button.is_high();
        let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as u64;
        
        unsafe {
            if LAST_BUTTON_STATE && !current_button_state {
                // Button pressed (high to low transition)
                channel += 1;
                if channel > 4 {
                    channel = 1;
                }
                tag = format!("ch{}", channel);
                info!("Channel changed to {}", tag);
                dp.set_channel(channel as u32);
                txd.set_tag(tag.clone());
                
                // Save current channel to NVS
                match nvs.set_u8("channel", channel) {
                    Ok(_) => {
                        info!("Channel {} saved to NVS", channel);
                    },
                    Err(e) => {
                        info!("Failed to save channel to NVS: {:?}", e);
                    }
                }                   
                BUTTON_PRESS_TIME = current_time;
            }
            LAST_BUTTON_STATE = current_button_state;
        }

        if wifi_enable == false{
            dp.set_wifi_status(WifiStatus::Disconnected);
        }
        else {
            dp.set_wifi_status(WifiStatus::Connected);
        }

        if logging_start == true {
            //startstop_led.set_high()?;
            dp.set_current_status(LoggingStatus::Start);
        }
        else {
            //startstop_led.set_low()?;
            dp.set_current_status(LoggingStatus::Stop);
        }

       // Read Current/Voltage
        let mut data = CurrentLog::default();
        // Timestamp
        let now = SystemTime::now();
        // set clock in ns
        data.clock = now.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos();

        // Voltage
        match voltage_read(&sensor_i2c) {
            Ok(vbus) => {
                data.voltage = vbus - average_voltage_offset;
                // info!("vbus={:?} {:?}V", vbus_buf, data.voltage);
            },
            Err(e) => {
                info!("{:?}", e);
//                dp.set_message(format!("{:?}", e), true, 1000);
            }
        }
        // Current
        match current_read(&sensor_i2c, current_lsb) {
            Ok(current) => {
                data.current = current - average_current_offset;
            },
            Err(e) => {
                info!("{:?}", e);
                // dp.set_message(format!("{:?}", e), true, 1000);
            }
        }
        // Power
        match power_read(&sensor_i2c) {
            Ok(power) => {
                data.power = power;
            },
            Err(e) => {
                info!("{:?}", e);
                // dp.set_message(format!("{:?}", e), true, 1000);
            }
        }

        // battery voltage 
        data.battery =  adc_pin.read().unwrap() as f32 * 2.0 / 1000.0;
        // info!("voltage={:.2}V current={:.5}A power={:.5}W battery={:.2}V",
        //     data.voltage, data.current, data.power, data.battery);
        dp.set_battery(data.battery);
        dp.set_voltage(data.voltage, data.current, data.power);
        if logging_start {
            clogs.record(data);
        }
        let current_record = clogs.get_size();
        if current_record >= max_records {
            logging_start = false;  // Auto stop logging if buffer is full.
            logging_stopped_by_buffer_full = true;  // Mark that logging was stopped due to buffer full
        }
        
        // Restart logging if it was stopped due to buffer full and buffer usage drops below 50%
        if logging_stopped_by_buffer_full && !logging_start && current_record < max_records / 2 {
            logging_start = true;
            logging_stopped_by_buffer_full = false;
            info!("Logging restarted: buffer usage dropped below 50% ({}/{})", current_record, max_records);
        }
        
        dp.set_buffer_watermark((current_record as u32) * 100 / max_records as u32);

        if wifi_enable == true && current_record > 0 {
            let logs = clogs.get_all_data();
            let txcount = txd.set_transfer_data(logs);
            if txcount > 0 {
                clogs.remove_data(txcount);
            }
        }
    }
}

fn current_read(shared_i2c: &Arc<Mutex<i2c::I2cDriver>>, current_lsb: f32) -> anyhow::Result<f32> {
    let mut curt_buf  = [0u8; 3];
    let mut i2c = shared_i2c.lock().unwrap();
    i2c.write(0x40, &[0x07u8; 1], BLOCK)?;
    match i2c.read(0x40, &mut curt_buf, BLOCK) {
        Ok(_v) => {
            let current_reg : f32;
            if curt_buf[0] & 0x80 == 0x80 {
                current_reg = (0x100000 - (((curt_buf[0] as u32) << 16 | (curt_buf[1] as u32) << 8 | (curt_buf[2] as u32)) >> 4)) as f32 * -1.0;
            }
            else {
                current_reg = (((curt_buf[0] as u32) << 16 | (curt_buf[1] as u32) << 8 | (curt_buf[2] as u32)) >> 4) as f32;
            }
            return Ok(current_lsb * current_reg);
        },
        Err(e) => {
            info!("{:?}", e);
            return Err(anyhow::anyhow!("Current Read Error"));
        }
    }
}

fn voltage_read(shared_i2c: &Arc<Mutex<i2c::I2cDriver>>) -> anyhow::Result<f32> {
    let mut vbus_buf  = [0u8; 3];
    let mut i2c = shared_i2c.lock().unwrap();
    i2c.write(0x40, &[0x05u8; 1], BLOCK)?;
    match i2c.read(0x40, &mut vbus_buf, BLOCK){
        Ok(_v) => {
            let vbus = ((((vbus_buf[0] as u32) << 16 | (vbus_buf[1] as u32) << 8 | (vbus_buf[2] as u32)) >> 4) as f32 * 193.3125) / 1000_000.0;
            // info!("vbus_buf={:?} vbus={:?}", vbus_buf, vbus);
            return Ok(vbus);
        },
        Err(e) => {
            info!("{:?}", e);
            return Err(anyhow::anyhow!("Voltage Read Error"));
        }
    }
}

fn power_read(shared_i2c: &Arc<Mutex<i2c::I2cDriver>>) -> anyhow::Result<f32> {
    let mut power_buf = [0u8; 3];
    let mut i2c = shared_i2c.lock().unwrap();
    i2c.write(0x40, &[0x08u8; 1], BLOCK)?;
    match i2c.read(0x40, &mut power_buf, BLOCK) {
        Ok(_v) => {
            let power_reg = ((power_buf[0] as u32) << 16 | (power_buf[1] as u32) << 8 | (power_buf[2] as u32)) as f32;
            let power = 3.2 * 16.384 / 524_288.0 * power_reg;
            return Ok(power);
        },
        Err(e) => {
            info!("{:?}", e);
            return Err(anyhow::anyhow!("Power Read Error"));
        }
    }
}

fn wifi_reconnect(wifi_dev: &mut Box<EspWifi>, dp: &mut DisplayPanel) -> bool{
    // display on
    dp.set_wifi_status(WifiStatus::Connecting);
    unsafe {
        esp_idf_sys::esp_wifi_start();
    }
    match wifi_dev.connect() {
        Ok(_) => { info!("Wifi connected"); true},
        Err(ref e) => { info!("{:?}", e); false }
    }
}