# AGENTS.md — type-text-injector

> 给 AI 编码助手（Claude Code / Codex / GitHub Copilot / WorkBuddy / Cline 等）的接手文档。
> 目标：让你在**不重新踩坑**的前提下，理解、构建、测试、发布并继续优化这个项目。

---

## 1. 这个项目是做什么的

把**本机文本模拟成键盘敲击**，输入到「当前前台窗口的光标处」。典型且唯一重要的场景：

> 你在一台 Mac / Linux / Windows 上有一段文本，要打进 **虚拟机 / 远程桌面 / 受限 App**（剪贴板共享被禁、没有粘贴接口、没有 API）。启动程序 → 粘贴文本 → 5 秒倒计时后，它把字一个个「敲」进目标窗口。

**核心约束（不要擅自扩大范围）**：
- 只支持 **英文 / 数字 / 标点**（US ANSI 键位）。非 ASCII（含中文）会被自动跳过并在结尾提示。
- 三个平台都只做**键盘模拟**，不经过剪贴板、不动鼠标。
- 单条命令、零交互主流程：`启动 → 读文本(EOF/Ctrl-D) → 倒计时 → 逐键注入 → 完成`。

---

## 2. 为什么这个项目存在（以及最关键的坑）

### 🥇 头号坑：往虚拟机打字必须用 keycode 级注入，不能用 Unicode 字符串

**症状**：`keystroke "整段文本"` 时，虚拟机里**回车键生效（行数对），但字母数字几乎全丢，只剩零星几个大写字母**。

**根因**：虚拟机 / 远程桌面只认 **HID 扫描码**。
| 方式 | 事件载荷 | 虚拟机侧 |
|---|---|---|
| `keystroke "字符串"` / enigo `text()` | Unicode 字符串 | 拿不到扫描码 → **整串丢弃** |
| `key code N` / enigo `key(Key, Click)` | 真实虚拟键码 | 能映射成 HID 报告 → **正常投递** |

**因此**：macOS 后端**全部用 `key code N`**（真实虚拟键码 + Shift 标志），这是它能稳定打进 Parallels / VMware 的原因。Linux / Windows 走各自平台的原生键事件（xdotool / Win32 SendInput），同样可靠。

> ⚠️ 任何「改回整段 `keystroke`」的“简化”都会让虚拟机场景重新崩掉。这是本项目存在的全部理由，不要回归。

**次要坑（虚拟机仍可能零星丢字）**：keycode 级注入能进虚拟机，但字符仍可能被目标键盘缓冲 / HID 队列丢弃。唯一有效手段是**拉大每键间隔**（默认 50ms，丢字就 80/120/200ms 逐档加）。代价 = 字符数 × 间隔，开工前必须打印预计耗时。

---

## 3. 架构

单一二进制 `type-text`，Rust（edition 2021），零运行时依赖（Windows 除外，见下）。

```
type-text-injector/
├── src/main.rs            # 全部逻辑（约 320 行）
├── Cargo.toml            # 二进制名 type-text；仅 Windows 依赖 enigo
├── .cargo/config.toml    # macOS→Linux musl 静态交叉编译配置
├── .github/workflows/release.yml   # 推 v* tag 时构建三平台并发布 Release
├── README.md             # 面向人类的使用说明（中文）
├── AGENTS.md             # 本文件
├── LICENSE               # MIT
└── .gitignore
```

### `src/main.rs` 结构
- `read_text()`：读 stdin → 统一 CRLF/CR→LF → trim 末尾空行。用 `std::io::IsTerminal` 判断交互模式（交互才打印输入提示）。
- `countdown(secs)`：逐秒倒计时，提示用户把光标点到目标窗口。
- `mod platform`：**用 `#[cfg(target_os = "...")]` 编译期分三个后端**，每个暴露统一接口：
  - `inject(text: &str, delay_ms: u64) -> Result<(usize, Vec<char>), String>`：返回 `(注入键数, 被跳过的非 ASCII 字符)`。
  - `probe(secs, delay_ms)`：自检，打一行标记确认注入生效。
- `main()`：解析参数 → 决定 dry-run / probe / 正常注入。

### 三平台后端对照
| 平台 | 注入方式 | 前置 |
|---|---|---|
| macOS | `osascript` 生成一条 AppleScript，循环 `key code N`（含 `using shift down`），回车=`key code 36` | 系统自带；需「辅助功能」授权终端 |
| Linux | `xdotool type --delay N <char>` / `xdotool key --delay N Return\|Tab` | 需装 `xdotool`，且为 X11 会话（Wayland 暂不支持） |
| Windows | `enigo 0.6`：`Enigo::new(&Settings::default())` + `Keyboard::text(&str)` / `key(Key::Return, Direction::Click)` | 编译期自动拉取，无需用户额外依赖 |

**Windows 专属细节（enigo 0.6 正确 API）**：
```rust
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
let mut enigo = Enigo::new(&Settings::default()).map_err(...)?;  // 返回 Result
enigo.text(&c.to_string());
enigo.key(Key::Return, Direction::Click);
```
`enigo = "0.6"` 只在 `Cargo.toml` 的 `[target.'cfg(target_os = "windows")'.dependencies]` 下声明，macOS/Linux 不引入该依赖（保持零编译依赖）。

