//! TypeText GUI —— egui 原生可视化界面（`type-text --gui` 入口）
//!
//! 功能：文本框（输入/粘贴/从文件读取）→ 预览解析 → 倒计时 → 直接调 lib 注入。
//! 视觉：Material You 风格（Google 蓝、圆角卡片、平滑交互动画、环形倒计时、注入 spinner）。

use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::platform;
use crate::{analyze, Preview};

/// egui 通过 eframe 引用（eframe 依赖树中自带，不单独引 crate）
use eframe::egui;

// ---------------------------------------------------------------- 配色（Material 3 浅色）
const PRIMARY: egui::Color32 = egui::Color32::from_rgb(0x1A, 0x73, 0xE8); // Google 蓝
const PRIMARY_DARK: egui::Color32 = egui::Color32::from_rgb(0x17, 0x5C, 0xC1);
const ON_PRIMARY: egui::Color32 = egui::Color32::WHITE;
const BG: egui::Color32 = egui::Color32::from_rgb(0xF6, 0xF8, 0xFB); // 页面背景浅灰蓝
const CARD: egui::Color32 = egui::Color32::WHITE; // 卡片纯白
const CARD_BORDER: egui::Color32 = egui::Color32::from_rgb(0xEC, 0xEF, 0xF4); // 卡片边框（更淡）
const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(0x1F, 0x21, 0x24);
const TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(0x5F, 0x63, 0x68);
const TRACK: egui::Color32 = egui::Color32::from_rgb(0xE0, 0xE6, 0xEF); // 进度环底轨
const SUCCESS: egui::Color32 = egui::Color32::from_rgb(0x18, 0x80, 0x38); // Google 绿
const ERROR: egui::Color32 = egui::Color32::from_rgb(0xD9, 0x30, 0x25); // Google 红
const WARNING: egui::Color32 = egui::Color32::from_rgb(0xF9, 0xAB, 0x00); // 倒计时橙

/// 颜色线性插值（最后 3 秒倒计时环 橙→红 渐变用）
fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from_rgb(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
    )
}

/// 后台注入线程回传的结果
enum InjectMsg {
    Done(Result<(usize, Vec<char>), String>),
}

pub fn run() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([640.0, 680.0])
            .with_min_inner_size([480.0, 460.0])
            .with_title("TypeText 文本注入器"),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "type-text-gui",
        options,
        Box::new(|cc| {
            setup_cjk_font(&cc.egui_ctx);
            apply_material_theme(&cc.egui_ctx);
            Ok(Box::new(GuiApp::new()))
        }),
    );
}

/// 从系统加载中文字体（egui 默认字体不含中文，且与 CJK 混排观感割裂）。
/// - Proportional（界面文字）：中文字体插到**首位**，中英混排风格统一（黑体自带拉丁字形）
/// - Monospace（文本区）：保持等宽字体优先，中文追加到末尾做回退
fn setup_cjk_font(ctx: &egui::Context) {
    // 按「现代→通用」排序：macOS 优先冬青黑体（比华文黑体笔画现代），再退苹方/华文
    #[cfg(target_os = "macos")]
    const CANDIDATES: &[&str] = &[
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
    ];
    #[cfg(target_os = "windows")]
    const CANDIDATES: &[&str] = &[
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyh.ttf",
        "C:\\Windows\\Fonts\\simhei.ttf",
    ];
    #[cfg(all(target_os = "linux", not(target_env = "musl")))]
    const CANDIDATES: &[&str] = &[
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/truetype/arphic/uming.ttc",
    ];

    let mut fonts = egui::FontDefinitions::default();
    for path in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else { continue };
        fonts
            .font_data
            .insert("cjk".to_owned(), egui::FontData::from_owned(bytes).into());
        // Proportional 插首位统一观感；Monospace 追加末尾做回退（保住等宽特性）
        if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            list.insert(0, "cjk".to_owned());
        }
        if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            list.push("cjk".to_owned());
        }
        break;
    }
    ctx.set_fonts(fonts);
}

