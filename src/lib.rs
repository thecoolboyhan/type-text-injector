//! type-text 核心库：CLI (main.rs) 与 GUI (gui.rs) 共用。
//!
//! 职责：读文本 → 倒计时 → 平台注入（macOS key code / Linux xdotool / Windows enigo）。
//! 关键设计：注入必须走 **keycode 级** 事件（虚拟机只认 HID 扫描码），
//! 任何「整段 keystroke / Unicode 字符串」的简化都会让虚拟机场景崩掉（见 AGENTS.md 第 2 节）。

use std::io::{self, IsTerminal, Read};
#[cfg(not(target_os = "windows"))]
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
    // 先按字节全量收齐，最后一次性解码。逐块 from_utf8_lossy 会在 UTF-8 多字节字符
    // 跨越 1024 字节边界时把字符替换成 U+FFFD，导致长中文文本乱码、字符数虚高。
    let mut bytes: Vec<u8> = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        match io::stdin().read(&mut buf) {
            Ok(0) => break, // EOF（Unix 的 Ctrl-D，或 Windows 的 Ctrl-Z 被 C 运行时转为 EOF）
            Ok(n) => {
                #[cfg(target_os = "windows")]
                {
                    // Windows 控制台可能把 Ctrl-D 作为原始字节 0x04 传入，手动识别为 EOF
                    if let Some(pos) = buf[..n].iter().position(|&b| b == 0x04) {
                        bytes.extend_from_slice(&buf[..pos]);
                        break;
                    }
                }
                bytes.extend_from_slice(&buf[..n]);
            }
            Err(_) => {
                eprintln!("读取输入失败");
                std::process::exit(1);
            }
        }
    }
    let mut s = String::from_utf8_lossy(&bytes).into_owned();
    // 统一换行，去掉末尾多余空行
    s = s.replace("\r\n", "\n").replace('\r', "\n");
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

/// 结尾「跳过字符」提示的样本上限。三端 inject 与 GUI 预览统一，避免长中文文本刷屏。
pub const SKIPPED_SAMPLE_LIMIT: usize = 20;

/// 统一的注入计划。**三端 inject、GUI 预览、CLI 预计耗时共用这一份口径**，
/// 保证「哪些字符算键 / 键数统计 / 跳过样本」在任何平台都不漂移。
pub struct InjectPlan {
    /// 需要逐键输入的字符（已过滤非 ASCII 与不可注入字符）
    pub keys: Vec<char>,
    /// 被跳过的字符样本（最多 SKIPPED_SAMPLE_LIMIT 个）
    pub skipped: Vec<char>,
    /// 被跳过的字符总数
    pub skipped_count: usize,
}

/// 纯函数：把文本拆成注入计划。无任何副作用，便于单测。
pub fn plan(text: &str) -> InjectPlan {
    let mut keys: Vec<char> = Vec::with_capacity(text.len());
    let mut skipped: Vec<char> = Vec::new();
    let mut skipped_count = 0usize;
    for ch in text.chars() {
        if is_injectable(ch) {
            keys.push(ch);
        } else {
            skipped_count += 1;
            if skipped.len() < SKIPPED_SAMPLE_LIMIT {
                skipped.push(ch);
            }
        }
    }
    InjectPlan { keys, skipped, skipped_count }
}

/// 三端 `inject()` 的统一返回结构。CLI 与 GUI 都从这里取数，
/// 避免把「跳过样本数」误当成「跳过总数」显示。
pub struct InjectResult {
    /// 实际注入的键数
    pub keys: usize,
    /// 被跳过的字符总数
    pub skipped_count: usize,
    /// 被跳过的字符样本（最多 SKIPPED_SAMPLE_LIMIT 个）
    pub skipped: Vec<char>,
}

/// 预览结果：文本会被如何解析（GUI 预览弹窗与调试用，不注入）
pub struct Preview {
    pub chars: usize,         // 总字符数
    pub lines: usize,         // 行数
    pub keys: usize,          // 将注入的键数（含回车/Tab）
    pub skipped_count: usize, // 被跳过的非 ASCII 字符总数
    pub skipped: Vec<char>,   // 被跳过的字符样本（最多 SKIPPED_SAMPLE_LIMIT 个）
    pub est_secs: f64,        // 预计耗时（秒）
}

/// 纯函数：分析文本的注入计划。CLI 与 GUI 共用；不注入任何键盘事件，安全。
pub fn analyze(text: &str, delay_ms: u64) -> Preview {
    let p = plan(text);
    Preview {
        chars: text.chars().count(),
        lines: text.lines().count(),
        keys: p.keys.len(),
        skipped_count: p.skipped_count,
        skipped: p.skipped,
        est_secs: p.keys.len() as f64 * (delay_ms as f64 + 3.0) / 1000.0,
    }
}