### CLI 契约（保持稳定，位置无关）
| 命令 | 行为 |
|---|---|
| `./type-text` | 默认：准备 5 秒，每键 50ms |
| `./type-text 10` | 第 1 个裸数字 = 准备秒数（倒计时） |
| `./type-text 5 80` | 第 2 个裸数字 = 每键间隔毫秒（虚拟机丢字就调大） |
| `./type-text -n` | 只预览解析结果，**不注入**（调试安全阀，必须保留） |
| `./type-text -p` | 自检：打一行标记确认注入是否生效 |
| `<input> \| ./type-text` | 管道 / 重定向输入 |

> 裸数字位置参数必须始终保留（兼容性）；`-n`/`-p` 为隐藏短开关，不写进正式帮助，但 `main()` 解析要认识它们。

---

## 4. 构建与测试

### 宿主平台直接构建
```bash
# macOS / Linux
cargo build --release                 # 产物 target/release/type-text
rustc -O src/main.rs -o type-text     # 零依赖单文件编译（mac/Linux，无需联网）

# Windows（在 windows 机器上）
cargo build --release                 # enigo 会自动拉取
```

### 从 macOS 交叉编译 Linux 静态二进制（musl）
宿主机没有 GNU ld / musl-gcc，用 Rust 自带的 `rust-lld` 自包含链接（见 `.cargo/config.toml`）：
```toml
[target.x86_64-unknown-linux-musl]
linker = "rust-lld"
rustflags = ["-C", "link-self-contained=yes"]
```
```bash
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
# => ELF 64-bit static-pie，目标机无需任何运行时
```

### 测试（安全）
```bash
printf 'Hello World 123\n\tTab test @#$%%\n' | ./type-text -n
```
- `-n` **只解析预览、不注入键盘**，是唯一的调试安全阀，CI / 本地验证都优先用它。
- macOS 上可用 `osacompile -o /dev/null /tmp/tt_inject.applescript` 校验生成的 AppleScript 语法（注入脚本会落盘到 `/tmp/tt_inject.applescript`）。
- `-p` 会**真正打字**到当前窗口，只在明确要验证目标环境时使用，别在 CI 里跑。

---

## 5. 发布流程（CI 自动）

- 推 `v*` tag 触发 `.github/workflows/release.yml`：矩阵在 `macos-latest`(aarch64) / `ubuntu-latest`(musl) / `windows-latest`(msvc) 三 runner 各构建一份，用 `softprops/action-gh-release@v2` 上传到同一 Release。
- **Windows 二进制只能由 `windows-latest` 编译**（macOS 编不了 Windows），所以 **CI 是出三端二进制的唯一可靠途径**；本机只负责 mac/linux 自测。

```bash
git tag v0.2.0
git push origin v0.2.0      # 触发 Actions，自动产出 type-text-macos / type-text-linux / type-text-windows.exe
```

### ⚠️ CI 工作流致命坑（已踩过）
矩阵**必须用 `os: [...]` + `include`**，绝不能用「list-of-maps」形式：
```yaml
strategy:
  matrix:
    os: [macos-latest, ubuntu-latest, windows-latest]
    include:
      - { os: macos-latest,    target: aarch64-apple-darwin,    asset: type-text-macos }
      - { os: ubuntu-latest,   target: x86_64-unknown-linux-musl, asset: type-text-linux }
      - { os: windows-latest,  target: x86_64-pc-windows-msvc,  asset: type-text-windows.exe }
```
若写成 `matrix: [{os:..., target:...}, ...]`（list-of-maps），GitHub 会在推送时直接判 workflow 文件非法 → run 瞬间 `failure (0s)`，`gh run view --log` 也拿不到日志，极难排查。**务必保持上面的写法。**

---

## 6. 已知限制与优化方向（接手后可做的改进）

1. **不支持中文 / 非 ASCII**：当前直接跳过。若要支持，推荐「客户机侧注入」路线（在共享目录放脚本让 guest 自己 SendInput），宿主侧 keycode 无法投递 Unicode。
2. **键位表是 US ANSI 写死的**：`macOS` 后端 `key_of()` 内置 US ANSI 键码；其他键盘布局（如 Dvorak、非美规）需要新增映射表 + 布局参数。
3. **Linux 仅支持 X11**：Wayland 用户需要 `ydotool` 后端（可作为第四个 `#[cfg]` 变体）。
4. **每键固定间隔**：可改为按目标响应动态退避（丢字检测 → 自动加间隔）。
5. **无单测 / 集成测试**：`inject` 纯副作用（敲键盘），难单测；可考虑把「文本→键序列」的映射逻辑抽成纯函数，单独单测（这是最有价值的重构点）。

---

## 7. 给接手 AI 的硬规则

- **不要回归到整段 `keystroke` / Unicode 字符串注入**（见第 2 节）——会毁掉虚拟机场景。
- **不要给 CLI 加功能清单**：用户要的就是「启动→粘贴→等 5 秒→打字」一条路径。新增诊断开关保持 `-n`/`-p` 这种隐藏短形式。
- **保留 `-n` 干跑**：任何改动后先 `./type-text -n` 验证解析不崩。
- **改了 `release.yml` 想重跑**：删旧 tag 重打即可（首次发布无 Release 产物时安全）；不要为了文档改动而盲目发新版本号。
- **提交信息用中文或英文均可**，但 PR/commit 要说明「改了哪个平台后端 / 哪个 CI 环节」。
