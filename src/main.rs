//! type-text —— 把本机文本模拟键盘输入到当前光标处（含虚拟机/远程桌面）
//!
//! 启动 → 粘贴文本 → 回车后按 Ctrl-D → 等 5 秒 → 自动打字
//! 全平台：macOS / Linux / Windows（各平台注入后端见 lib.rs）
//!
//! 用法：
//!   ./type-text                  # 准备 5 秒，每键间隔 50ms
//!   ./type-text 10               # 准备 10 秒
//!   ./type-text 5 80             # 准备 5 秒，每键 80ms（虚拟机丢字就调大这个）
//!   ./type-text -n               # 只预览不注入
//!   ./type-text -p               # 自检
//!   ./type-text --gui            # 打开可视化界面（macOS/Windows 原生构建）
//!
//! 编译：cargo build --release    （产物 target/release/type-text）

use std::env;
use type_text_injector::{countdown, platform, read_text};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    // GUI 入口：只有非 musl 构建（macOS/Windows/linux-gnu）带 GUI 模块；
    // musl 静态交叉版是纯 CLI，给出明确提示。
    if args.iter().any(|a| a == "--gui") {
        #[cfg(not(target_env = "musl"))]
        {
            type_text_injector::gui::run();
            return;
        }
        #[cfg(target_env = "musl")]
        {
            eprintln!("此构建（Linux musl 静态版）不含 GUI，请用 macOS / Windows 原生构建。");
            std::process::exit(1);
        }
    }

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
        platform::probe(secs, delay_ms);
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

    match platform::inject(&text, delay_ms) {
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
