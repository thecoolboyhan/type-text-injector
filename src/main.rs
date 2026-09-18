//! type-text —— 把本机文本模拟键盘输入到当前光标处（含虚拟机/远程桌面）
//!
//! 启动 → 粘贴文本 → 回车后按 Ctrl-D → 等 5 秒 → 自动打字
//!
//! 用法：
//!   ./type-text                  # 准备 5 秒，每键间隔 50ms
//!   ./type-text 10               # 准备 10 秒
//!   ./type-text 5 80             # 准备 5 秒，每键 80ms（虚拟机丢字就调大这个）
//!   ./type-text -n               # 只预览不注入
//!   ./type-text -p               # 自检：看虚拟机认哪种注入方式
//!
//! 关键实现点：往 Parallels / VMware / 远程桌面里打字，必须用 **keycode 级**注入
//! （CGEvent 携带真实 virtual keycode，客户机能映射成 HID 扫描码）。
//! 不能用 `keystroke "整串文本"` 那种 **Unicode 字符串**注入——虚拟机侧收不到，会整串丢弃。
//!
//! 编译：rustc -O src/main.rs -o type-text   （或：cargo build --release）

use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::process::Command;
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------- 键位表
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

        // 大写字母 = 小写键 + Shift
        'A'..='Z' => {
            let (k, _) = key_of(c.to_ascii_lowercase())?;
            (k, true)
        }
        // 上档符号 = 对应键 + Shift
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

// ---------------------------------------------------------------- 读入文本
fn read_text() -> String {
    let interactive = io::stdin().is_terminal();
    if interactive {
        println!("请输入要输入的文本，输入完后按回车，再按 Ctrl-D 结束：");
    }
    let mut s = String::new();
    if io::stdin().read_to_string(&mut s).is_err() {
        eprintln!("读取输入失败");
        std::process::exit(1);
    }
    // 统一换行，去掉末尾多余空行
    let mut s = s.replace("\r\n", "\n").replace('\r', "\n");
    while s.ends_with('\n') {
        s.pop();
    }
    s
}

// ---------------------------------------------------------------- 注入
/// 把文本编译成一条 AppleScript，逐键发 keycode（Shift 用 `using shift down`）
/// delay_ms：每个键之后的停顿；虚拟机里丢字就把它调大。
fn inject(text: &str, delay_ms: u64) -> Result<(usize, Vec<char>), String> {
    let mut flat: Vec<i64> = Vec::new(); // 交错存放：键码, Shift标志
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
    // 行末停顿取键间停顿的 3 倍（至少 60ms），回车后给目标端多一点喘息
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
    fs::write(path, script).map_err(|e| e.to_string())?;

    let out = Command::new("osascript")
        .arg(path)
        .output()
        .map_err(|e| format!("启动 osascript 失败: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "注入失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok((keys, skipped))
}

/// 隐藏用途：确认虚拟机认哪种注入方式（-p）
fn probe(secs: u64, delay_ms: u64) {
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

// ---------------------------------------------------------------- 倒计时
fn countdown(secs: u64) {
    for i in (1..=secs.max(1)).rev() {
        print!("\r{} 秒后开始输入，请把光标点到目标位置…  ", i);
        let _ = io::Write::flush(&mut io::stdout());
        thread::sleep(Duration::from_secs(1));
    }
    println!("\r开始输入…                                  ");
}

// ---------------------------------------------------------------- 主流程
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    // 可选参数：第 1 个裸数字 = 准备秒数，第 2 个 = 每键停顿毫秒；-p 自检；-n 只预览不注入
    let mut secs = 5u64;
    let mut delay_ms = 50u64;
    let mut nums: Vec<u64> = Vec::new();
    let mut dry = false;
    let mut do_probe = false;
    for a in &args {
        match a.as_str() {
            "-p" => do_probe = true,
            "-n" => dry = true,
            _ => {
                if let Ok(n) = a.parse::<u64>() {
                    nums.push(n);
                }
            }
        }
    }
    if let Some(n) = nums.first() {
        secs = *n;
    }
    if let Some(n) = nums.get(1) {
        delay_ms = *n;
    }

    if do_probe {
        probe(secs, delay_ms);
        return;
    }

    let text = read_text();
    if text.trim().is_empty() {
        eprintln!("没有输入任何内容。");
        std::process::exit(1);
    }
    let est = text.chars().count() as f64 * (delay_ms as f64 + 3.0) / 1000.0;
    println!(
        "已收到 {} 行 / {} 字符，每键间隔 {}ms，预计耗时约 {:.0} 秒；{} 秒后开始输入。",
        text.lines().count(),
        text.chars().count(),
        delay_ms,
        est,
        secs
    );

    if dry {
        println!("[预览] 前 10 行：");
        for (i, l) in text.lines().take(10).enumerate() {
            println!("{:>3}| {}", i + 1, l.replace('\t', "<TAB>").replace(' ', "·"));
        }
        return;
    }

    countdown(secs);

    match inject(&text, delay_ms) {
        Ok((n, sk)) => {
            println!("完成，共输入 {} 个键。", n);
            if !sk.is_empty() {
                eprintln!(
                    "跳过 {} 个非英文/数字/标点字符：{}",
                    sk.len(),
                    sk.iter().collect::<String>()
                );
            }
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}
