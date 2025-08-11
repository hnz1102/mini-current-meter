<div align="center">
  <h1><code>Mini Current Meter</code></h1>
  <p>
    <img src="doc/front.jpg"/>
  </p>
</div>

# Mini Current Meter - Mini Size High-resolution Digital Power Monitor and Logger

This tool provides a logging function that captures data on voltage, current and power consumption. Voltage input range is from 0 to 35V, maximum current input is 10A. 

# Features

**High-resolution** - By using Texas Instruments INA228 IC - 20-bit delta-sigma ADC, it can obtain measurement data with 195µV bus voltage and 163.84mV shunt voltage resolution.

**Microcontroller on board** - No need for a client PC when measuring. Data is sent directly to the server.

**Transfer measurement data via WiFi** - This logger can transfer data to a Linux PC via WiFi network and you can view the dashboard graph in InfluxDB.

**Calibration Function** - Built-in calibration functionality with persistent storage. Long press the center button for 2+ seconds to perform automatic calibration that corrects voltage and current measurement offsets. Calibration results are automatically saved to non-volatile storage and restored on power-up.

**Change Channel** - This logger allows you to change the measurement channel by pushing the center button with a pin. Once you push the center button, the channel will change to the next channel. The channel cycles through 1, 2, 3, 4, and back to 1.

**Battery Powered** - Uses LiPo battery. It can run for 12 hours on a single charge. The battery is charged via USB Type-C port.

**Mini Size** - The dimensions are 37mm(W) x 67mm(D) x 55mm(H). It can be used in various locations.

# How to Use the Logger

![PinOut](doc/pinout.png)

This meter has a pin socket with three pins: VBUS/VIN+, GND, and VIN-. The VBUS/VIN+ pin is connected to the voltage input or the current input. The GND pin is connected to the ground of the voltage or current input. The VIN- pin is connected to the load side of the voltage or current input.

![How to connect](doc/howtomeasure.png)

DO NOT CONNECT WITH REVERSE POLARITY BETWEEN VBUS/VIN+ AND GND. IF CONNECTED INCORRECTLY, THE ADC IC WILL BE DAMAGED.

The measurement interval time is fixed at 100ms. Each measurement data is sent to the server every 1 second.

The display shows the current voltage, current, power consumption, battery voltage, buffer consumption, WiFi connection status, and channel number.
If the WiFi Access Point cannot establish a connection, the display will not show the WiFi indicator. If voltage is measured while WiFi is not connected, the data is stored in the logger's internal memory buffer. The buffer that is not being sent to the server is indicated by a buffer bar on the display. When the buffer is full (the bar reaches the right edge of the display), measurement stops automatically. When WiFi is connected and data is transmitted to the server, the buffer bar shrinks to the left. When the buffer is full and measurement is stopped, measurement will resume automatically after the buffer drops below 50%.

![board](doc/board.jpg)

![Display](doc/display.jpg)

