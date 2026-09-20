# type-text-injector

把本机文本**模拟键盘输入**到当前光标处 —— 适合远程桌面、虚拟机、受限 App、没有粘贴接口的界面。

> 典型场景：你 Mac 上有一份文本，要打进 **Parallels / VMware 里的 Windows 虚拟机**（剪贴板共享被禁、没法直接粘贴）。启动程序 → 粘贴文本 → 5 秒后它替你把字一个个「敲」进虚拟机里。

> ⚠️ **中文 / 非 ASCII 字符会被跳过**（本工具定位英文/数字/标点）。三个平台都只做键盘模拟，不做粘贴。

---

## 🖥️ 图形界面（GUI）

`type-text --gui` 打开 Material 风格图形界面，同进程调用注入逻辑（不 spawn 子进程）：

- 文本框输入 / 粘贴 / 📄 从文件读取
- 🔍 预览：字符数、行数、注入键数、预计耗时、跳过字符（纯函数解析，不注入）
- 倒计时 0–99 秒（0 = 立即注入）、每键间隔 1–5000ms 拖拽
- Pixel 风格动画：环形倒计时（末 3 秒橙→红渐变）、注入中旋转 spinner、按钮悬停反馈
- 注入结果、跳过字符、错误提示在底部状态区（自动换行）

![GUI 预览](docs/gui-preview.png)

### 快速开始

```bash
cargo build --release
./target/release/type-text --gui
```

> macOS 首次真实注入需在「系统设置 → 隐私与安全性 → 辅助功能」授权。

---

## 支持平台

| 平台 | 注入后端 | 前置依赖 |
|---|---|---|
| macOS | `osascript` keycode（最稳，能进虚拟机） | 系统自带；需授予「辅助功能」权限 |
| Linux | `xdotool`（逐字 type / key） | 需安装 `xdotool`（如 `apt install xdotool`） |
| Windows | `enigo`（Win32 SendInput） | 自动包含，无需额外依赖 |

Release 页提供三个平台编译好的可执行文件，开箱即用（Windows 版需本机有 VC++ 运行库，一般已自带）。

---

## 为什么需要它

往虚拟机里打字，最朴素的写法是 `keystroke "整段文本"`（一次性把整串塞进键盘事件）。但你会发现：

- **回车键生效了**（行数对），但**字母数字几乎全丢**，只剩零星几个大写字母。

原因：虚拟机/远程桌面只认 **HID 扫描码**。`keystroke "字符串"` 走的是 CGEvent 的 **Unicode 字符串**载荷，客户机拿不到扫描码 → 整串被丢弃；而 `key code N`（真实虚拟键码）能映射成 HID 报告 → 正常进字。

macOS 版**全部用 keycode 级注入**，因此能稳定打进 Parallels / VMware 等虚拟机。Linux（xdotool）、Windows（enigo SendInput）走各自平台的原生键事件，同样可靠。

---

## 前置条件（通用）

1. 运行时会**先倒计时 N 秒**，请把光标点到目标窗口（如虚拟机的记事本）再等它注入。
2. **macOS**：`系统设置 → 隐私与安全性 → 辅助功能` 里把运行本程序的终端/App 加进去并勾选（没授权时按键静默失效）。
3. **Linux**：已安装 `xdotool`，且当前是 X11 会话（Wayland 需改用 `ydotool`，本工具暂未支持）。
4. **Windows**：直接可用。

---

## 安装 / 编译

### 方式 A：直接下载 Release 二进制（最简单）

