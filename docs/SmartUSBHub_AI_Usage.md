# SmartUSBHub AI 使用说明

本文档给其他 AI/自动化脚本使用，目标是安全发现并控制 SmartUSBHub，避免把其他串口设备误当成 Hub。

## 最小交接信息

给新项目或新 AI 的最小资料只有两项：

```text
请使用 SmartUSBHub 控制 USB 设备，先阅读并遵守：
https://github.com/qqzlqqzlqqzl/smartusbhub/blob/main/docs/SmartUSBHub_AI_Usage.md

被测设备插在 SmartUSBHub 的 CHx 通道。需要重新枚举时优先切 dataline；只有明确要硬重启时才切 power。
```

把 `CHx` 换成实际通道号，例如 `CH1`。不要给固定 `COM` 口，因为换 USB 插口后端口号可能变化。

## 当前实测状态

- 验证时间：2026-04-25
- 操作系统：Windows，PowerShell
- SmartUSBHub 控制库：`https://github.com/qqzlqqzlqqzl/smartusbhub`
- 本次实测控制口：`COM9`，端口号可能随 USB 插口变化
- 控制口 USB ID：`VID_1A86 PID_FE0C`
- Hub 上游枚举：`USB\VID_1A86&PID_8091`
- 当前另有其他 `VID_1A86` 串口设备；只有 `PID_FE0C` 是 SmartUSBHub 控制口。

2026-04-25 实测只读结果：

| 通道 | 电源 | 数据线 | 空载电压 | 电流 |
| --- | --- | --- | --- | --- |
| CH1 | ON | ON | 4748 mV | 0 |
| CH2 | ON | ON | 4755 mV | 0 |
| CH3 | ON | ON | 4751 mV | 0 |
| CH4 | ON | ON | 4749 mV | 0 |

结论：控制口可通信，Hub 上游口正常枚举，4 路空载供电测量正常。当前没有在 Hub 下游口接 USB 设备，所以电流为 0 是正常现象。

## 连接识别

SmartUSBHub 要同时接两根线时功能最完整：

- USB 上游口：负责普通 USB Hub 数据传输，在 Windows 中枚举为 `VID_1A86 PID_8091` 的通用 USB 集线器。
- 指令控制口：负责串口命令控制，USB ID 为 `VID_1A86 PID_FE0C`。Windows 端口名可能是 `COMx`，不要写死。

用下面命令确认设备：

```powershell
Get-PnpDevice -PresentOnly |
  Where-Object { $_.InstanceId -match 'VID_1A86&PID_(FE0C|8091)' } |
  Select-Object Status,Class,FriendlyName,InstanceId
```

用 Python 确认控制口：

```powershell
cd smartusbhub
@'
import serial.tools.list_ports
for p in serial.tools.list_ports.comports():
    print(p.device, p.vid, p.pid, p.description, p.hwid)
'@ | python -
```

只把 `VID_1A86 PID_FE0C` 当作 SmartUSBHub 控制口。其他串口即使也是 `VID_1A86`，也不要当成 Hub 控制口。

## 环境准备

本机已经安装过这些依赖：

```powershell
python -m pip install pyserial colorlog
```

如果换机器或依赖缺失，先获取库并安装依赖：

```powershell
git clone https://github.com/qqzlqqzlqqzl/smartusbhub.git
cd smartusbhub
python -m pip install -r requirements.txt
```

仅做命令行控制时，核心依赖是 `pyserial` 和 `colorlog`。

## 安全默认规则

1. 操作前先按 `VID_1A86 PID_FE0C` 发现控制口，不要依赖固定 `COMx`。
2. 不要把其他 `VID_1A86` 串口设备当成 SmartUSBHub；常见 CH340 是 `PID_7523`。
3. 若 Hub 下游接了正在刷写或运行测试的设备，先问清楚再断电。
4. 默认用只读命令检查状态；需要模拟拔插时，优先只断数据线，除非明确需要断电。
5. 操作结束建议把需要使用的通道恢复为 `power=1`、`dataline=1`。

## 常用 Python 操作

进入库目录：

```powershell
cd smartusbhub
```

发现控制口：

```powershell
@'
import serial.tools.list_ports

ports = [
    p.device for p in serial.tools.list_ports.comports()
    if p.vid == 0x1A86 and p.pid == 0xFE0C
]
if not ports:
    raise SystemExit("SmartUSBHub control port not found")
if len(ports) > 1:
    raise SystemExit(f"Multiple SmartUSBHub control ports found: {ports}")
print(ports[0])
'@ | python -
```

只读自检：

```powershell
@'
from smartusbhub import SmartUSBHub

hub = SmartUSBHub.scan_and_connect()  # scans VID_1A86 PID_FE0C
if hub is None:
    raise SystemExit("SmartUSBHub control port not found")

print("control_port =", hub.port)
for ch in range(1, 5):
    power = hub.get_channel_power_status(ch)
    data_status = hub.get_channel_dataline_status(ch)
    data = data_status.get(ch) if isinstance(data_status, dict) else data_status
    voltage = hub.get_channel_voltage(ch)
    current = hub.get_channel_current(ch)
    print(f"CH{ch}: power={power} data={data} voltage_mV={voltage} current={current}")

hub.disconnect()
'@ | python -
```

打开所有通道电源和数据线：

```powershell
@'
from smartusbhub import SmartUSBHub

hub = SmartUSBHub.scan_and_connect()
if hub is None:
    raise SystemExit("SmartUSBHub control port not found")
hub.set_channel_power(1, 2, 3, 4, state=1)
hub.set_channel_dataline(1, 2, 3, 4, state=1)
hub.disconnect()
'@ | python -
```

模拟某一路 USB 热插拔，保持供电，只断开/恢复数据线：

```powershell
@'
import time
from smartusbhub import SmartUSBHub

CH = 1
hub = SmartUSBHub.scan_and_connect()
if hub is None:
    raise SystemExit("SmartUSBHub control port not found")
hub.set_channel_dataline(CH, state=0)
time.sleep(1)
hub.set_channel_dataline(CH, state=1)
hub.disconnect()
'@ | python -
```

重启某一路下游设备，断电再上电：

```powershell
@'
import time
from smartusbhub import SmartUSBHub

CH = 1
hub = SmartUSBHub.scan_and_connect()
if hub is None:
    raise SystemExit("SmartUSBHub control port not found")
hub.set_channel_power(CH, state=0)
time.sleep(2)
hub.set_channel_power(CH, state=1)
hub.set_channel_dataline(CH, state=1)
hub.disconnect()
'@ | python -
```

## 返回值和注意点

- `get_channel_voltage(ch)` 返回毫伏，例如 `4750` 表示 4.750 V。
- `get_channel_current(ch)` 返回库中的原始电流值；示例 GUI 把它除以 1000 显示为 A。空载时通常为 `0`。
- `get_channel_power_status(1,2,3,4)` 的批量返回在本机实测可能不完整；需要可靠状态时逐通道查询。
- `get_channel_dataline_status(ch)` 返回字典，取当前通道键即可。

## 问题判断

- 找不到 `VID_1A86 PID_FE0C`：检查指令控制口那根线是否接到电脑，重新运行串口枚举。
- 找不到 `VID_1A86 PID_8091`：检查 USB 上游口那根线是否接到电脑；只接控制口时可以控制但不会提供下游数据传输。
- 电压约 4.7-5.1 V 且空载电流为 0：正常。
- 某通道电压为 0：该通道电源可能关闭，或 Hub 供电/线缆异常。
- 下游设备不枚举：确认对应通道 `power=1` 且 `dataline=1`，并确认 USB 上游口已连接主机。
