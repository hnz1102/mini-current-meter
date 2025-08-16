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

const ADCRANGE : bool = true; // true: 40.96mV, false: 163.84mV
const CALIBRATION_USE: bool = true;    // Enable or disable calibration
const WIFI_DELAY_START: u64 = 0;

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
    #[default("50")]
    shunt_temp_coefficient: &'static str,
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
    let mut nvs = match EspNvs::new(nvs_default_partition, "storage", true) {
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

    // Initialize INA228 sensor
    match ADCRANGE {
        true => write_ina228_reg16(&sensor_i2c, 0x00, 0x0030)?, // Bit4: ADCRANGE=1(40.96mV), Bit5 Enables temperature compensation
        false => write_ina228_reg16(&sensor_i2c, 0x00, 0x0020)?, // Bit4: ADCRANGE=0(163.84mV), Bit5 Enables temperature compensation
    }
    let read_value = read_ina228_reg16(&sensor_i2c, 0x00)?;
    info!("INA228 Config Set to: {:04x}", read_value);

    // INA228 ADC Config
    let read_adc_config = read_ina228_reg16(&sensor_i2c, 0x01)?;
    info!("INA228 ADC Config Read: {:04x}", read_adc_config);
    // Mode: 0xF = Continuous bus voltage, shunt voltage and temperature
    // VBUSCT: 0x5 = 1052us Conversion Time for VBUS
    // VSHCT: 0x7 = 4120us Conversion Time for shunt voltage measurement
    // VTCT: 0x5 = 1052us Conversion Time for temperature measurement
    // AVG: 0x5 = 256 samples ADC sample averaging count, 0x6 = 512 samples, 0x7 = 1024 samples
    let write_adc_config : u16 = (0xF << 12) | (0x5 << 9) | (0x7 << 6) | (0x5 << 3) | 0x6; 
    write_ina228_reg16(&sensor_i2c, 0x01, write_adc_config)?;
    let read_adc_config = read_ina228_reg16(&sensor_i2c, 0x01)?;
    info!("INA228 ADC Config Set to: {:04x}", read_adc_config);

    // SHUNT_CAL
    let shunt_resistance = CONFIG.shunt_resistance.parse::<f32>().unwrap();
    let current_lsb = match ADCRANGE {
        true => {
            // 40.96mV range
            40.96 / 524_288.0
        },
        false => {
            // 163.84mV range
            163.84 / 524_288.0
        }
    };
    let shunt_cal_val = match ADCRANGE {
        true => 13107.2 * current_lsb * 1000_000.0 * shunt_resistance * 4.0, // 40.96mV range
        false => 13107.2 * current_lsb * 1000_000.0 * shunt_resistance, // 163.84mV range
    };
    let shunt_cal = shunt_cal_val as u16;
    info!("current_lsb={:?} shunt_cal_val={:?} shunt_cal={:?}", current_lsb, shunt_cal_val, shunt_cal);
    write_ina228_reg16(&sensor_i2c, 0x02, shunt_cal)?;
    let read_shunt_cal = read_ina228_reg16(&sensor_i2c, 0x02)?;
    info!("INA228 SHUNT_CAL Set to: {:04x}", read_shunt_cal);
    // Shunt Temperature Coefficient
    let shunt_temp_coefficient = CONFIG.shunt_temp_coefficient.parse::<u16>().unwrap();
    info!("Shunt Temperature Coefficient: {:?}", shunt_temp_coefficient);
    write_ina228_reg16(&sensor_i2c, 0x03, shunt_temp_coefficient)?;
    let read_shunt_temp_coefficient = read_ina228_reg16(&sensor_i2c, 0x03)?;
    info!("INA228 SHUNT_TEMP_COEFFICIENT Set to: {:04x}", read_shunt_temp_coefficient);

    // Temperature Measurement
    let temperature: f32 = read_ina228_reg16(&sensor_i2c, 0x06)? as f32 * 7.8125;
    info!("Initial Temperature Read: {:.2}°C", temperature / 1000.0);
    
    // Load calibration offsets from NVS
    let mut average_current_offset: f32 = {
        let mut buffer = [0u8; 4];
        match nvs.get_blob("current_offset", &mut buffer) {
            Ok(Some(data)) if data.len() == 4 => {
                let offset_bytes: [u8; 4] = [data[0], data[1], data[2], data[3]];
                let offset = f32::from_le_bytes(offset_bytes);
                info!("Loaded current offset from NVS: {:.6}A", offset);
                offset
            },
            Ok(Some(data)) => {
                info!("Invalid current offset size in NVS (got {} bytes), using default 0.0A", data.len());
                0.0
            },
            Ok(None) => {
                info!("No current offset found in NVS, using default 0.0A");
                0.0
            },
            Err(e) => {
                info!("Failed to read current offset from NVS: {:?}, using default 0.0A", e);
                0.0
            }
        }
    };
    
    let mut average_voltage_offset: f32 = {
        let mut buffer = [0u8; 4];
        match nvs.get_blob("voltage_offset", &mut buffer) {
            Ok(Some(data)) if data.len() == 4 => {
                let offset_bytes: [u8; 4] = [data[0], data[1], data[2], data[3]];
                let offset = f32::from_le_bytes(offset_bytes);
                info!("Loaded voltage offset from NVS: {:.6}V", offset);
                offset
            },
            Ok(Some(data)) => {
                info!("Invalid voltage offset size in NVS (got {} bytes), using default 0.0V", data.len());
                0.0
            },
            Ok(None) => {
                info!("No voltage offset found in NVS, using default 0.0V");
                0.0
            },
            Err(e) => {
                info!("Failed to read voltage offset from NVS: {:?}, using default 0.0V", e);
                0.0
            }
        }
    };
    
    // Display loaded calibration info
    if (average_current_offset != 0.0 || average_voltage_offset != 0.0) && CALIBRATION_USE {
        info!("Using stored calibration - Current offset: {:.6}A, Voltage offset: {:.6}V", 
              average_current_offset, average_voltage_offset);
    } else {
        info!("No calibration data found - using zero offsets");
        average_current_offset = 0.0;
        average_voltage_offset = 0.0;
    }

    // GPIO9 Button for channel selection (polling method)
    let channel_select_pin = peripherals.pins.gpio9;
    let mut channel_select_button = PinDriver::input(channel_select_pin)?;
    channel_select_button.set_pull(Pull::Up)?;

    // Temperature Logs
    let mut clogs = CurrentRecord::new();

    // WiFi
    let mut wifi_enable : bool = false;
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
    if WIFI_DELAY_START > 0 {
        wifi_device.as_mut().map(|wifi| {
            wifi::stop_wifi(wifi).unwrap();
        });
    }
    let start_time = SystemTime::now();
    loop {
        thread::sleep(Duration::from_millis(100));

        if SystemTime::now().duration_since(start_time).unwrap().as_secs() < WIFI_DELAY_START {
            wifi_enable = true;
        }
        else {
            if wifi_enable == false {
                if let Some(ref mut wifi) = wifi_device {
                    wifi_reconnect(wifi, &mut dp);
                }
            }
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
        }

        // Button polling with debounce and long press detection
        static mut LAST_BUTTON_STATE: bool = true;
        static mut BUTTON_PRESS_START_TIME: u64 = 0;
        static mut CALIBRATION_IN_PROGRESS: bool = false;
        static mut MESSAGE_CLEAR_TIME: u64 = 0;
        static mut LONG_PRESS_TRIGGERED: bool = false;  // Track if long press was already triggered
        
        const LONG_PRESS_TIME_MS: u64 = 2000;  // 2 seconds for calibration
        
        let current_button_state = channel_select_button.is_high();
        let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as u64;
        
        unsafe {
            // Clear message after timeout
            if MESSAGE_CLEAR_TIME > 0 && current_time >= MESSAGE_CLEAR_TIME {
                dp.set_err_message("".to_string());
                MESSAGE_CLEAR_TIME = 0;
            }
            
            if LAST_BUTTON_STATE && !current_button_state {
                BUTTON_PRESS_START_TIME = current_time;
                LONG_PRESS_TRIGGERED = false;  // Reset the trigger flag
                info!("Button press detected");
            }
            
            // Check for long press (2+ seconds) for calibration
            if !current_button_state && 
                (current_time - BUTTON_PRESS_START_TIME) >= LONG_PRESS_TIME_MS && 
                !CALIBRATION_IN_PROGRESS &&
                !LONG_PRESS_TRIGGERED {
                
                CALIBRATION_IN_PROGRESS = true;
                LONG_PRESS_TRIGGERED = true;
                info!("Long press detected - starting calibration...");
                dp.set_err_message("Calibrating...".to_string());
            
                // Perform calibration
                match calibration(&sensor_i2c, current_lsb) {
                    Ok((current_offset, voltage_offset)) => {
                        average_current_offset = current_offset;
                        average_voltage_offset = voltage_offset;
                        info!("Calibration completed - Current offset: {:.6}A, Voltage offset: {:.6}V", 
                                current_offset, voltage_offset);
                        
                        // Save calibration offsets to NVS
                        let current_offset_bytes = current_offset.to_le_bytes();
                        let voltage_offset_bytes = voltage_offset.to_le_bytes();
                        
                        match nvs.set_blob("current_offset", &current_offset_bytes) {
                            Ok(_) => {
                                info!("Current offset saved to NVS: {:.6}A", current_offset);
                            },
                            Err(e) => {
                                info!("Failed to save current offset to NVS: {:?}", e);
                            }
                        }
                        
                        match nvs.set_blob("voltage_offset", &voltage_offset_bytes) {
                            Ok(_) => {
                                info!("Voltage offset saved to NVS: {:.6}V", voltage_offset);
                            },
                            Err(e) => {
                                info!("Failed to save voltage offset to NVS: {:?}", e);
                            }
                        }
                        
                        dp.set_err_message("Calibration OK".to_string());
                        MESSAGE_CLEAR_TIME = current_time + 2000; // Clear after 2 seconds
                    },
                    Err(e) => {
                        info!("Calibration failed: {:?}", e);
                        dp.set_err_message("Calibration Failed".to_string());
                        MESSAGE_CLEAR_TIME = current_time + 2000; // Clear after 2 seconds
                    }
                }
            }
            
            // Button release detected (low to high transition after debounce)
            if !LAST_BUTTON_STATE && current_button_state {
                let press_duration = current_time - BUTTON_PRESS_START_TIME;
                
                if !CALIBRATION_IN_PROGRESS && press_duration < LONG_PRESS_TIME_MS {
                    // Short press - change channel
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
                }
                
                CALIBRATION_IN_PROGRESS = false;
                LONG_PRESS_TRIGGERED = false;  // Reset the trigger flag on button release
                info!("Button released after {}ms", press_duration);
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
        // let shunt_voltage_measured = match ADCRANGE {
        //     true => (read_ina228_reg24(&sensor_i2c, 0x04)? >> 4) as f32 * 78.125,
        //     false => (read_ina228_reg24(&sensor_i2c, 0x04)? >> 4) as f32 * 312.5,
        // };
        // info!("Shunt Voltage Measured: {:.2}nV", shunt_voltage_measured);
        // Power
        match power_read(&sensor_i2c, current_lsb) {
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
            let vbus = ((((vbus_buf[0] as u32) << 16 | (vbus_buf[1] as u32) << 8 | (vbus_buf[2] as u32)) >> 4) as f32 * 195.3125) / 1000_000.0;
            // info!("vbus_buf={:?} vbus={:?}", vbus_buf, vbus);
            return Ok(vbus);
        },
        Err(e) => {
            info!("{:?}", e);
            return Err(anyhow::anyhow!("Voltage Read Error"));
        }
    }
}

fn power_read(shared_i2c: &Arc<Mutex<i2c::I2cDriver>>, current_lsb: f32) -> anyhow::Result<f32> {
    let mut power_buf = [0u8; 3];
    let mut i2c = shared_i2c.lock().unwrap();
    i2c.write(0x40, &[0x08u8; 1], BLOCK)?;
    match i2c.read(0x40, &mut power_buf, BLOCK) {
        Ok(_v) => {
            let power_reg = ((power_buf[0] as u32) << 16 | (power_buf[1] as u32) << 8 | (power_buf[2] as u32)) as f32;
            let power = 3.2 * current_lsb * power_reg;
            return Ok(power);
        },
        Err(e) => {
            info!("{:?}", e);
            return Err(anyhow::anyhow!("Power Read Error"));
        }
    }
}

fn write_ina228_reg16(shared_i2c: &Arc<Mutex<i2c::I2cDriver>>, reg: u8, value: u16) -> anyhow::Result<()> {
    let mut config = [0u8; 3];
    config[0] = reg;
    config[1] = (value >> 8) as u8;
    config[2] = value as u8;
    let mut i2c = shared_i2c.lock().unwrap();
    i2c.write(0x40, &config, BLOCK)?;
    Ok(())
}

fn read_ina228_reg16(shared_i2c: &Arc<Mutex<i2c::I2cDriver>>, reg: u8) -> anyhow::Result<u16> {
    let mut data = [0u8; 2];
    let mut i2c = shared_i2c.lock().unwrap();
    i2c.write(0x40, &[reg; 1], BLOCK)?;
    i2c.read(0x40, &mut data, BLOCK)?;
    // info!("INA228 Reg {:02x} Read: {:02x} {:02x}", reg, data[0], data[1]);
    Ok(((data[0] as u16) << 8) | (data[1] as u16))
}

// fn read_ina228_reg24(shared_i2c: &Arc<Mutex<i2c::I2cDriver>>, reg: u8) -> anyhow::Result<u32> {
//     let mut data = [0u8; 3];
//     let mut i2c = shared_i2c.lock().unwrap();
//     i2c.write(0x40, &[reg; 1], BLOCK)?;
//     i2c.read(0x40, &mut data, BLOCK)?;
//     Ok(((data[0] as u32) << 16) | ((data[1] as u32) << 8) | (data[2] as u32))
// }

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

fn calibration(shared_i2c: &Arc<Mutex<i2c::I2cDriver>>, current_lsb: f32) -> anyhow::Result<(f32, f32)> {
    // INA228 Calibration
    // Take 300 samples to calculate average offset for current and voltage
    let mut average_current_offset = 0.0;
    let mut average_voltage_offset = 0.0;
    
    info!("Starting calibration - taking 300 samples over 3 seconds...");
    
    for i in 0..300 {
        match current_read(shared_i2c, current_lsb) {
            Ok(current) => {
                average_current_offset += current;
            },
            Err(e) => {
                return Err(anyhow::anyhow!("Current read error during calibration: {:?}", e));
            }
        }
        
        match voltage_read(shared_i2c) {
            Ok(voltage) => {
                average_voltage_offset += voltage;
            },
            Err(e) => {
                return Err(anyhow::anyhow!("Voltage read error during calibration: {:?}", e));
            }
        }
        
        // Log progress every 50 samples
        if i % 50 == 0 {
            info!("Calibration progress: {}/300 samples", i + 1);
        }
        
        thread::sleep(Duration::from_millis(10));
    }
    
    average_current_offset /= 300.0;
    average_voltage_offset /= 300.0;
    
    info!("Calibration completed - Average Current Offset: {:.6}A, Voltage Offset: {:.6}V", 
          average_current_offset, average_voltage_offset);
    
    Ok((average_current_offset, average_voltage_offset))
}