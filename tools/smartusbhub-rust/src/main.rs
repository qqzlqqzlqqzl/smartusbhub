use anyhow::{anyhow, bail, Context, Result};
use serialport::{SerialPort, SerialPortType};
use std::env;
use std::io::{Read, Write};
use std::thread;
use std::time::{Duration, Instant};

const VID_CH34X: u16 = 0x1A86;
const PID_SMARTUSBHUB_CONTROL: u16 = 0xFE0C;

const CMD_GET_CHANNEL_POWER_STATUS: u8 = 0x00;
const CMD_SET_CHANNEL_POWER: u8 = 0x01;
const CMD_GET_CHANNEL_VOLTAGE: u8 = 0x03;
const CMD_GET_CHANNEL_CURRENT: u8 = 0x04;
const CMD_SET_CHANNEL_DATALINE: u8 = 0x05;
const CMD_GET_CHANNEL_DATALINE_STATUS: u8 = 0x08;

const BAUD_RATE: u32 = 115_200;
const IO_TIMEOUT: Duration = Duration::from_millis(100);
const ACK_TIMEOUT: Duration = Duration::from_millis(700);

#[derive(Debug, Clone)]
struct Frame {
    cmd: u8,
    channel_mask: u8,
    values: Vec<u8>,
}

struct SmartUsbHub {
    port_name: String,
    port: Box<dyn SerialPort>,
    rx: Vec<u8>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        print_usage();
        return Ok(());
    }

    match args[0].as_str() {
        "ports" => list_ports(),
        "status" => {
            let mut hub = SmartUsbHub::connect_auto()?;
            hub.print_status()
        }
        "power" => {
            if args.len() != 3 {
                bail!("usage: smartusbhub-cli power <channels> <on|off>");
            }
            let channels = parse_channels(&args[1])?;
            let state = parse_state(&args[2])?;
            let mut hub = SmartUsbHub::connect_auto()?;
            hub.set_power(&channels, state)?;
            println!("power {:?} -> {}", channels, on_off(state));
            Ok(())
        }
        "data" | "dataline" => {
            if args.len() != 3 {
                bail!("usage: smartusbhub-cli data <channels> <on|off>");
            }
            let channels = parse_channels(&args[1])?;
            let state = parse_state(&args[2])?;
            let mut hub = SmartUsbHub::connect_auto()?;
            hub.set_dataline(&channels, state)?;
            println!("dataline {:?} -> {}", channels, on_off(state));
            Ok(())
        }
        "open-all" => {
            let mut hub = SmartUsbHub::connect_auto()?;
            let channels = [1, 2, 3, 4];
            hub.set_power(&channels, true)?;
            hub.set_dataline(&channels, true)?;
            println!("all channels power=on dataline=on");
            Ok(())
        }
        "reconnect" => {
            if args.len() < 2 || args.len() > 4 {
                bail!("usage: smartusbhub-cli reconnect <channel> [data|power] [delay-ms]");
            }
            let channel = parse_channel(&args[1])?;
            let mode = args.get(2).map(String::as_str).unwrap_or("data");
            let delay_ms = args
                .get(3)
                .map(|s| s.parse::<u64>().context("delay-ms must be a number"))
                .transpose()?
                .unwrap_or(1000);
            let mut hub = SmartUsbHub::connect_auto()?;
            match mode {
                "data" | "dataline" => {
                    hub.set_dataline(&[channel], false)?;
                    thread::sleep(Duration::from_millis(delay_ms));
                    hub.set_dataline(&[channel], true)?;
                    println!("CH{channel} dataline reconnected after {delay_ms} ms");
                }
                "power" => {
                    hub.set_power(&[channel], false)?;
                    thread::sleep(Duration::from_millis(delay_ms));
                    hub.set_power(&[channel], true)?;
                    hub.set_dataline(&[channel], true)?;
                    println!("CH{channel} power-cycled after {delay_ms} ms");
                }
                other => bail!("unknown reconnect mode: {other}; expected data or power"),
            }
            Ok(())
        }
        other => bail!("unknown command: {other}"),
    }
}