If you send data to the server, you can see real-time data using the Dashboard in [InfluxDB](https://www.influxdata.com/influxdb/). If you have 3 meters, you can view data on the dashboard with different channels simultaneously.

These meters are small and light, so you can use them in various locations. For example, you can use them to measure circuit current on your breadboard, or you can measure the current at each measurement point on your breadboard circuit. 

![breadboard](doc/meter_with_breadboard.jpg)

![dashboard](doc/dashboard_4ch.png)

To charge the battery, simply connect to a USB Type-C port from a bus-powered USB port. During charging, the CHG LED is RED. After charging is complete, the FUL LED is GREEN and charging will stop automatically. However,

DO NOT CONTINUE CHARGING IF THE BATTERY IS FULL FOR A LONG TIME.

# Calibration Function

The Mini Current Meter includes built-in calibration functionality to correct measurement offsets and improve accuracy.

## How to Perform Calibration

1. **Prepare for calibration**: Ensure no current is flowing through the shunt resistor and no voltage is applied to the measurement inputs (VBUS/VIN+ and GND should be at the same potential).

2. **Start calibration**: Press and hold the center button for **2-5 seconds**. The display will show "Calibrating..." message.

3. **Calibration process**: The device automatically takes 300 samples over 3 seconds to calculate average voltage and current offsets.

4. **Completion**: When calibration is complete, the display will show "Calibration OK" for 2 seconds, and the offset values will be automatically saved to non-volatile storage.

## Button Functions

- **Short press** (< 2 seconds): Change measurement channel (1-4)
- **Long press** (2+ seconds): Perform calibration

## Calibration Features

- **Automatic offset correction**: Calibration corrects both voltage and current measurement offsets
- **Persistent storage**: Calibration results are automatically saved to NVS (Non-Volatile Storage) and restored on power-up
- **High accuracy**: Uses 300-sample averaging for precise offset calculation
- **User feedback**: Real-time status display during calibration process

## When to Calibrate

- **Initial setup**: Perform calibration when first using the device
- **Temperature changes**: Recalibrate if operating temperature changes significantly
- **Accuracy concerns**: If measurements appear to have a systematic offset
- **After firmware updates**: Recalibration recommended after major firmware changes

**Note**: Always ensure zero current flow and zero voltage difference during calibration for best results.

# Recent Updates

## Version 2025.08 - Calibration and Accuracy Improvements

- **Added calibration functionality**: Built-in calibration system with persistent storage
- **Improved measurement accuracy**: Automatic offset correction for voltage and current readings
- **Enhanced button handling**: Added debouncing and multi-function button support (channel change, calibration, reset)
- **NVS integration**: Calibration data automatically saved and restored across power cycles
- **User interface improvements**: Real-time calibration status display and user feedback
- **Error correction**: Fixed voltage measurement accuracy issues and ADC offset handling

# How to Build from Code and Install to the Unit

Using Ubuntu 22.04 LTS

1. Install Rust Compiler
```bash
sudo apt update && sudo apt -y install git python3 python3-pip gcc build-essential curl pkg-config libudev-dev libtinfo5 clang libclang-dev llvm-dev udev libssl-dev python3.10-venv
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

select No.1

After installation, you need to set the environment variable for Rust.
Add the following line to your shell configuration file (e.g., `~/.bashrc`, `~/.zshrc`, etc.):
```bash
source "$HOME/.cargo/env"
```

2. Install toolchain for ESP32-C3
```bash
cargo install ldproxy
cargo install espup
cargo install cargo-espflash
```

At this time (2025-07-25), espup cannot be compiled. If you get an error, please use the following command to install the toolchain.
```bash
cargo install cargo-binstall
cargo binstall espup
```

```bash
espup install
espup update
```

Then, run the following command to set the environment for the ESP32-C3 toolchain:
```bash
. ./export-esp.sh
```

3. Add UDEV rules
```bash
sudo sh -c 'echo "SUBSYSTEMS==\"usb\", ATTRS{idVendor}==\"303a\", ATTRS{idProduct}==\"1001\", MODE=\"0666\"" > /etc/udev/rules.d/99-esp32.rules'
sudo udevadm control --reload-rules
sudo udevadm trigger
```

4. Download Mini Current Meter code
```bash
git clone https://github.com/hnz1102/mini-current-meter.git
cd mini-current-meter/code
``` 
5. Setting WiFi SSID, Password, and InfluxDB Server IP Address

You need to set the WiFi SSID, password, and InfluxDB server IP address in the configuration file.
You can find the configuration file 'cfg.toml.samp' in the `code` directory. You need to copy this file to `cfg.toml` and edit it.

```bash
nano code/cfg.toml

[mini-current-meter]
wifi_ssid = "XXXXXXXXXXXX"  # Set your WiFi SSID.
wifi_psk = "XXXXXXXXXXXXX"  # Set your WiFi Password.
shunt_resistance = "0.005"
influxdb_server = "<IP Address>:8086"  # Set your InfluxDB server IP address.
influxdb_api_key = "<API_KEY>" # Set your InfluxDB API Key.
influxdb_api = "/api/v2/write?org=<ORG>&bucket=LOGGER&precision=ns" # Set your InfluxDB API URL. You must set <ORG> same as Initial Organization Name.
influxdb_tag = "ch"
influxdb_measurement = "minicurrent"
max_records = "1023"
```

6. Connecting the Board and Setting Device and Toolchain
```bash
Connect the mini-current-meter via USB to this build code PC. Then, 
$ cargo espflash board-info
select /dev/ttyACM0
Chip type:         esp32c3 (revision v0.4)
Crystal frequency: 40MHz
Flash size:        4MB
Features:          WiFi, BLE
MAC address:       xx:xx:xx:xx:xx:xx

$ rustup component add rust-src --toolchain nightly-2023-06-10-x86_64-unknown-linux-gnu
```

7. Build Code and Write to Flash
```bash
$ cargo espflash flash --release --monitor
App/part. size:    964,240/3,145,728 bytes, 30.23%
[00:00:00] [========================================]      12/12      0x0                                                                       
[00:00:00] [========================================]       1/1       0x8000                                                                    
[00:00:11] [========================================]     546/546     0x10000                                                                   [2023-11-11T10:17:05Z INFO ] Flashing has completed!

Automatically boots!
```
# How to Install InfluxDB

1. Download [InfluxDB](https://docs.influxdata.com/influxdb/v2.7/install/?t=Linux) and Install
```bash
$ wget https://dl.influxdata.com/influxdb/releases/influxdb2-2.7.0-amd64.deb
$ sudo dpkg -i influxdb2-2.7.0-amd64.deb
$ sudo service influxdb start
```

2. Configure InfluxDB

```
Connect to 'http://<InfluxDB installed PC Address>:8086'
```
Click `GET STARTED` and set `Username`, `Password`, `Initial Organization Name`, and `Initial Bucket Name`
|Term|Value|
|---|---|
|Username|Set login username as InfluxDB administrator web console|
|Password|Set login password as InfluxDB administrator web console|
|Initial Organization Name| Organization Name ex. ORG|
|Initial Bucket Name| LOGGER |

After setting these, click `CONTINUE`.

3. Copy the Operator API Token

You can see the operator API token in the browser. YOU WON'T BE ABLE TO SEE IT AGAIN!
If you want to get a new API token, click `API Tokens` menu from `Sources` Icon, then click `GENERATE API TOKEN` and select `All access token`, click `Save`.
You can see the new API token and copy it.
After copying the token, click `CONFIGURE LATER`.

4. Import the Dashboard Template

Click the `Dashboard` icon, and select `Import Dashboard` from the `CREATE DASHBOARD` menu.

Drop the `mini-current-meter/dashboard/mini-current-meter.json` file to `Drop a file here`, then click `IMPORT JSON AS DASHBOARD`.

You can see the `Mini Current Meter Dashboard` panel on the Dashboards page.

Click this panel, and you can see the Mini Current Meter Dashboard.

If you want to customize the dashboard design, click the configure mark. You can change the graph design.

6. Start Mini Current Meter Logging and Send Data

Turn on the power switch. Logging data will be sent to InfluxDB. You can see the data on the dashboard.

## Schematic, PCB Gerber and Container 3D Data

There is schematic data in the hardware directory including 3D printing data as 3MF format files. 

![Container](doc/container.jpg)

## LICENSE
This source code is licensed under MIT. Other Hardware Schematic documents are licensed under CC-BY-SA V4.0.
