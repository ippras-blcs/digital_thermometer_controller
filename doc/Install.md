# Install

## Windows

* `winget uninstall usbipd`
* `winget install usbipd`

## Wsl

Install the proper toolchain with the rust-src component:

* `rustup toolchain install nightly --component rust-src`

Install cargo sub-commands:

* `cargo install esp-generate --locked`
* `cargo install espflash --locked`
* `cargo install esp-config --features=tui --locked`

Install cargo sub-commands (std):

* `cargo install cargo-generate --locked`
* `cargo install ldproxy --locked`
* `cargo install espup --locked`
* `cargo install espflash --locked`
* `cargo install cargo-espflash --locked` # Optional