/// Material You 风格主题：浅色背景、圆角控件、整体字号调大、交互动画。
fn apply_material_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = BG;

    // 控件统一圆角（Material 3：更圆润）
    let corner = egui::CornerRadius::same(14);
    for w in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = corner;
        w.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0xD0, 0xD5, 0xDD));
    }
    visuals.widgets.inactive.bg_fill = CARD;
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0xF0, 0xF5, 0xFB);
    visuals.widgets.hovered.bg_stroke.color = PRIMARY;
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0xE2, 0xEC, 0xFA);
    visuals.selection.bg_fill = PRIMARY;
    visuals.selection.stroke.color = ON_PRIMARY;

    // 文字颜色
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.text_cursor.stroke = egui::Stroke::new(2.0, PRIMARY);

    ctx.set_visuals(visuals);

    // 字号调大（egui 默认 12.5px 中文显小发虚）+ 间距 + 交互动画时长
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(18.0, 9.0);
        style.spacing.interact_size.y = 38.0;
        style.animation_time = 0.3; // hover/点击平滑过渡（Material 动效时长）
        style.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.5));
        style.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.5));
        style.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.5));
        style.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(24.0));
        style.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(14.0));
    });
}

/// 白色圆角卡片容器：强制撑满可用宽度，随窗口缩放
fn card(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width();
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(16)
        .inner_margin(egui::Margin::same(16))
        .stroke(egui::Stroke::new(1.0, CARD_BORDER))
        .show(ui, |ui| {
            ui.set_min_width(width - 32.0); // 减去左右 margin，卡片撑满
            add_contents(ui);
        });
}

struct GuiApp {
    text: String,
    secs: u64,
    delay_ms: u64,
    status: String,
    status_error: bool,
    busy: bool,                  // 注入中（禁用按钮）
    countdown_left: Option<f32>, // Some(剩余秒数) = 正在倒计时
    countdown_end: Option<Instant>,
    countdown_display: f32,      // 平滑过渡的剩余秒数（数字滚动动画）
    preview_open: bool,          // 预览弹窗
    preview: Option<Preview>,
    start_hovered: bool,         // 上一帧「开始输入」是否被悬停（主按钮 hover 变色）
    inject_tx: Sender<InjectMsg>,
    inject_rx: Receiver<InjectMsg>,
}