// ---------------------------------------------------------------- 统一口径单测（三端共通）
#[cfg(test)]
mod common_tests {
    use super::*;

    #[test]
    fn plan_counts_only_injectable_chars() {
        let p = plan("ab中文cd\ne");
        assert_eq!(p.keys, vec!['a', 'b', 'c', 'd', '\n', 'e']);
        assert_eq!(p.skipped_count, 2);
        assert_eq!(p.skipped, vec!['中', '文']);
    }

    #[test]
    fn plan_caps_skipped_samples() {
        let p = plan(&"中".repeat(100));
        assert!(p.keys.is_empty());
        assert_eq!(p.skipped_count, 100);
        assert_eq!(p.skipped.len(), SKIPPED_SAMPLE_LIMIT);
    }

    #[test]
    fn analyze_agrees_with_plan() {
        let text = "Hello 中文\nWorld";
        let p = plan(text);
        let a = analyze(text, 50);
        assert_eq!(a.keys, p.keys.len());
        assert_eq!(a.skipped_count, p.skipped_count);
        assert_eq!(a.skipped, p.skipped);
        assert!((a.est_secs - p.keys.len() as f64 * 0.053).abs() < 1e-9);
    }
}

// ---------------------------------------------------------------- 平台注入后端
/// 各平台把文本注入到前台窗口，统一返回 `InjectResult`（键数 / 跳过总数 / 跳过样本）。
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

    /// 单条 AppleScript 最多容纳的键数。
    /// 实测 osascript 运行时 `set ks to {...}` 字面量约 8080 个数字项即报「堆栈溢出(-2706)」，
    /// 即 4039 键；取 4000 留余量。每个键占 2 项：虚拟键码 + 是否需要 Shift。
    /// 长文本按此大小切片，逐批生成脚本依次执行（仍然是 key code 级注入）。
    const MAX_KEYS_PER_BATCH: usize = 4000;

    /// 纯函数：把 plan() 筛出的键编译成 [keycode, shift, ...]。
    /// 若键位表出现映射缺口，直接报错而不是静默少打一个键（保证三端键数一致）。
    fn compile_keys(keys: &[char]) -> Result<Vec<i64>, String> {
        let mut flat: Vec<i64> = Vec::with_capacity(keys.len() * 2);
        for &ch in keys {
            match key_of(ch) {
                Some((k, s)) => {
                    flat.push(k as i64);
                    flat.push(if s { 1 } else { 0 });
                }
                None => return Err(format!("键位表缺少字符 {:?} 的 US ANSI 键码映射", ch)),
            }
        }
        Ok(flat)
    }

    /// 把一批键序列编译成 AppleScript 源码。虚拟机只认 HID 扫描码，
    /// 因此这里必须是 `key code N`（可带 `using shift down`），不能改回整段 keystroke。
    fn build_script(flat: &[i64], delay_ms: u64, line_delay_ms: u64) -> String {
        let list = flat.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
        format!(
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
        )
    }

    /// 把文本逐键注入当前前台窗口。超过 MAX_KEYS_PER_BATCH 时自动分批，
    /// 每批生成一条独立的 AppleScript 依次执行；接口与 CLI 契约保持不变。
    /// 返回 (总注入键数, 被跳过的非 ASCII 字符样本)。
    pub fn inject(text: &str, delay_ms: u64) -> Result<InjectResult, String> {
        let p = plan(text);
        let flat = compile_keys(&p.keys)?;
        if flat.is_empty() {
            return Err("没有可输入的内容（只支持英文/数字/标点）".into());
        }

        let keys = flat.len() / 2;
        let line_delay_ms = delay_ms.saturating_mul(3).max(60);
        let chunks: Vec<&[i64]> = flat.chunks(MAX_KEYS_PER_BATCH * 2).collect();
        let batches = chunks.len();
        let path = "/tmp/tt_inject.applescript";

        for (idx, chunk) in chunks.iter().enumerate() {
            if idx > 0 {
                // 上一条 osascript 退出后给目标窗口一点缓冲，避免批次交界处丢字
                thread::sleep(Duration::from_millis(line_delay_ms));
            }
            let script = build_script(chunk, delay_ms, line_delay_ms);
            std::fs::write(path, script).map_err(|e| format!("写入注入脚本失败: {}", e))?;
            let out = Command::new("osascript")
                .arg(path)
                .output()
                .map_err(|e| format!("启动 osascript 失败: {}", e))?;
            if !out.status.success() {
                return Err(format!(
                    "注入失败（第 {}/{} 批，共 {} 键）：{}",
                    idx + 1,
                    batches,
                    keys,
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
        }
        Ok(InjectResult { keys, skipped_count: p.skipped_count, skipped: p.skipped })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn compile_keys_maps_ascii_and_whitespace() {
            // A \n \t b ' ' '1' => 6 键
            let p = plan("A\n\tb 1");
            let flat = compile_keys(&p.keys).unwrap();
            assert_eq!(flat.len(), 12, "每键应占 2 项");
            assert!(p.skipped.is_empty());
            assert_eq!(&flat[0..2], &[0, 1], "A = keycode 0 + shift");
            assert_eq!(&flat[2..4], &[36, 0], "换行 = keycode 36");
            assert_eq!(&flat[4..6], &[48, 0], "Tab = keycode 48");
            assert_eq!(&flat[10..12], &[18, 0], "1 = keycode 18");
        }

        #[test]
        fn compile_keys_skips_non_ascii_via_plan() {
            let p = plan("ab中文cd");
            let flat = compile_keys(&p.keys).unwrap();
            assert_eq!(flat.len(), 8);
            assert_eq!(p.skipped, vec!['中', '文']);
            assert_eq!(p.skipped_count, 2);
        }

        #[test]
        fn every_injectable_char_has_a_keycode() {
            // plan() 认定的字符必须都能映射，否则 macOS 键数会与另外两端对不上。
            let mut chars: Vec<char> = (0x21u8..=0x7eu8).map(|b| b as char).collect();
            chars.extend([' ', '\t', '\r', '\n']);
            for c in chars {
                assert!(key_of(c).is_some(), "字符 {:?} 缺少 US ANSI 键码映射", c);
            }
        }

        #[test]
        fn long_text_splits_into_even_sized_batches() {
            let text = "a".repeat(MAX_KEYS_PER_BATCH * 3 + 7);
            let flat = compile_keys(&plan(&text).keys).unwrap();
            let chunks: Vec<&[i64]> = flat.chunks(MAX_KEYS_PER_BATCH * 2).collect();
            assert_eq!(chunks.len(), 4, "3 满批 + 1 尾巴");
            let mut total = 0usize;
            for c in &chunks {
                assert_eq!(c.len() % 2, 0, "切片不能拆开 (keycode, shift) 对");
                for (i, v) in c.iter().enumerate() {
                    if i % 2 == 1 {
                        assert!(*v == 0 || *v == 1, "奇数位只能是 shift 标志");
                    }
                }
                total += c.len() / 2;
            }
            assert_eq!(total, text.chars().count(), "分批不能丢键");
        }

        #[test]
        fn script_stays_keycode_based() {
            let flat = compile_keys(&plan("Hello").keys).unwrap();
            let s = build_script(&flat, 50, 150);
            assert!(s.contains("key code k"), "必须保持 key code 级注入");
            assert!(!s.contains("keystroke"), "不得回归到 Unicode 字符串注入");
            assert!(s.contains("set ks to {"));
            assert!(s.contains("set d to 0.05"));
        }
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

    pub fn inject(text: &str, delay_ms: u64) -> Result<InjectResult, String> {
        let d = delay_ms.to_string();
        let p = plan(text);
        if p.keys.is_empty() {
            return Err("没有可输入的内容（只支持英文/数字/标点）".into());
        }
        // 字符过滤由统一 plan() 负责：只有可注入字符会走到这里，键数即 p.keys.len()。
        for ch in &p.keys {
            match *ch {
                '\n' | '\r' => xdotool(&["key", "--delay", &d, "Return"])?,
                '\t' => xdotool(&["key", "--delay", &d, "Tab"])?,
                c => xdotool(&["type", "--delay", &d, &c.to_string()])?,
            }
        }
        Ok(InjectResult { keys: p.keys.len(), skipped_count: p.skipped_count, skipped: p.skipped })
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

    pub fn inject(text: &str, delay_ms: u64) -> Result<InjectResult, String> {
        let mut enigo = Enigo::new(&Settings::default()).map_err(|e| format!("初始化 enigo 失败：{:?}", e))?;
        let gap = Duration::from_millis(delay_ms);
        let p = plan(text);
        if p.keys.is_empty() {
            return Err("没有可输入的内容（只支持英文/数字/标点）".into());
        }
        // 同 Linux：字符过滤与计数统一交给 plan()。
        for ch in &p.keys {
            match *ch {
                '\n' | '\r' => { let _ = enigo.key(Key::Return, Direction::Click); }
                '\t' => { let _ = enigo.key(Key::Tab, Direction::Click); }
                c => { let _ = enigo.text(&c.to_string()); }
            }
            thread::sleep(gap);
        }
        Ok(InjectResult { keys: p.keys.len(), skipped_count: p.skipped_count, skipped: p.skipped })
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