到 [Releases](https://github.com/thecoolboyhan/type-text-injector/releases) 页面，按系统下载：

- macOS：`type-text-macos`（Apple Silicon）
- Linux：`type-text-linux`（x86_64，静态链接 musl）
- Windows：`type-text-windows.exe`

> 这三个文件由 GitHub Actions 在推 `v*` tag 时**自动交叉编译并发布到 Releases**，无需本机编译。打 tag 即可发布新版（见下方「发布新版本」）。

### 方式 B：Cargo（推荐开发者）

```bash
git clone https://github.com/thecoolboyhan/type-text-injector.git
cd type-text-injector
cargo build --release
# 产物：target/release/type-text（平台对应后缀）
```

### 方式 C：零依赖单文件编译（仅 macOS / Linux，无需联网）

源码核心用 Rust 标准库（Windows 版需 enigo，仍走 Cargo）：

```bash
rustc -O src/main.rs -o type-text
```

---

## 用法

### 交互式（最常用）

```bash
./type-text            # 启动后按提示粘贴文本，准备 5 秒、每键 50ms
```

流程：

```
$ ./type-text
请输入要输入的文本，输入完后按回车，再按 Ctrl-D 结束：
（粘贴你的文本）
^D
已收到 25 行 / 1240 字符，每键间隔 50ms，预计耗时约 66 秒；5 秒后开始输入。
5 秒后开始输入，请把光标点到目标位置…   4   3   2   1
开始输入…
完成，共输入 1240 个键。
```

> 终端里结束输入：先按一次回车，再按 **Ctrl-D**（若没反应，再按一次）。

### 管道 / 重定向

```bash
cat data.txt | ./type-text 5 80      # 准备 5 秒、每键 80ms
./type-text 10 < data.txt             # 准备 10 秒
echo "hello world" | ./type-text      # 直接喂一行
```

---

## 参数

位置无关，支持位置参数：

| 写法 | 含义 |
|---|---|
| `./type-text` | 默认：准备 5 秒，每键间隔 50ms |
| `./type-text 10` | 第 1 个数字 = **准备秒数**（注入前的倒计时） |
| `./type-text 5 80` | 第 2 个数字 = **每键间隔毫秒**（虚拟机丢字就调大这个） |
| `./type-text -n` | 只预览解析结果，不真正注入键盘（调试用） |
| `./type-text -p` | 自检：打一行标记，确认注入是否生效 |

### 虚拟机里还漏字怎么办

字符仍可能被零星丢掉（客户机键盘缓冲溢出 / HID 队列丢弃）。唯一有效的手段是**拉大每键间隔**：

```bash
./type-text 5 80     # 还是漏就 5 120、5 200，逐档加
```

> 代价：1240 字符 × 50ms ≈ 62 秒。开工前程序会打印预计耗时，心里有数。

---

## 支持的输入范围

- `a-z` `A-Z` `0-9`
- 符号：`!@#$%^&*()_-+=[]{};:'",.<>/?\|` `` ` `` `~`
- 空格、Tab、换行
- **非 ASCII（含中文）会被自动跳过**，并在结尾提示被跳过的字符。

---

## 常见问题

**Q：按了没反应 / 一个字都没打出来？**
检查：① 倒计时结束时光标是否在目标窗口；② macOS 是否给了「辅助功能」权限；③ Linux 是否装了 `xdotool`。先 `./type-text -n` 确认解析正常。

**Q：能打中文吗？**
本工具当前只支持英文/数字/标点（非 ASCII 会被跳过）。

**Q：会改我的剪贴板吗？**
不会。文本走临时文件 / 进程内，不经过粘贴板。

**Q：会动我的鼠标吗？**
不会。只发键盘事件，永远打进「当前前台窗口的光标处」。

**Q：Linux 上 xdotool 找不到？**
`apt install xdotool`（Debian/Ubuntu）或对应发行版的包管理器。Wayland 用户需自行改用 ydotool 或切到 X11。

---

## 发布新版本

打一个 `v*` tag 推到 GitHub，CI 会自动构建三个平台并把可执行文件上传到对应 Release：

```bash
git tag v0.1.0
git push origin v0.1.0
```

等 Actions 跑完，到 [Releases](https://github.com/thecoolboyhan/type-text-injector/releases) 即可看到 `type-text-macos` / `type-text-linux` / `type-text-windows.exe` 三个产物。

---

## License

MIT © thecoolboyhan —— 见 [LICENSE](./LICENSE)。