impl GuiApp {
    fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            text: String::new(),
            secs: 5,
            delay_ms: 50,
            status: "就绪。".into(),
            status_error: false,
            busy: false,
            countdown_left: None,
            countdown_end: None,
            countdown_display: 0.0,
            preview_open: false,
            preview: None,
            start_hovered: false,
            inject_tx: tx,
            inject_rx: rx,
        }
    }

    /// 点「开始输入」：校验 → 进入倒计时（0 秒则直接注入）
    fn start(&mut self) {
        if self.busy {
            return;
        }
        if self.text.trim().is_empty() {
            self.set_status("文本为空，请先输入或粘贴内容", true);
            return;
        }
        let secs = self.secs;
        if secs == 0 {
            self.spawn_inject();
        } else {
            self.countdown_end = Some(Instant::now() + Duration::from_secs(secs));
            self.countdown_left = Some(secs as f32);
            self.countdown_display = secs as f32;
        }
    }

    /// 取消倒计时（注入进行中不可取消）
    fn cancel(&mut self) {
        if self.countdown_left.is_some() && !self.busy {
            self.countdown_left = None;
            self.countdown_end = None;
            self.set_status("已取消。", false);
        }
    }

    /// 倒计时到 0 → 启动后台注入线程
    fn spawn_inject(&mut self) {
        let text = self.text.clone();
        let delay_ms = self.delay_ms;
        let tx = self.inject_tx.clone();
        self.busy = true;
        self.countdown_left = None;
        self.set_status("正在输入… 不要移动焦点", false);
        std::thread::spawn(move || {
            let r = platform::inject(&text, delay_ms);
            let _ = tx.send(InjectMsg::Done(r));
        });
    }

    fn set_status(&mut self, msg: &str, err: bool) {
        self.status = msg.into();
        self.status_error = err;
    }

    /// 处理后台线程回传的注入结果
    fn poll_inject(&mut self) {
        if let Ok(InjectMsg::Done(r)) = self.inject_rx.try_recv() {
            self.busy = false;
            match r {
                Ok((n, sk)) => {
                    let mut msg = format!("✔ 完成，共输入 {} 个键。", n);
                    if !sk.is_empty() {
                        msg.push_str(&format!(
                            " 跳过 {} 个非英文/数字/标点字符：{}",
                            sk.len(),
                            sk.iter().collect::<String>()
                        ));
                    }
                    self.set_status(&msg, false);
                }
                Err(e) => {
                    self.set_status(
                        &format!(
                            "✘ {}\n提示：macOS 需在「系统设置 → 隐私与安全性 → 辅助功能」授权后注入才能生效",
                            e
                        ),
                        true,
                    );
                }
            }
        }
    }

    /// 从文件读取文本（UTF-8，兼容 BOM），等效 CLI 的 `type-text < file.txt`
    fn load_from_file(&mut self) {
        if self.busy {
            self.set_status("正在注入中，请稍候再读取文件", true);
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .set_title("选择要注入的文本文件")
            .pick_file()
        {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let mut s = String::from_utf8_lossy(&bytes).into_owned();
                    if s.starts_with('\u{FEFF}') {
                        s = s[3..].to_string(); // 去 UTF-8 BOM
                    }
                    let n = s.chars().count();
                    self.text = s;
                    self.set_status(
                        &format!(
                            "已从 {} 读取 {} 字符",
                            path.file_name().unwrap_or_default().to_string_lossy(),
                            n
                        ),
                        false,
                    );
                }
                Err(e) => self.set_status(&format!("读取文件失败：{}", e), true),
            }
        }
    }

    /// 状态文字颜色：错误红 / 注入中蓝 / 倒计时橙 / 默认绿
    fn status_color(&self) -> egui::Color32 {
        if self.status_error {
            ERROR
        } else if self.busy {
            PRIMARY
        } else if self.countdown_left.is_some() {
            WARNING
        } else {
            SUCCESS
        }
    }

    /// 倒计时环当前颜色：剩余 >3 秒橙色，最后 3 秒渐变到红（紧迫感）
    fn countdown_color(&self, left: f32) -> egui::Color32 {
        let t = (3.0 - left) / 3.0;
        if t > 0.0 {
            lerp_color(WARNING, ERROR, t)
        } else {
            WARNING
        }
    }

    /// 画一个圆形进度（弧线折线逼近）
    fn draw_arc(
        painter: &egui::Painter,
        center: egui::Pos2,
        radius: f32,
        start_ang: f32,
        frac: f32,
        stroke: egui::Stroke,
    ) {
        let end_ang = start_ang + frac * std::f32::consts::TAU;
        let segments = 48;
        let pts: Vec<egui::Pos2> = (0..=segments)
            .map(|i| {
                let t = i as f32 / segments as f32;
                let a = start_ang + t * (end_ang - start_ang);
                center + radius * egui::vec2(a.cos(), a.sin())
            })
            .collect();
        painter.add(egui::Shape::line(pts, stroke));
    }

    /// 注入中：无限旋转的 spinner（Material 风格）
    fn draw_spinner(&self, ui: &mut egui::Ui) {
        let size = 64.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
        let center = rect.center();
        let radius = size / 2.0 - 8.0;
        let painter = ui.painter();
        // 底轨
        painter.circle_stroke(center, radius, egui::Stroke::new(6.0, TRACK));
        // 旋转弧：缺口随时间转动
        let time = ui.input(|i| i.time) as f32;
        let sweep = 2.6; // 弧度（约 150°）
        let start_ang = time * 4.5; // 旋转速度
        Self::draw_arc(painter, center, radius, start_ang, sweep / std::f32::consts::TAU, egui::Stroke::new(6.0, PRIMARY));
        // 中间文字
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            "…",
            egui::FontId::proportional(22.0),
            PRIMARY,
        );
    }

    /// 倒计时：环形进度 + 平滑数字 + 末段变色
    fn draw_countdown(&mut self, ui: &mut egui::Ui, left: f32) {
        ui.horizontal(|ui| {
            let total = self.secs.max(1) as f32;
            let frac = (left / total).clamp(0.0, 1.0);
            let color = self.countdown_color(left);
            let size = 64.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            let center = rect.center();
            let radius = size / 2.0 - 8.0;
            let painter = ui.painter();
            // 底轨
            painter.circle_stroke(center, radius, egui::Stroke::new(6.0, TRACK));
            // 进度弧（从顶部顺时针）
            Self::draw_arc(painter, center, radius, -std::f32::consts::FRAC_PI_2, frac, egui::Stroke::new(6.0, color));
            // 平滑数字（低通滤波，减少跳变）
            self.countdown_display += (left - self.countdown_display) * 0.2;
            let shown = self.countdown_display.ceil().max(0.0) as u64;
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                format!("{}", shown),
                egui::FontId::proportional(26.0),
                color,
            );
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("正在倒计时…").size(15.0).strong().color(color));
                ui.label(
                    egui::RichText::new("请把焦点切到目标窗口！")
                        .size(12.5)
                        .color(TEXT_SECONDARY),
                );
            });
        });
    }
}

