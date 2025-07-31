use log::*;
use std::{thread, time::Duration, sync::Arc, sync::Mutex};
use esp_idf_hal::i2c;
use ssd1306::{I2CDisplayInterface, prelude::*, Ssd1306};
use embedded_graphics::{
    mono_font::{ascii::{FONT_10X20, FONT_5X8, FONT_6X10}, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    text::{Text},
    geometry::{Point, Size},
    prelude::*,
    image::Image,
    primitives::{Rectangle, PrimitiveStyle},
};
use tinybmp::Bmp;

pub enum LoggingStatus {
    Start,
    Stop,
}

pub enum WifiStatus {
    Disconnected,
    Connecting,
    Connected,
}

struct DisplayText {
    voltage: f32,
    current: f32,
    power: f32,
    wifi_rssi: i32,
    message: String,
    battery: f32,
    status: LoggingStatus,
    wifi: WifiStatus,
    buffer_water_mark: u32,
    channel: u32,
    voltage_range: u8,  // 0=mV, 1=V
    current_range: u8,  // 0=mA, 1=A
    power_range: u8,    // 0=mW, 1=W
}

pub struct DisplayPanel {
    txt: Arc<Mutex<DisplayText>>
}

impl DisplayPanel {

    pub fn new() -> DisplayPanel {
        DisplayPanel { txt: Arc::new(Mutex::new(
            DisplayText {voltage: 0.0,
                         message: "".to_string(),
                         current: 0.0,
                         power: 0.0,
                         wifi_rssi: 0,
                         battery: 0.0,
                         status: LoggingStatus::Stop,
                         wifi: WifiStatus::Disconnected,
                         buffer_water_mark: 0,
                         channel: 1, // Default channel
                         voltage_range: 1, // Default to V
                         current_range: 1, // Default to A
                         power_range: 1,   // Default to W
                     })) }
    }

    pub fn start(&mut self, shared_i2c: Arc<Mutex<i2c::I2cDriver<'static>>>)
    {
        let txt = self.txt.clone();
        let _th = thread::spawn(move || {
            info!("Start Display Thread.");
            
            // Create a simple wrapper that implements the required traits for SSD1306
            struct I2CWrapper {
                driver: Arc<Mutex<i2c::I2cDriver<'static>>>,
            }
            
            impl embedded_hal_0_2::blocking::i2c::Write for I2CWrapper {
                type Error = ();
                
                fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Self::Error> {
                    let mut driver = self.driver.lock().unwrap();
                    driver.write(address, bytes, esp_idf_hal::delay::BLOCK).map_err(|_| ())
                }
            }
            
            let wrapper = I2CWrapper { driver: shared_i2c };
            let interface = I2CDisplayInterface::new(wrapper);
            let mut display = Ssd1306::new(interface, 
                DisplaySize128x64,
                ssd1306::prelude::DisplayRotation::Rotate0)
                .into_buffered_graphics_mode();
                
            if let Err(e) = display.init() {
                info!("Display init failed: {:?}", e);
                return;
            }
            
            let style_large = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
            // let style_large_inv = MonoTextStyleBuilder::new()
            //     .font(&FONT_10X20)
            //     .text_color(BinaryColor::Off)
            //     .background_color(BinaryColor::On)
            //     .build();
            let style_small = MonoTextStyle::new(&FONT_5X8, BinaryColor::On);
            let style_middle = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
            let style_middle_inv = MonoTextStyleBuilder::new()
                .font(&FONT_6X10)
                .text_color(BinaryColor::Off)
                .background_color(BinaryColor::On)
                .build();
            
            // Wifi BMP
            let wifibmp0 = Bmp::from_slice(include_bytes!("./img/wifi-0.bmp")).unwrap();
            let wifi_img0: Image<Bmp<BinaryColor>> = Image::new(&wifibmp0, Point::new(108,20));
            let wifibmp1 = Bmp::from_slice(include_bytes!("./img/wifi-1.bmp")).unwrap();
            let wifi_img1: Image<Bmp<BinaryColor>> = Image::new(&wifibmp1, Point::new(108,20));
            let wifibmp2 = Bmp::from_slice(include_bytes!("./img/wifi-2.bmp")).unwrap();
            let wifi_img2: Image<Bmp<BinaryColor>> = Image::new(&wifibmp2, Point::new(108,20));
            let wifibmp3 = Bmp::from_slice(include_bytes!("./img/wifi-3.bmp")).unwrap();
            let wifi_img3: Image<Bmp<BinaryColor>> = Image::new(&wifibmp3, Point::new(108,20));
            let wifibmp4 = Bmp::from_slice(include_bytes!("./img/wifi-4.bmp")).unwrap();
            let wifi_img4: Image<Bmp<BinaryColor>> = Image::new(&wifibmp4, Point::new(108,20));

            // Battery BMP
            let bat_x = 112;
            let bat_y = 42;
            let bat0 = Bmp::from_slice(include_bytes!("./img/battery-0.bmp")).unwrap();
            let bat0_img: Image<Bmp<BinaryColor>> = Image::new(&bat0, Point::new(bat_x, bat_y));
            let bat20 = Bmp::from_slice(include_bytes!("./img/battery-20.bmp")).unwrap();
            let bat20_img: Image<Bmp<BinaryColor>> = Image::new(&bat20, Point::new(bat_x, bat_y));
            let bat40 = Bmp::from_slice(include_bytes!("./img/battery-40.bmp")).unwrap();
            let bat40_img: Image<Bmp<BinaryColor>> = Image::new(&bat40, Point::new(bat_x, bat_y));
            let bat60 = Bmp::from_slice(include_bytes!("./img/battery-60.bmp")).unwrap();
            let bat60_img: Image<Bmp<BinaryColor>> = Image::new(&bat60, Point::new(bat_x, bat_y));
            let bat80 = Bmp::from_slice(include_bytes!("./img/battery-80.bmp")).unwrap();
            let bat80_img: Image<Bmp<BinaryColor>> = Image::new(&bat80, Point::new(bat_x, bat_y));
            let bat100 = Bmp::from_slice(include_bytes!("./img/battery-100.bmp")).unwrap();
            let bat100_img: Image<Bmp<BinaryColor>> = Image::new(&bat100, Point::new(bat_x, bat_y));
            let usbpwr = Bmp::from_slice(include_bytes!("./img/usb-power.bmp")).unwrap();
            let usbpwr_img: Image<Bmp<BinaryColor>> = Image::new(&usbpwr, Point::new(bat_x, bat_y));

            // Clear display
            display.clear();
            display.flush().unwrap();
            
            let mut loopcount = 0;
            let mut battery_level = 0;
            
            // Previous values for change detection
            let mut prev_voltage = -1.0;
            let mut prev_current = -1.0;
            let mut prev_power = -1.0;
            let mut prev_voltage_range = 255;
            let mut prev_current_range = 255;
            let mut prev_power_range = 255;
            let mut prev_status = LoggingStatus::Stop;
            let mut prev_wifi_status = WifiStatus::Disconnected;
            let mut prev_wifi_rssi = -999;
            let mut prev_buffer_wm = 999;
            let mut prev_battery = -1.0;
            let mut prev_battery_level = 999;
            let mut prev_channel = 0;
            let mut prev_message = String::new();
            let mut prev_loopcount_display = 0;
            
            loop {
                let mut lck = txt.lock().unwrap();
                loopcount += 1;
                if loopcount > 15 {
                    loopcount = 0;
                }

                // Auto-range voltage display with hysteresis
                let voltage = lck.voltage;
                let voltage_abs = voltage.abs();
                match lck.voltage_range {
                    0 => { // mV range
                        if voltage_abs >= 2.0 { // 2V threshold to go up
                            lck.voltage_range = 1;
                        }
                    },
                    1 => { // V range
                        if voltage_abs < 1.5 { // 1.5V threshold to go down
                            lck.voltage_range = 0;
                        }
                    },
                    _ => {
                        lck.voltage_range = 1;
                    }
                }

                // Auto-range current display with hysteresis
                let current = lck.current;
                let current_abs = current.abs();
                match lck.current_range {
                    0 => { // mA range
                        if current_abs >= 2.0 { // 2A threshold to go up
                            lck.current_range = 1;
                        }
                    },
                    1 => { // A range
                        if current_abs < 1.5 { // 1.5A threshold to go down
                            lck.current_range = 0;
                        }
                    },
                    _ => {
                        lck.current_range = 1;
                    }
                }

                // Auto-range power display with hysteresis
                let power = lck.power;
                let power_abs = power.abs();
                match lck.power_range {
                    0 => { // mW range
                        if power_abs >= 2.0 { // 2W threshold to go up
                            lck.power_range = 1;
                        }
                    },
                    1 => { // W range
                        if power_abs < 1.5 { // 1.5W threshold to go down
                            lck.power_range = 0;
                        }
                    },
                    _ => {
                        lck.power_range = 1;
                    }
                }

                // Battery level with hysteresis to prevent frequent changes
                let battery_voltage = lck.battery;
                match battery_level {
                    0 => {
                        if battery_voltage >= 3.65 {  // Higher threshold to go up
                            battery_level = 20;
                        }
                    },
                    20 => {
                        if battery_voltage >= 3.75 {  // Higher threshold to go up
                            battery_level = 40;
                        }
                        else if battery_voltage < 3.60 {  // Lower threshold to go down
                            battery_level = 0;
                        }
                    },
                    40 => {
                        if battery_voltage >= 3.85 {  // Higher threshold to go up
                            battery_level = 60;
                        }
                        else if battery_voltage < 3.70 {  // Lower threshold to go down
                            battery_level = 20;
                        }
                    },
                    60 => {
                        if battery_voltage >= 3.95 {  // Higher threshold to go up
                            battery_level = 80;
                        }
                        else if battery_voltage < 3.80 {  // Lower threshold to go down
                            battery_level = 40;
                        }
                    },
                    80 => {
                        if battery_voltage >= 4.05 {  // Higher threshold to go up
                            battery_level = 100;
                        }
                        else if battery_voltage < 3.90 {  // Lower threshold to go down
                            battery_level = 60;
                        }
                    },
                    100 => {
                        if battery_voltage >= 4.25 {  // Higher threshold to go up (USB power)
                            battery_level = 200;
                        }
                        else if battery_voltage < 4.00 {  // Lower threshold to go down
                            battery_level = 80;
                        }
                    },
                    200 => {
                        if battery_voltage < 4.15 {  // Lower threshold to go down from USB power
                            battery_level = 100;
                        }
                    },
                    _ => {
                        battery_level = 0;
                    }
                }

                // Check if anything has changed that requires display update
                let wifi_changed = match (&lck.wifi, &prev_wifi_status) {
                    (WifiStatus::Disconnected, WifiStatus::Disconnected) => false,
                    (WifiStatus::Connecting, WifiStatus::Connecting) => loopcount != prev_loopcount_display, // Animation frames
                    (WifiStatus::Connected, WifiStatus::Connected) => lck.wifi_rssi != prev_wifi_rssi,
                    _ => true,
                };

                let status_changed = match (&lck.status, &prev_status) {
                    (LoggingStatus::Start, LoggingStatus::Start) => false,
                    (LoggingStatus::Stop, LoggingStatus::Stop) => false,
                    _ => true,
                };

                let display_needs_update = 
                    lck.voltage != prev_voltage ||
                    lck.current != prev_current ||
                    lck.power != prev_power ||
                    lck.voltage_range != prev_voltage_range ||
                    lck.current_range != prev_current_range ||
                    lck.power_range != prev_power_range ||
                    status_changed ||
                    wifi_changed ||
                    lck.buffer_water_mark != prev_buffer_wm ||
                    lck.battery != prev_battery ||
                    battery_level != prev_battery_level ||
                    lck.channel != prev_channel ||
                    lck.message != prev_message;

                // Only update display if something changed
                if display_needs_update {
                    display.clear();

                    // Display voltage with auto-range
                    match lck.voltage_range {
                        0 => { // mV
                            Text::new(&format!("V:{:.2}mV", voltage * 1_000.0), Point::new(1, 30), style_large).draw(&mut display).unwrap();
                        },
                        1 => { // V
                            Text::new(&format!("V:{:.4}V", voltage), Point::new(1, 30), style_large).draw(&mut display).unwrap();
                        },
                        _ => {}
                    }
                    
                    // Display current with auto-range
                    match lck.current_range {
                        0 => { // mA
                            Text::new(&format!("I:{:.2}mA", current * 1_000.0), Point::new(1, 15), style_large).draw(&mut display).unwrap();
                        },
                        1 => { // A
                            Text::new(&format!("I:{:.4}A", current), Point::new(1, 15), style_large).draw(&mut display).unwrap();
                        },
                        _ => {}
                    }
                    
                    // Display power with auto-range
                    match lck.power_range {
                        0 => { // mW
                            Text::new(&format!("P:{:.2}mW", power * 1_000.0), Point::new(1, 40), style_middle).draw(&mut display).unwrap();
                        },
                        1 => { // W
                            Text::new(&format!("P:{:.4}W", power), Point::new(1, 40), style_middle).draw(&mut display).unwrap();
                        },
                        _ => {}
                    }
                                    
                    // Display logging status
                    match lck.status {
                        LoggingStatus::Start => {
                            Text::new("LOGGING", Point::new(1, 50), style_middle_inv).draw(&mut display).unwrap();
                        },
                        LoggingStatus::Stop => {
                            Text::new("STOPPED", Point::new(1, 50), style_middle).draw(&mut display).unwrap();
                        }
                    }
                    
                    // Display buffer watermark as bar
                    let bar_x = 1;
                    let bar_y = 55;
                    let bar_width = 60;
                    let bar_height = 5;
                    
                    // Draw outer frame
                    Rectangle::new(Point::new(bar_x, bar_y), Size::new(bar_width, bar_height))
                        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
                        .draw(&mut display).unwrap();
                    
                    // Calculate filled width based on watermark percentage
                    let filled_width = (bar_width as u32 - 2) * lck.buffer_water_mark / 100;
                    if filled_width > 0 {
                        Rectangle::new(Point::new(bar_x + 1, bar_y + 1), Size::new(filled_width, bar_height - 2))
                            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                            .draw(&mut display).unwrap();
                    }
                    
                    // Display percentage text next to the bar
                    Text::new(&format!("{}%", lck.buffer_water_mark), Point::new(65, 60), style_small).draw(&mut display).unwrap();
                                                    
                    // Battery status
                    Text::new(&format!("{:.1}V", battery_voltage), Point::new(86, 60), style_small).draw(&mut display).unwrap();
                    
                    match battery_level {
                        0 => {
                            bat0_img.draw(&mut display).unwrap();
                        },
                        20 => {
                            bat20_img.draw(&mut display).unwrap();
                        },
                        40 => {
                            bat40_img.draw(&mut display).unwrap();
                        },
                        60 => {
                            bat60_img.draw(&mut display).unwrap();
                        },
                        80 => {
                            bat80_img.draw(&mut display).unwrap();
                        },
                        100 => {
                            bat100_img.draw(&mut display).unwrap();
                        },
                        200 => {
                            usbpwr_img.draw(&mut display).unwrap();
                        },
                        _ => {}
                    }
                    
                    // Wifi status
                    match lck.wifi {
                        WifiStatus::Disconnected => {
                        },
                        WifiStatus::Connecting => {
                            match loopcount {
                                0..=2 => {
                                    wifi_img0.draw(&mut display).unwrap();
                                },
                                3..=5 => {
                                    wifi_img1.draw(&mut display).unwrap();
                                },
                                6..=8 => {
                                    wifi_img2.draw(&mut display).unwrap();
                                },
                                9..=11 => {
                                    wifi_img3.draw(&mut display).unwrap();
                                },
                                12..=15 => {
                                    wifi_img4.draw(&mut display).unwrap();
                                },
                                _ => {},
                            }
                        },
                        WifiStatus::Connected => {
                            match lck.wifi_rssi {
                                -100..=-80 => {
                                    wifi_img0.draw(&mut display).unwrap();
                                },
                                -79..=-75 => {
                                    wifi_img1.draw(&mut display).unwrap();
                                },
                                -74..=-70 => {
                                    wifi_img2.draw(&mut display).unwrap();
                                },
                                -69..=-65 => {
                                    wifi_img3.draw(&mut display).unwrap();
                                },
                                -64..=-30 => {
                                    wifi_img4.draw(&mut display).unwrap();
                                },
                                _ => {
                                },
                            }
                            if lck.wifi_rssi != 0 {
                                Text::new(&format!("{:+02}dBm", lck.wifi_rssi), Point::new(81, 52), style_small).draw(&mut display).unwrap();
                            }
                            else {
                                Text::new("NO SIG", Point::new(81, 52), style_small).draw(&mut display).unwrap();
                            }
                        },
                    }    
                    
                    // Display Channel
                    Text::new(&format!("CH:{}", lck.channel), Point::new(50, 50), style_middle).draw(&mut display).unwrap();

                    // Error message if any
                    if !lck.message.is_empty() {
                        display.clear();
                        Text::new("ERROR:", Point::new(1, 1), style_small).draw(&mut display).unwrap();
                        Text::new(&lck.message, Point::new(1, 8), style_small).draw(&mut display).unwrap();
                    }

                    match display.flush() {                  
                        Ok(_) => {},
                        Err(_) => {},
                    }

                    // Update previous values for next comparison
                    prev_voltage = lck.voltage;
                    prev_current = lck.current;
                    prev_power = lck.power;
                    prev_voltage_range = lck.voltage_range;
                    prev_current_range = lck.current_range;
                    prev_power_range = lck.power_range;
                    prev_status = match lck.status {
                        LoggingStatus::Start => LoggingStatus::Start,
                        LoggingStatus::Stop => LoggingStatus::Stop,
                    };
                    prev_wifi_status = match lck.wifi {
                        WifiStatus::Disconnected => WifiStatus::Disconnected,
                        WifiStatus::Connecting => WifiStatus::Connecting,
                        WifiStatus::Connected => WifiStatus::Connected,
                    };
                    prev_wifi_rssi = lck.wifi_rssi;
                    prev_buffer_wm = lck.buffer_water_mark;
                    prev_battery = lck.battery;
                    prev_battery_level = battery_level;
                    prev_channel = lck.channel;
                    prev_message = lck.message.clone();
                    prev_loopcount_display = loopcount;
                }
                drop(lck);                
                thread::sleep(Duration::from_millis(100));
            }
        });
    }

    pub fn set_voltage(&mut self, vol: f32, cur: f32, power: f32)
    {
        let mut lck = self.txt.lock().unwrap();
        lck.voltage = vol;
        lck.current = cur;
        lck.power = power;
    }

    pub fn set_current_status(&mut self, status: LoggingStatus)
    {
        let mut lck = self.txt.lock().unwrap();
        lck.status = status;
    }

    pub fn set_wifi_status(&mut self, status: WifiStatus)
    {
        let mut lck = self.txt.lock().unwrap();
        lck.wifi = status;
    }

    pub fn set_err_message(&mut self, msg: String)
    {
        let mut lck = self.txt.lock().unwrap();
        lck.message = msg;
    }

    pub fn set_battery(&mut self, bat: f32)
    {
        let mut lck = self.txt.lock().unwrap();
        lck.battery = bat;
    }

    pub fn set_buffer_watermark(&mut self, wm: u32)
    {
        let mut lck = self.txt.lock().unwrap();
        lck.buffer_water_mark = wm;
    }

    pub fn set_wifi_rssi(&mut self, rssi: i32)
    {
        let mut lck = self.txt.lock().unwrap();
        lck.wifi_rssi = rssi;
    }

    pub fn set_channel(&mut self, channel: u32)
    {
        let mut lck = self.txt.lock().unwrap();
        lck.channel = channel;
    }
}