impl SmartUsbHub {
    fn connect_auto() -> Result<Self> {
        let ports = smart_hub_ports()?;
        match ports.as_slice() {
            [] => bail!(
                "SmartUSBHub control port not found; expected VID_{:04X} PID_{:04X}",
                VID_CH34X,
                PID_SMARTUSBHUB_CONTROL
            ),
            [name] => Self::connect(name),
            many => bail!("multiple SmartUSBHub control ports found: {many:?}"),
        }
    }

    fn connect(port_name: &str) -> Result<Self> {
        let port = serialport::new(port_name, BAUD_RATE)
            .timeout(IO_TIMEOUT)
            .open()
            .with_context(|| format!("open serial port {port_name}"))?;

        Ok(Self {
            port_name: port_name.to_string(),
            port,
            rx: Vec::new(),
        })
    }

    fn print_status(&mut self) -> Result<()> {
        println!("control_port={}", self.port_name);
        for ch in 1..=4 {
            let power = self.get_power(ch)?;
            let data = self.get_dataline(ch)?;
            let voltage_mv = self.get_u16(CMD_GET_CHANNEL_VOLTAGE, ch)?;
            let current_raw = self.get_u16(CMD_GET_CHANNEL_CURRENT, ch)?;
            println!(
                "CH{ch}: power={} dataline={} voltage_mV={} current_raw={}",
                bit(power),
                bit(data),
                voltage_mv,
                current_raw
            );
        }
        Ok(())
    }

    fn set_power(&mut self, channels: &[u8], state: bool) -> Result<()> {
        self.send_and_expect(CMD_SET_CHANNEL_POWER, channels, &[state as u8])?;
        Ok(())
    }

    fn set_dataline(&mut self, channels: &[u8], state: bool) -> Result<()> {
        self.send_and_expect(CMD_SET_CHANNEL_DATALINE, channels, &[state as u8])?;
        Ok(())
    }

    fn get_power(&mut self, channel: u8) -> Result<u8> {
        let frame = self.send_and_expect(CMD_GET_CHANNEL_POWER_STATUS, &[channel], &[0])?;
        frame
            .values
            .first()
            .copied()
            .ok_or_else(|| anyhow!("power status response missing value"))
    }

    fn get_dataline(&mut self, channel: u8) -> Result<u8> {
        let frame = self.send_and_expect(CMD_GET_CHANNEL_DATALINE_STATUS, &[channel], &[0])?;
        frame
            .values
            .first()
            .copied()
            .ok_or_else(|| anyhow!("dataline status response missing value"))
    }

    fn get_u16(&mut self, cmd: u8, channel: u8) -> Result<u16> {
        let frame = self.send_and_expect(cmd, &[channel], &[0])?;
        if frame.values.len() != 2 {
            bail!("command 0x{cmd:02X} response expected 2 data bytes, got {:?}", frame.values);
        }
        Ok(((frame.values[0] as u16) << 8) | frame.values[1] as u16)
    }

    fn send_and_expect(&mut self, cmd: u8, channels: &[u8], data: &[u8]) -> Result<Frame> {
        let packet = build_packet(cmd, channels, data)?;
        self.rx.clear();
        self.port.write_all(&packet)?;
        self.port.flush()?;

        let deadline = Instant::now() + ACK_TIMEOUT;
        while Instant::now() < deadline {
            let mut buf = [0u8; 64];
            match self.port.read(&mut buf) {
                Ok(n) if n > 0 => {
                    self.rx.extend_from_slice(&buf[..n]);
                    while let Some(frame) = parse_next_frame(&mut self.rx) {
                        if frame.cmd == cmd {
                            return Ok(frame);
                        }
                    }
                }
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {}
                Err(err) => return Err(err).context("read serial response"),
            }
        }

        bail!("timeout waiting for ACK to command 0x{cmd:02X}")
    }
}

fn smart_hub_ports() -> Result<Vec<String>> {
    let mut matches = Vec::new();
    for port in serialport::available_ports()? {
        if let SerialPortType::UsbPort(info) = port.port_type {
            if info.vid == VID_CH34X && info.pid == PID_SMARTUSBHUB_CONTROL {
                matches.push(port.port_name);
            }
        }
    }
    Ok(matches)
}

