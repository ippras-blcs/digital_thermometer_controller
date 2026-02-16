# Build + Run

## Build

### List

`usbipd list`

```cmd
1-2    10c4:ea60  CP2102N USB to UART Bridge Controller                         Not shared
```

### Not shared -> Shared (once)

`usbipd bind --busid 1-2`

```cmd
1-2    10c4:ea60  CP2102N USB to UART Bridge Controller                         Shared
```

### Shared -> Attached

`usbipd attach --wsl --busid 1-2`

```cmd
1-2    10c4:ea60  CP2102N USB to UART Bridge Controller                         Attached
```

### Info

`espflash board-info --port=/dev/ttyUSB0`

```shell
Chip type:         esp32c3 (revision v0.3)
Crystal frequency: 40 MHz
Flash size:        4MB
Features:          WiFi, BLE
MAC address:       7c:df:a1:a3:5a:f8
```

## Run

### Wsl

1. `export SSID="..."`
2. `export PASSWORD="..."`
3. `export RUST_LOG="warn,digital_thermometer_controller=debug"`
4. `cargo run`

## Links