impl eframe::App for GuiApp {
    /// eframe 0.36 的入口：UI 绘制前的逻辑回调（每帧 + 窗口隐藏时也调用）
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_inject();

        // 倒计时推进
        if let (Some(end), Some(_)) = (self.countdown_end, self.countdown_left) {
            let left = end.saturating_duration_since(Instant::now()).as_secs_f32();
            if left <= 0.0 {
                self.spawn_inject();
            } else {
                self.countdown_left = Some(left);
                ctx.request_repaint_after(Duration::from_millis(50)); // 环形进度平滑动画
            }
        }
        // 注入中 spinner 也要持续重绘
        if self.busy {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
    }

    /// eframe 0.36 的绘制入口：直接拿 Ui 画界面
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(4.0);

            // ---------- 顶部：App Bar（Material 风格，蓝底白字）----------
            egui::Frame::new()
                .fill(PRIMARY)
                .corner_radius(16)
                .inner_margin(egui::Margin::symmetric(20, 14))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("TypeText")
                                .size(24.0)
                                .strong()
                                .color(ON_PRIMARY),
                        );
                        ui.label(
                            egui::RichText::new("文本注入器")
                                .size(20.0)
                                .strong()
                                .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 210)),
                        );
                    });
                    ui.label(
                        egui::RichText::new("把文本模拟键盘输入到虚拟机 / 远程桌面")
                            .size(12.5)
                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 170)),
                    );
                });
            ui.add_space(10.0);

            // ---------- 参数卡片 ----------
            card(ui, |ui| {
                ui.label(
                    egui::RichText::new("参数设置")
                        .size(14.0)
                        .strong()
                        .color(TEXT_SECONDARY),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("倒计时");
                    ui.add(
                        egui::DragValue::new(&mut self.secs)
                            .range(0..=99)
                            .suffix(" 秒")
                            .speed(0.2),
                    );
                    ui.separator();
                    ui.label("每键间隔");
                    ui.add(
                        egui::DragValue::new(&mut self.delay_ms)
                            .range(1..=5000)
                            .suffix(" ms")
                            .speed(1.0),
                    );
                    ui.label(
                        egui::RichText::new("（虚拟机丢字就调大间隔）")
                            .size(12.0)
                            .color(TEXT_SECONDARY),
                    );
                });
            });
            ui.add_space(10.0);

            // ---------- 操作按钮行（永远可见，不会被文本区挤出）----------
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new("📄 从文件…")
                            .corner_radius(20)
                            .min_size(egui::vec2(0.0, 40.0)),
                    )
                    .clicked()
                {
                    self.load_from_file();
                }
                if ui
                    .add(
                        egui::Button::new("🔍 预览")
                            .corner_radius(20)
                            .min_size(egui::vec2(0.0, 40.0)),
                    )
                    .clicked()
                {
                    self.preview = Some(analyze(&self.text, self.delay_ms));
                    self.preview_open = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cancel_btn = ui.add_enabled(
                        self.countdown_left.is_some() && !self.busy,
                        egui::Button::new("取消")
                            .corner_radius(20)
                            .min_size(egui::vec2(90.0, 40.0)),
                    );
                    if cancel_btn.clicked() {
                        self.cancel();
                    }
                    // 主按钮：胶囊形填充蓝，hover 变深（Material 交互反馈）
                    let start_btn = ui.add_enabled(
                        !self.busy,
                        egui::Button::new(
                            egui::RichText::new("开始输入")
                                .size(15.0)
                                .strong()
                                .color(ON_PRIMARY),
                        )
                        .fill(if self.start_hovered { PRIMARY_DARK } else { PRIMARY })
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(26)
                        .min_size(egui::vec2(150.0, 44.0)),
                    );
                    self.start_hovered = start_btn.hovered() || start_btn.is_pointer_button_down_on();
                    if start_btn.clicked() {
                        self.start();
                    }
                });
            });
            ui.add_space(10.0);

            // ---------- 文本卡片（弹性，占满剩余空间）----------
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("要注入的文本")
                            .size(14.0)
                            .strong()
                            .color(TEXT_SECONDARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} 字符 / {} 行",
                                self.text.chars().count(),
                                self.text.lines().count()
                            ))
                            .size(12.5)
                            .color(TEXT_SECONDARY),
                        );
                    });
                });
                ui.add_space(6.0);
                // 文本区填满卡片剩余空间（预留底部状态区高度；错误状态信息更长，多预留）
                let avail = ui.available_size();
                let status_h = if self.status_error {
                    78.0
                } else if self.busy || self.countdown_left.is_some() {
                    84.0
                } else {
                    34.0
                };
                ui.add_sized(
                    egui::vec2(avail.x, (avail.y - status_h).max(70.0)),
                    egui::TextEdit::multiline(&mut self.text)
                        .font(egui::TextStyle::Monospace)
                        .margin(egui::Margin::same(10)),
                );
                ui.add_space(6.0);

                // ---------- 底部状态区（卡片内固定，永远可见）----------
                ui.horizontal(|ui| {
                    if self.busy {
                        self.draw_spinner(ui);
                    } else if let Some(left) = self.countdown_left {
                        self.draw_countdown(ui, left);
                    }
                });
                // 状态点 + 文字（wrap 自动换行，长报错不再被截断）
                ui.horizontal(|ui| {
                    let (dot_rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 4.5, self.status_color());
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&self.status)
                                .size(13.0)
                                .color(self.status_color()),
                        )
                        .wrap(),
                    );
                });
            });

            // ---------- 预览弹窗 ----------
            if self.preview_open {
                let mut open = self.preview_open;
                egui::Window::new("🔍 预览解析结果")
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(true)
                    .default_width(380.0)
                    .show(ui.ctx(), |ui| {
                        if let Some(p) = &self.preview {
                            egui::Grid::new("preview_grid")
                                .num_columns(2)
                                .spacing([24.0, 8.0])
                                .show(ui, |ui| {
                                    ui.label("字符数");
                                    ui.label(egui::RichText::new(p.chars.to_string()).strong());
                                    ui.end_row();
                                    ui.label("行数");
                                    ui.label(egui::RichText::new(p.lines.to_string()).strong());
                                    ui.end_row();
                                    ui.label("将注入键数");
                                    ui.label(egui::RichText::new(p.keys.to_string()).strong());
                                    ui.end_row();
                                    ui.label("预计耗时");
                                    ui.label(
                                        egui::RichText::new(format!("约 {:.0} 秒", p.est_secs)).strong(),
                                    );
                                    ui.end_row();
                                    ui.label("跳过字符");
                                    if p.skipped_count > 0 {
                                        let sample: String = p.skipped.iter().collect();
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{} 个：{}",
                                                p.skipped_count,
                                                if sample.is_empty() { "(略)" } else { &sample }
                                            ))
                                            .color(ERROR),
                                        );
                                    } else {
                                        ui.label(egui::RichText::new("无").color(SUCCESS));
                                    }
                                    ui.end_row();
                                });
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("仅支持英文/数字/标点；中文等非 ASCII 会被跳过。")
                                    .size(12.0)
                                    .color(TEXT_SECONDARY),
                            );
                        } else {
                            ui.label("文本为空，先输入内容再预览。");
                        }
                    });
                self.preview_open = open;
            }
        });
    }
}
