//! type-text 核心库：CLI (main.rs) 与 GUI (gui.rs) 共用。
//!
//! 职责：读文本 → 倒计时 → 平台注入（macOS key code / Linux xdotool / Windows enigo）。
//! 关键设计：注入必须走 **keycode 级** 事件（虚拟机只认 HID 扫描码），
//! 任何「整段 keystroke / Unicode 字符串」的简化都会让虚拟机场景崩掉（见 AGENTS.md 第 2 节）。

use std::io::{self, IsTerminal, Read};
use std::process::Command;
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------- 读入文本（跨平台）
/// 从 stdin 读全文，统一换行，去掉末尾多余空行。交互终端下打印提示。
///
/// 跨平台 EOF 处理：
/// - Unix/macOS/Linux：Ctrl-D（字节 0x04）→ read 返回 0
/// - Windows：默认只认 Ctrl-Z（0x1A）为 EOF；这里同时识别 Ctrl-D（0x04）以兼容 Unix 习惯
pub fn read_text() -> String {
    let interactive = io::stdin().is_terminal();
    if interactive {
        #[cfg(target_os = "windows")]
        println!("请输入要输入的文本，输入完后按回车，再按 Ctrl-Z（或 Ctrl-D）结束：");
        #[cfg(not(target_os = "windows"))]
        println!("请输入要输入的文本，输入完后按回车，再按 Ctrl-D 结束：");
    }
    let mut s = String::new();
    let mut buf = [0u8; 1024];
    loop {
        match io::stdin().read(&mut buf) {
            Ok(0) => break, // EOF（Unix 的 Ctrl-D，或 Windows 的 Ctrl-Z 被 C 运行时转为 EOF）
            Ok(n) => {
                #[cfg(target_os = "windows")]
                {
                    // Windows 控制台可能把 Ctrl-D 作为原始字节 0x04 传入，手动识别为 EOF
                    if let Some(pos) = buf[..n].iter().position(|&b| b == 0x04) {
                        s.push_str(&String::from_utf8_lossy(&buf[..pos]));
                        break;
                    }
                }
                s.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
            Err(_) => {
                eprintln!("读取输入失败");
                std::process::exit(1);
            }
        }
    }
    // 统一换行，去掉末尾多余空行
    let mut s = s.replace("\r\n", "\n").replace('\r', "\n");
    while s.ends_with('\n') {
        s.pop();
    }
    s
}

// ---------------------------------------------------------------- 倒计时（跨平台）
/// 逐秒倒计时，提示用户把光标点到目标位置。`secs == 0` 时不等待立即返回（GUI 自绘倒计时）。
pub fn countdown(secs: u64) {
    if secs == 0 {
        return; // 0 秒 = 立即注入（GUI 端自己显示倒计时，CLI 不再等待）
    }
    for i in (1..=secs).rev() {
        print!("\r{} 秒后开始输入，请把光标点到目标位置…  ", i);
        let _ = io::Write::flush(&mut io::stdout());
        thread::sleep(Duration::from_secs(1));
    }
    println!("\r开始输入…                                  ");
}

// ---------------------------------------------------------------- 文本解析（纯函数，可单测）
/// 单个字符能否被注入（三平台后端统一口径：可打印 ASCII + 空白/换行/Tab）
pub fn is_injectable(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\t') || c.is_ascii_graphic() || c == ' '
}

/// 预览结果：文本会被如何解析（GUI 预览弹窗与调试用，不注入）
pub struct Preview {
    pub chars: usize,       // 总字符数
    pub lines: usize,       // 行数
    pub keys: usize,        // 将注入的键数（含回车/Tab）
    pub skipped_count: usize, // 被跳过的非 ASCII 字符总数
    pub skipped: Vec<char>,   // 被跳过的字符样本（前 50 个）
    pub est_secs: f64,      // 预计耗时（秒）
}

/// 纯函数：分析文本的注入计划。CLI 与 GUI 共用；不注入任何键盘事件，安全。
pub fn analyze(text: &str, delay_ms: u64) -> Preview {
    let mut keys = 0usize;
    let mut skipped_count = 0usize;
    let mut skipped: Vec<char> = Vec::new();
    for ch in text.chars() {
        if is_injectable(ch) {
            keys += 1;
        } else {
            skipped_count += 1;
            if skipped.len() < 50 {
                skipped.push(ch);
            }
        }
    }
    let est_secs = keys as f64 * (delay_ms as f64 + 3.0) / 1000.0;
    Preview {
        chars: text.chars().count(),
        lines: text.lines().count(),
        keys,
        skipped_count,
        skipped,
        est_secs,
    }
}

// ---------------------------------------------------------------- 平台注入后端
/// 各平台把文本注入到前台窗口。返回 (注入键数, 被跳过的非 ASCII 字符)。
/// `delay_ms` = 每键间隔毫秒；`secs` 倒计时由调用方负责（CLI 用 countdown，GUI 自绘）。
#[cfg(target_os = "macos")]
pub mod platform {
    use super::*;

    /// macOS US ANSI 虚拟键码 -> (键码, 是否需要 Shift)
    fn key_of(c: char) -> Option<(u32, bool)> {
        Some(match c {
            'a' => (0, false),   's' => (1, false),   'd' => (2, false),
            'f' => (3, false),   'h' => (4, false),   'g' => (5, false),
            'z' => (6, false),   'x' => (7, false),   'c' => (8, false),
            'v' => (9, false),   'b' => (11, false),  'q' => (12, false),
            'w' => (13, false),  'e' => (14, false),  'r' => (15, false),
            'y' => (16, false),  't' => (17, false),  'o' => (31, false),
            'u' => (32, false),  'i' => (34, false),  'p' => (35, false),
            'l' => (37, false),  'j' => (38, false),  'k' => (40, false),
            'n' => (45, false),  'm' => (46, false),
            '1' => (18, false),  '2' => (19, false),  '3' => (20, false),
            '4' => (21, false),  '5' => (23, false),  '6' => (22, false),
            '7' => (26, false),  '8' => (28, false),  '9' => (25, false),
            '0' => (29, false),
            '-' => (27, false),  '=' => (24, false),  '[' => (33, false),
            ']' => (30, false),  '\\' => (42, false), ';' => (41, false),
            '\'' => (39, false), ',' => (43, false),  '.' => (47, false),
            '/' => (44, false),  '`' => (50, false),
            ' ' => (49, false),  '\t' => (48, false),
            '\n' | '\r' => (36, false),

            'A'..='Z' => {
                let (k, _) = key_of(c.to_ascii_lowercase())?;
                (k, true)
            }
            '!' => (18, true),   '@' => (19, true),   '#' => (20, true),
            '$' => (21, true),   '%' => (23, true),   '^' => (22, true),
            '&' => (26, true),   '*' => (28, true),   '(' => (25, true),
            ')' => (29, true),   '_' => (27, true),   '+' => (24, true),
            '{' => (33, true),   '}' => (30, true),   '|' => (42, true),
            ':' => (41, true),   '"' => (39, true),   '<' => (43, true),
            '>' => (47, true),   '?' => (44, true),   '~' => (50, true),

            _ => return None,
        })
    }

    /// 把文本编译成一条 AppleScript，逐键发 key code（虚拟机场景唯一可靠的方式）。
    pub fn inject(text: &str, delay_ms: u64) -> Result<(usize, Vec<char>), String> {
        let mut flat: Vec<i64> = Vec::new();
        let mut keys = 0usize;
        let mut skipped: Vec<char> = Vec::new();

        for ch in text.chars() {
            match key_of(ch) {
                Some((k, s)) => {
                    flat.push(k as i64);
                    flat.push(if s { 1 } else { 0 });
                    keys += 1;
                }
                None => {
                    if skipped.len() < 20 {
                        skipped.push(ch);
                    }
                }
            }
        }
        if flat.is_empty() {
            return Err("没有可输入的内容（只支持英文/数字/标点）".into());
        }

        let list = flat.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
        let line_delay_ms = (delay_ms * 3).max(60);
        let script = format!(
            r#"set d to {d}
set d2 to {d2}
set ks to {{{list}}}
tell application "System Events"
	set n to (count of ks)
	set i to 1
	repeat while i < n
		set k to item i of ks
		set s to item (i + 1) of ks
		if s is 1 then
			key code k using shift down
		else
			key code k
		end if
		delay d
		if k is 36 then delay d2
		set i to i + 2
	end repeat
end tell"#,
            d = (delay_ms as f64) / 1000.0,
            d2 = (line_delay_ms as f64) / 1000.0,
            list = list
        );

        let path = "/tmp/tt_inject.applescript";
        std::fs::write(path, script).map_err(|e| e.to_string())?;
        let out = Command::new("osascript")
            .arg(path)
            .output()
            .map_err(|e| format!("启动 osascript 失败: {}", e))?;
        if !out.status.success() {
            return Err(format!("注入失败：{}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok((keys, skipped))
    }

    /// 自检：分别用 Unicode 字符串注入与 keycode 注入各打一行，看虚拟机认哪种。
    pub fn probe(secs: u64, delay_ms: u64) {
        countdown(secs);
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to keystroke \"AAA-unicode-111\""])
            .status();
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to key code 36"])
            .status();
        thread::sleep(Duration::from_millis(300));
        let _ = inject("BBB-keycode-222", delay_ms);
        println!("看目标窗口出现哪一行：只出 BBB-keycode-222 就是正常的。");
    }
}

/// Linux：调用 xdotool（需目标机已安装，如 apt install xdotool）
#[cfg(target_os = "linux")]
pub mod platform {
    use super::*;

    fn xdotool(args: &[&str]) -> Result<(), String> {
        match Command::new("xdotool").args(args).output() {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(format!("xdotool 失败：{}", String::from_utf8_lossy(&o.stderr).trim())),
            Err(e) => Err(format!("找不到 xdotool，请先安装（如 apt install xdotool）：{}", e)),
        }
    }

    pub fn inject(text: &str, delay_ms: u64) -> Result<(usize, Vec<char>), String> {
        let d = delay_ms.to_string();
        let mut keys = 0usize;
        let mut skipped: Vec<char> = Vec::new();
        for ch in text.chars() {
            match ch {
                '\n' | '\r' => xdotool(&["key", "--delay", &d, "Return"])?,
                '\t' => xdotool(&["key", "--delay", &d, "Tab"])?,
                c if c.is_ascii() => xdotool(&["type", "--delay", &d, &c.to_string()])?,
                _ => skipped.push(ch),
            }
            keys += 1;
        }
        if keys == 0 {
            return Err("没有可输入的内容（只支持英文/数字/标点）".into());
        }
        Ok((keys, skipped))
    }

    pub fn probe(_secs: u64, _delay_ms: u64) {
        println!("（Linux 需目标机已装 xdotool；下面会在倒计时后打一行标记）");
        countdown(_secs);
        let _ = inject("PROBE-OK-123", _delay_ms);
        println!("看目标窗口是否出现 PROBE-OK-123。");
    }
}

/// Windows：调用 enigo（Win32 SendInput，逐字打入并加间隔）
#[cfg(target_os = "windows")]
pub mod platform {
    use super::*;
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    pub fn inject(text: &str, delay_ms: u64) -> Result<(usize, Vec<char>), String> {
        let mut enigo = Enigo::new(&Settings::default()).map_err(|e| format!("初始化 enigo 失败：{:?}", e))?;
        let gap = Duration::from_millis(delay_ms);
        let mut keys = 0usize;
        let mut skipped: Vec<char> = Vec::new();
        for ch in text.chars() {
            match ch {
                '\n' | '\r' => { let _ = enigo.key(Key::Return, Direction::Click); }
                '\t' => { let _ = enigo.key(Key::Tab, Direction::Click); }
                c if c.is_ascii() => { let _ = enigo.text(&c.to_string()); }
                _ => skipped.push(ch),
            }
            keys += 1;
            thread::sleep(gap);
        }
        if keys == 0 {
            return Err("没有可输入的内容（只支持英文/数字/标点）".into());
        }
        Ok((keys, skipped))
    }

    pub fn probe(_secs: u64, _delay_ms: u64) {
        println!("（Windows 会在倒计时后打一行标记）");
        countdown(_secs);
        let _ = inject("PROBE-OK-123", _delay_ms);
        println!("看目标窗口是否出现 PROBE-OK-123。");
    }
}

// ---------------------------------------------------------------- GUI（非 musl 构建）
// eframe/rfd 只在非 musl 目标上编译（见 Cargo.toml），模块同步条件编译，
// 保证 Linux musl 静态交叉版仍是纯 CLI。
// GUI 模块门控与 Cargo.toml 保持一致：非 musl 即含 GUI（Windows 用 glow 后端避开 D3D12 编译问题）
#[cfg(not(target_env = "musl"))]
pub mod gui;

