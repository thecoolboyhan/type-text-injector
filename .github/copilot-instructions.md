# Copilot 指令

本项目是 `type-text-injector`：把本机文本模拟键盘输入到当前光标处（含虚拟机/远程桌面），跨 macOS / Linux / Windows。

**完整架构、关键坑、构建/测试/发布流程见根目录 [`AGENTS.md`](../../AGENTS.md)，修改前务必通读，尤其是「VM keycode 注入」与「CI 矩阵 YAML 写法」两节。**

## 速查

- **构建（宿主平台）**：`cargo build --release`，或零依赖 `rustc -O src/main.rs -o type-text`（mac/Linux）。
- **跨编译 Linux 静态二进制（从 macOS）**：`rustup target add x86_64-unknown-linux-musl && cargo build --release --target x86_64-unknown-linux-musl`（依赖 `.cargo/config.toml` 的 rust-lld 自包含链接）。
- **安全测试（不注入键盘）**：`printf 'Hello 123\n' | ./type-text -n`。
- **发布**：打 `v*` tag 推送到 `origin` 即触发 `.github/workflows/release.yml`，自动构建三平台并上传到 Release。

## 硬性约束（不要违反）

1. 三端后端**必须都是逐键真实键事件**：macOS `key code N`、Linux `xdotool type/key`（XTEST）、Windows `enigo.key(Key::Other(vk), …)`（`wVk` + 扫描码）。
   - **禁止**回归到整段 `keystroke "字符串"`，也**禁止** Windows 改回 `enigo.text()`（`KEYEVENTF_UNICODE`）——两者都会让虚拟机 / 远程桌面场景崩掉。
   - Windows 的 Shift 必须自己 `Press/Release`（enigo 会丢掉 `VkKeyScanExW` 的 shift 位）；键位判定统一走 `base_key()` / `needs_shift()`。
   - 节奏统一走 `delay_ms` + `line_delay_ms()`，`BATCH_KEYS = 4000` 为分批与批边界停顿阈值，三端共用。
2. CI 矩阵必须用 `os: [...]` + `include` 写法，不要用 list-of-maps。
3. 单条命令、零交互主流程：`启动 → 读文本 → 倒计时 → 逐键注入`。不要给 CLI 加功能清单。
4. 始终保留 `-n`（干跑预览，不注入）作为调试安全阀。
5. 仅英文/数字/标点；非 ASCII 自动跳过并提示，不要擅自扩展中文支持（见 AGENTS.md 第 6 节）。
