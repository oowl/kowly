# kowly：KDE 上的 AirDrop

一个 KDE Plasma 桌面应用，可以和 iPhone 互相发送文件，底层基于 [Owly](https://github.com/t4t5/owly)。可执行文件名为 `owly-drop`。

- **发送**：把文件拖进窗口，或者直接拖到某个设备的图标上；也可以在 Dolphin 里右键选「用 AirDrop 发送」。
- **接收**：iPhone 发来的文件会弹出「接受 / 拒绝」卡片。窗口不在前台时改用桌面通知，通知里也有这两个按钮。
- **控制**：一个开关控制 radio。界面上显示附近的 AWDL 设备数和每笔传输的进度。

## 结构

```
Qt/QML 界面（Kirigami）            qml/Main.qml
  └─ cxx-qt 桥接（Backend QObject） src/bridge.rs
       └─ 引擎（tokio，与 Qt 无关）  src/airdrop/
            ├─ owly-airdrop  发现、发送、接收（直接链接 Owly 的库）
            ├─ owlyd::api    通过系统 D-Bus 向 owlyd 申请 radio 租约
            ├─ BlueZ         BLE 广播，用来唤醒 iPhone
            └─ 通知          org.freedesktop.Notifications
```

monitor 模式和 AWDL 需要 root，这部分由 Owly 的系统服务 `owlyd` 负责，本程序本身以普通用户身份运行。退出程序时会归还 radio，Wi-Fi 也会随之恢复。

## 安装

1. **安装 Owly**（需要 sudo，只装一次）：
   ```sh
   yay -S owly
   ```
   owlyd 通过 D-Bus 按需启动，不需要 `systemctl enable`。polkit 默认允许当前登录的用户免密使用 radio。

2. **安装本程序**（用户级安装，不需要 root）：
   ```sh
   scripts/install.sh
   ```
   安装后会出现：
   - 应用菜单里的「AirDrop」
   - Dolphin 右键菜单里的「用 AirDrop 发送」

   卸载：`scripts/install.sh uninstall`

## 使用前提

先运行 `owly doctor`，看你的网卡在 Owly 里是哪种模式、哪些信道可用。Owly 的[硬件兼容表](https://github.com/t4t5/owly#supported-hardware)列出了各网卡的情况。

**Shared 模式**（多数已测网卡的默认模式）：Wi-Fi 保持连接，AWDL 和它共用同一个信道。

- Wi-Fi 必须连在 6、44 或 149 信道的网络上，推荐 5GHz 的 149。
- 如果平时用有线上网，可以给这个 Wi-Fi 连接设成不抢默认路由：
  ```sh
  nmcli connection modify <连接名> ipv4.never-default yes ipv6.never-default yes
  ```

**Exclusive 模式**：radio 开着时整张网卡给 AWDL 用，不需要连任何 Wi-Fi。在 `/etc/owly/drivers.toml` 里为你的驱动写一个完整条目，把 `mode` 设为 `"exclusive"`；如果这张网卡没测过这个模式，还要在 `/etc/owly/radio.toml` 里设 `allow_untested = true`。具体格式见 Owly 的 README。

**iPhone 端**：要解锁，并把 AirDrop 设为「所有人（10 分钟）」。「仅限联系人」需要 Apple 账户身份，Linux 上拿不到。

**防火墙**：需要在 `awdl0` 上放行来自 `fe80::/10` 的 TCP 8771。

### 常见问题

- **`regdom 00`，44 和 149 显示 `no-ir`**：通常是缺少法规数据库。安装后设置国家代码：
  ```sh
  sudo pacman -S wireless-regdb
  # 在 /etc/conf.d/wireless-regdom 里取消注释你所在国家那一行
  sudo iw reg reload && sudo iw reg set CN
  ```
  如果开机时内核就没读到数据库，它不会自动重试，所以需要 `iw reg reload` 或重启。

- **`Couldn't start AWDL: No such device`**：如果 `uname -r` 和 `/usr/lib/modules/` 里的版本对不上，说明内核升级后还没重启，`tun` 模块加载不了。重启即可。

## 设置

在窗口右上角「设置」里可以改：

- **显示名称**：iPhone 上看到的名字，默认是主机名。
- **保存目录**：默认 `~/Downloads/AirDrop`。
- **自动接受**：收到文件时不再询问。
- **启动时自动开启 AirDrop**
- **语言**：跟随系统 / 中文 / English，切换后立即生效。错误提示和桌面通知也会跟着切换。

## 开发与测试

```sh
cargo build
scripts/veth-test.sh             # 引擎端到端测试：自动接受 / 询问后接受 / 询问后拒绝
scripts/gui-demo.sh /tmp/shots   # GUI 端到端测试：发送和接收各截一张图
```

这两个测试都和 Owly 自己的测试一样，在 `unshare` 创建的用户网络命名空间里，用一对 veth 网卡运行：

- 不需要 root
- 不需要无线网卡
- 不需要 iPhone

协议层（mDNS、`/Discover`、`/Ask`、TLS `/Upload`）走的是真实流程。但 AWDL 无线层和 iPhone 互通只能用真机测。

### 无界面模式

用于调试或写脚本：

```sh
owly-drop --headless receive [--iface IF] [--name N] [--dir D] [--accept | --answer yes|no] [--once]
owly-drop --headless send [--iface IF] [--name N] --to NAME [--seconds S] FILE...
```

不加 `--iface` 时，向 owlyd 申请 radio。环境变量 `OWLY_DROP_IFACE=<网卡>` 可以让 GUI 跳过 owlyd，直接使用指定网卡。

## 已知限制

- 关闭窗口就会退出程序并归还 radio，**目前没有托盘常驻**。想一直能接收文件，就要让窗口开着（最小化也可以）。
- Owly 依赖固定在 commit `0f2ca40`（crates.io 上的版本落后于仓库，`owlyd` 也没有发布）。升级 Owly 时需要同步修改 `Cargo.toml`。
- Breeze 样式的不确定进度条会在日志里打印 `ProgressBar.qml: TypeError`。这是上游样式的问题，不影响使用。

`src/airdrop/bluez.rs`、`src/airdrop/wifi.rs` 和 `src/airdrop/net.rs` 的一部分改编自 owly-cli。

## 许可证

MIT，见 [LICENSE](LICENSE)。改编自 Owly 的部分保留了 Owly 的 MIT 版权声明，见 [LICENSE-owly](LICENSE-owly)。
