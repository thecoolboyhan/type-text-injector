# type-text-injector

把本机文本**模拟键盘输入**到当前光标处 —— 适合远程桌面、虚拟机、受限 App、没有粘贴接口的界面。

> 典型场景：你 Mac 上有一份文本，要打进 **Parallels / VMware 里的 Windows 虚拟机**（剪贴板共享被禁、没法直接粘贴）。启动程序 → 粘贴文本 → 5 秒后它替你把字一个个「敲」进虚拟机里。

> ⚠️ **中文 / 非 ASCII 字符会被跳过**（本工具定位英文/数字/标点）。三个平台都只做键盘模拟，不做粘贴。

📖 **图文介绍**：[type-text-injector：把文本「敲」进虚拟机与远程桌面的小工具](https://thecoolboyhan.github.io/p/type-text-injector/) —— 讲清 HID 扫描码为什么是虚拟机打字的命门、三端各走哪条事件路径，以及 v0.3.0 做了哪些统一。

---

## 🖥️ 图形界面（GUI）

`type-text --gui` 打开 Material 风格图形界面，同进程调用注入逻辑（不 spawn 子进程）：

- 文本框输入 / 粘贴 / 📄 从文件读取 / 🗑 清空
- 📊 输入时实时统计：可注入键数、预计耗时、批次数、跳过字符数（与 CLI 同一口径，不用点预览）
- 🔍 预览：字符数、行数、注入键数、预计耗时、注入批次、跳过字符（纯函数解析，不注入）
- 倒计时 0–99 秒（0 = 立即注入）、每键间隔 1–5000ms 拖拽
- 注入中进度条：已用时间 / 预计耗时、键数、批数
- Pixel 风格动画：环形倒计时（末 3 秒橙→红渐变）、注入中旋转 spinner、按钮悬停反馈
- 注入结果、跳过字符、错误提示在底部状态区（自动换行）
- 三端界面结构 / 配色 / 字号一致，仅排错提示按平台变化；标题栏显示版本与平台

![GUI 预览](docs/gui-preview.png)

### 快速开始

```bash
cargo build --release
./target/release/type-text --gui
```

> macOS 首次真实注入需在「系统设置 → 隐私与安全性 → 辅助功能」授权。

---

## 支持平台

| 平台 | 注入后端 | 前置依赖 | GUI |
|---|---|---|---|
| macOS | `osascript` keycode（最稳，能进虚拟机） | 系统自带；需授予「辅助功能」权限 | ✅ |
| Linux | `xdotool`（X11 XTEST 按键事件，逐键） | 需安装 `xdotool`（如 `apt install xdotool`） | ✅（X11；Wayland 需 ydotool） |
| Windows | `enigo`（Win32 SendInput：VK + 扫描码，逐键） | 自动包含，无需额外依赖 | ✅（glow/OpenGL 后端） |

Release 页提供三个平台编译好的可执行文件，开箱即用（Windows 版需本机有 VC++ 运行库，一般已自带）。

---

## 为什么需要它

往虚拟机里打字，最朴素的写法是 `keystroke "整段文本"`（一次性把整串塞进键盘事件）。但你会发现：

- **回车键生效了**（行数对），但**字母数字几乎全丢**，只剩零星几个大写字母。

原因：虚拟机/远程桌面只认 **HID 扫描码**。`keystroke "字符串"` 走的是 CGEvent 的 **Unicode 字符串**载荷，客户机拿不到扫描码 → 整串被丢弃；而 `key code N`（真实虚拟键码）能映射成 HID 报告 → 正常进字。

三个平台现在**都是逐键模拟键盘输入**，同一个字符在三端按下的物理键序列一致：

- macOS：`key code N` 虚拟键码 → HID 扫描码（能进 Parallels / VMware 的关键）。
- Linux：`xdotool type` / `key` 走 X11 **XTEST** 按键事件（keysym→keycode，自动带 Shift）。
- Windows：enigo `SendInput` 发 Win32 虚拟键码 + 扫描码；**不再用 `enigo.text()`** —— 后者是 `KEYEVENTF_UNICODE` 字符注入（wVk=0、wScan=UTF-16 码元），虚拟机 / 远程桌面 / 受限窗口收不到。

三端共用同一套 US ANSI 键位判定（`base_key()` / `needs_shift()`）和同一套节奏（每键间隔、换行与批边界的额外停顿）。虚拟机场景只在 macOS 做过实测，Linux / Windows 请先用 `-p` 自检。

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
请输入要输入的文本，输入完后按回车，再按 Ctrl-D 结束：  （Windows：Ctrl-Z 或 Ctrl-D）
（粘贴你的文本）
^D
已收到 25 行 / 1240 字符，每键间隔 50ms，预计耗时约 66 秒；5 秒后开始输入。
5 秒后开始输入，请把光标点到目标位置…   4   3   2   1
开始输入…
完成，共输入 1240 个键。
```

> 终端里结束输入：先按一次回车，再按 **Ctrl-D**（Windows 用 **Ctrl-Z** 或 Ctrl-D；若没反应，再按一次）。

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

## 长文本

macOS 版会**自动分批**：单条 AppleScript 的键码列表约 4039 键就会触发「堆栈溢出 (-2706)」，所以程序按 **4000 键一批**切成多条脚本依次执行。一次输入几万字也没问题，不用手动拆。

- 仍然是 `key code` 级注入（虚拟机场景不会因为分批而退化）。
- 每批之间留一个换行级的缓冲停顿（`max(间隔×3, 60)`ms），避免批次交界处丢字。
- 耗时与字数成正比：默认 50ms/键，1 万字约 9 分钟；赶时间可把间隔调小，但虚拟机里丢字风险会上升。
- 中途某批失败会明确报「第 X/Y 批」；已注入的批次不会回滚。

---

## 三端口径统一

三个平台的 `inject()` 共用同一个纯函数 `plan()`：决定哪些字符算一个键、跳过哪些字符、结尾提示保留多少样本，以及预计耗时。所以「完成，共输入 N 个键」和 GUI 预览在三端完全一致，差异只在注入后端本身（见上方平台表）。

- 可注入范围：可打印 ASCII + 空格 / Tab / 换行；非 ASCII 一律跳过。
- 跳过字符的提示样本统一只留前 20 个（`SKIPPED_SAMPLE_LIMIT`），长中文文本不会刷屏。
- 三端都是**逐键真实键事件**（键码/扫描码），不再有 Unicode 字符串注入路径；虚拟机场景仍只对 macOS 做过实测，Linux / Windows 请先用 `-p` 自检。

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

## 相关链接

- 📖 图文介绍（博客）：<https://thecoolboyhan.github.io/p/type-text-injector/>
- 源码仓库：<https://github.com/thecoolboyhan/type-text-injector>
- 下载 Releases：<https://github.com/thecoolboyhan/type-text-injector/releases>

---

## License

MIT © thecoolboyhan —— 见 [LICENSE](./LICENSE)。
