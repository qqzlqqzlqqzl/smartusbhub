# SmartUSBHub Rust CLI

Rust command line controller for SmartUSBHub.

The program discovers the control serial port by USB ID:

- VID: `0x1A86`
- PID: `0xFE0C`

Do not hard-code `COMx`; Windows can change the COM number after moving USB ports.

## Build

Install Rust first if `cargo` is not available:

```powershell
winget install Rustlang.Rustup
```

On this Windows machine there is no MSVC linker, so the GNU host toolchain is the verified local path:

```powershell
cd C:\Users\lyl\Desktop\AISTM32\tools\smartusbhub-rust
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup run stable-x86_64-pc-windows-gnu cargo test --locked
rustup run stable-x86_64-pc-windows-gnu cargo build --release --locked
```

The executable will be:

```text
target\release\smartusbhub-cli.exe
```

## Commands

List serial ports and mark the Hub control port:

```powershell
cargo run -- ports
```

Read four-channel status:

```powershell
target\release\smartusbhub-cli.exe status
```

Open all channels:

```powershell
cargo run -- open-all
```

Control power:

```powershell
cargo run -- power 1 on
cargo run -- power 1 off
cargo run -- power all on
```

Control data lines while keeping power unchanged:

```powershell
cargo run -- data 1 off
cargo run -- data 1 on
```

Simulate USB re-enumeration:

```powershell
cargo run -- reconnect 1 data 1000
```

Power-cycle one channel:

```powershell
cargo run -- reconnect 1 power 2000
```

## Safety Notes

- `data off/on` is preferred when only USB re-enumeration is needed.
- `power off/on` should only be used when a hard device restart is intended.
- Other `VID_1A86` devices, such as CH340 serial adapters with `PID_7523`, are not SmartUSBHub control ports.