fn list_ports() -> Result<()> {
    for port in serialport::available_ports()? {
        match port.port_type {
            SerialPortType::UsbPort(info) => {
                let marker = if info.vid == VID_CH34X && info.pid == PID_SMARTUSBHUB_CONTROL {
                    " SmartUSBHub-control"
                } else {
                    ""
                };
                println!(
                    "{} VID_{:04X} PID_{:04X}{}",
                    port.port_name, info.vid, info.pid, marker
                );
            }
            _ => println!("{}", port.port_name),
        }
    }
    Ok(())
}

fn build_packet(cmd: u8, channels: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let channel_mask = channel_mask(channels)?;
    let payload_data: Vec<u8> = if data.is_empty() {
        vec![0]
    } else {
        data.to_vec()
    };

    let mut packet = vec![0x55, 0x5A, cmd, channel_mask];
    packet.extend_from_slice(&payload_data);
    let checksum = packet[2..].iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    packet.push(checksum);
    Ok(packet)
}

fn parse_next_frame(buffer: &mut Vec<u8>) -> Option<Frame> {
    while buffer.len() >= 6 {
        if buffer[0] != 0x55 || buffer[1] != 0x5A {
            buffer.remove(0);
            continue;
        }

        let cmd = buffer[2];
        let len = if protocol_v2(cmd) { 7 } else { 6 };
        if buffer.len() < len {
            return None;
        }

        let channel_mask = buffer[3];
        let values = if len == 7 {
            vec![buffer[4], buffer[5]]
        } else {
            vec![buffer[4]]
        };
        let checksum = buffer[len - 1];
        let calculated = buffer[2..len - 1]
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte));

        if checksum != calculated {
            buffer.remove(0);
            continue;
        }

        buffer.drain(0..len);
        return Some(Frame {
            cmd,
            channel_mask,
            values,
        });
    }
    None
}

fn protocol_v2(cmd: u8) -> bool {
    matches!(cmd, CMD_GET_CHANNEL_VOLTAGE | CMD_GET_CHANNEL_CURRENT)
}

fn parse_channels(input: &str) -> Result<Vec<u8>> {
    if input.eq_ignore_ascii_case("all") {
        return Ok(vec![1, 2, 3, 4]);
    }

    let mut channels = Vec::new();
    for part in input.split(',') {
        let channel = parse_channel(part.trim())?;
        if !channels.contains(&channel) {
            channels.push(channel);
        }
    }
    if channels.is_empty() {
        bail!("no channels specified");
    }
    Ok(channels)
}

fn parse_channel(input: &str) -> Result<u8> {
    let normalized = input.trim().trim_start_matches("CH").trim_start_matches("ch");
    let channel: u8 = normalized
        .parse()
        .with_context(|| format!("invalid channel: {input}"))?;
    if !(1..=4).contains(&channel) {
        bail!("channel must be 1..4, got {channel}");
    }
    Ok(channel)
}

fn channel_mask(channels: &[u8]) -> Result<u8> {
    let mut mask = 0u8;
    for &ch in channels {
        if !(1..=4).contains(&ch) {
            bail!("channel must be 1..4, got {ch}");
        }
        mask |= 1 << (ch - 1);
    }
    Ok(mask)
}

fn parse_state(input: &str) -> Result<bool> {
    match input.to_ascii_lowercase().as_str() {
        "1" | "on" | "true" | "enable" | "enabled" => Ok(true),
        "0" | "off" | "false" | "disable" | "disabled" => Ok(false),
        _ => bail!("state must be on/off"),
    }
}

fn on_off(state: bool) -> &'static str {
    if state {
        "on"
    } else {
        "off"
    }
}

fn bit(value: u8) -> &'static str {
    if value == 0 {
        "off"
    } else {
        "on"
    }
}

fn print_usage() {
    println!(
        r#"SmartUSBHub Rust CLI

Usage:
  smartusbhub-cli ports
  smartusbhub-cli status
  smartusbhub-cli open-all
  smartusbhub-cli power <channels|all> <on|off>
  smartusbhub-cli data <channels|all> <on|off>
  smartusbhub-cli reconnect <channel> [data|power] [delay-ms]

Examples:
  smartusbhub-cli status
  smartusbhub-cli power 1 on
  smartusbhub-cli data 1 off
  smartusbhub-cli reconnect 1 data 1000
  smartusbhub-cli reconnect CH1 power 2000

The control port is discovered by USB VID_1A86 PID_FE0C. Do not hard-code COMx.
"#
    );
}
