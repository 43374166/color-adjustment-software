//! fdxmp 单文件 UI 打包器（不拆分模块）。
//!
//! 功能（阶段 6 完整实现，本文件已具备骨架）：
//! 1. 启动时自动检测各平台打包工具
//! 2. 每个平台一张卡片，绿 ✓ / 红 ✗ / 黄 ⚠
//! 3. 点击按钮执行对应打包命令
//! 4. 实时日志输出到界面底部
//! 5. 输出统一到根目录 `dist/{platform}/`
//! 6. iOS 按钮禁用，显示「暂不支持」

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver};

use eframe::egui;
use serde::Serialize;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 680.0])
            .with_min_inner_size([760.0, 520.0])
            .with_title("fdxmp 打包器"),
        ..Default::default()
    };
    eframe::run_native(
        "fdxmp-packer",
        options,
        Box::new(|cc| {
            install_cjk_fonts(&cc.egui_ctx);
            Ok(Box::<PackerApp>::default())
        }),
    )
}

/// 加载系统中文字体（egui 默认字体不含 CJK；详见 crates/win_ui 中的同名函数）。
fn install_cjk_fonts(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\Deng.ttf",
        "/System/Library/Fonts/PingFang.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    ];

    let path = std::env::var_os("FDXMP_UI_FONT")
        .map(std::path::PathBuf::from)
        .into_iter()
        .chain(CANDIDATES.iter().map(std::path::PathBuf::from))
        .find(|candidate| candidate.is_file());

    let Some(path) = path else {
        eprintln!(
            "警告：未找到中文字体，界面中文可能显示为方框（可用 FDXMP_UI_FONT 指定字体文件）"
        );
        return;
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "fdxmp-cjk".to_owned(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        list.insert(0, "fdxmp-cjk".to_owned());
    }
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        list.push("fdxmp-cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    Windows,
    Android,
    HarmonyOs,
    Ios,
}

impl Platform {
    const ALL: [Platform; 4] = [
        Platform::Windows,
        Platform::Android,
        Platform::HarmonyOs,
        Platform::Ios,
    ];

    fn title(self) -> &'static str {
        match self {
            Platform::Windows => "Windows",
            Platform::Android => "Android",
            Platform::HarmonyOs => "HarmonyOS",
            Platform::Ios => "iOS",
        }
    }

    fn script(self) -> Option<&'static str> {
        match self {
            Platform::Windows => Some("packaging/windows/build.ps1"),
            Platform::Android => Some("packaging/android/build.ps1"),
            Platform::HarmonyOs => Some("packaging/harmony/build.ps1"),
            Platform::Ios => None,
        }
    }

    fn dist_dir(self) -> &'static str {
        match self {
            Platform::Windows => "dist/windows",
            Platform::Android => "dist/android",
            Platform::HarmonyOs => "dist/harmony",
            Platform::Ios => "dist/ios",
        }
    }

    fn command_hint(self) -> &'static str {
        match self {
            Platform::Windows => "cargo build --release -p fdxmp-win-ui + makensis + 7z",
            Platform::Android => "cargo ndk + gradle assembleRelease",
            Platform::HarmonyOs => "ohos-rs build + hvigor assembleHap",
            Platform::Ios => "暂不支持",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum Status {
    Ok,
    Missing,
    Warn,
    Disabled,
}

impl Status {
    fn color(self) -> egui::Color32 {
        match self {
            Status::Ok => egui::Color32::from_rgb(0x3D, 0xD5, 0x6A),
            Status::Missing => egui::Color32::from_rgb(0xE5, 0x4B, 0x4B),
            Status::Warn => egui::Color32::from_rgb(0xE5, 0xB5, 0x2F),
            Status::Disabled => egui::Color32::from_rgb(0x8A, 0x8A, 0x8A),
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Status::Ok => "\u{2713}",
            Status::Missing => "\u{2717}",
            Status::Warn => "\u{26A0}",
            Status::Disabled => "\u{2013}",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct CheckItem {
    name: String,
    status: Status,
    detail: String,
}

impl CheckItem {
    fn new(name: impl Into<String>, status: Status, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct Card {
    platform: String,
    checks: Vec<CheckItem>,
}

impl Card {
    fn summary(&self) -> Status {
        if self.checks.iter().any(|c| c.status == Status::Missing) {
            Status::Missing
        } else if self.checks.iter().any(|c| c.status == Status::Warn) {
            Status::Warn
        } else if self.checks.iter().all(|c| c.status == Status::Disabled) {
            Status::Disabled
        } else {
            Status::Ok
        }
    }
}

fn which_any(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| which::which(name).ok())
        .map(|path| path.display().to_string())
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

fn detect() -> Vec<Card> {
    let root = workspace_root();
    let mut cards = Vec::new();

    // ---- Windows ----
    let mut windows = Vec::new();
    for (name, tools) in [("cargo", vec!["cargo"]), ("rustup", vec!["rustup"])] {
        windows.push(match which_any(&tools) {
            Some(path) => CheckItem::new(name, Status::Ok, path),
            None => CheckItem::new(name, Status::Missing, "未在 PATH 中找到"),
        });
    }
    for (name, tools) in [
        ("makensis", vec!["makensis", "makensis.exe"]),
        ("7z", vec!["7z", "7z.exe", "7za"]),
    ] {
        windows.push(match which_any(&tools) {
            Some(path) => CheckItem::new(name, Status::Ok, path),
            None => CheckItem::new(name, Status::Warn, "未检测到（安装 NSIS / 7-Zip 后可打包）"),
        });
    }
    cards.push(Card {
        platform: Platform::Windows.title().to_string(),
        checks: windows,
    });

    // ---- Android ----
    let mut android = Vec::new();
    android.push(match which_any(&["cargo-ndk"]) {
        Some(path) => CheckItem::new("cargo-ndk", Status::Ok, path),
        None => CheckItem::new("cargo-ndk", Status::Missing, "cargo install cargo-ndk"),
    });
    let sdk = env_dir("ANDROID_SDK_ROOT")
        .or_else(|| env_dir("ANDROID_HOME"))
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(|local| PathBuf::from(local).join("Android").join("Sdk"))
                .filter(|path| path.exists())
        });
    android.push(match &sdk {
        Some(path) => CheckItem::new("Android SDK", Status::Ok, path.display().to_string()),
        None => CheckItem::new(
            "Android SDK",
            Status::Missing,
            "未找到 ANDROID_SDK_ROOT / ANDROID_HOME",
        ),
    });
    let ndk = env_dir("ANDROID_NDK_HOME").or_else(|| {
        sdk.as_ref().and_then(|sdk| {
            let ndk_dir = sdk.join("ndk");
            std::fs::read_dir(ndk_dir)
                .ok()
                .and_then(|mut entries| entries.next())
                .and_then(|entry| entry.ok())
                .map(|entry| entry.path())
        })
    });
    android.push(match &ndk {
        Some(path) => CheckItem::new("NDK", Status::Ok, path.display().to_string()),
        None => CheckItem::new(
            "NDK",
            Status::Missing,
            "设置 ANDROID_NDK_HOME 或在 SDK 下安装 NDK",
        ),
    });
    let gradle = which_any(&["gradle"]).or_else(|| {
        let wrapper = root.join("platforms").join("android").join("gradlew.bat");
        wrapper.exists().then(|| wrapper.display().to_string())
    });
    android.push(match gradle {
        Some(path) => CheckItem::new("Gradle", Status::Ok, path),
        None => CheckItem::new("Gradle", Status::Warn, "阶段 5 生成 gradlew 后可用"),
    });
    android.push(match which_any(&["java"]) {
        Some(path) => CheckItem::new("Java", Status::Ok, path),
        None => CheckItem::new("Java", Status::Missing, "需要 JDK 17+"),
    });
    cards.push(Card {
        platform: Platform::Android.title().to_string(),
        checks: android,
    });

    // ---- HarmonyOS ----
    let mut harmony = Vec::new();
    harmony.push(match which_any(&["ohos-rs"]) {
        Some(path) => CheckItem::new("ohos-rs", Status::Ok, path),
        None => CheckItem::new(
            "ohos-rs",
            Status::Missing,
            "cargo install cargo-ohos / 使用 ohos-rs 工具链",
        ),
    });
    let deveco = env_dir("DEVECO_SDK_HOME")
        .or_else(|| env_dir("OHOS_SDK_HOME"))
        .or_else(|| {
            let typical = PathBuf::from(r"C:\Program Files\Huawei\DevEco Studio");
            typical.exists().then_some(typical)
        });
    harmony.push(match &deveco {
        Some(path) => CheckItem::new("DevEco / OHOS SDK", Status::Ok, path.display().to_string()),
        None => CheckItem::new(
            "DevEco / OHOS SDK",
            Status::Missing,
            "安装 DevEco Studio 并设置 DEVECO_SDK_HOME",
        ),
    });
    harmony.push(match which_any(&["hvigorw", "hvigorw.bat"]) {
        Some(path) => CheckItem::new("hvigor", Status::Ok, path),
        None => CheckItem::new(
            "hvigor",
            Status::Missing,
            "DevEco 自带 hvigorw，需要加入 PATH",
        ),
    });
    cards.push(Card {
        platform: Platform::HarmonyOs.title().to_string(),
        checks: harmony,
    });

    // ---- iOS ----
    cards.push(Card {
        platform: Platform::Ios.title().to_string(),
        checks: vec![CheckItem::new(
            "打包",
            Status::Disabled,
            "占位：先写代码，不打包",
        )],
    });

    cards
}

struct PackerApp {
    cards: Vec<Card>,
    logs: Vec<String>,
    receiver: Option<Receiver<String>>,
    running: Option<Platform>,
    output_dir: PathBuf,
}

impl Default for PackerApp {
    fn default() -> Self {
        let output_dir = workspace_root().join("dist");
        Self {
            cards: detect(),
            logs: vec!["fdxmp 打包器已启动；点击「重新检测」刷新环境。".to_string()],
            receiver: None,
            running: None,
            output_dir,
        }
    }
}

impl PackerApp {
    fn start_build(&mut self, platform: Platform, ctx: &egui::Context) {
        let Some(script_rel) = platform.script() else {
            self.logs.push("iOS：暂不支持".to_string());
            return;
        };
        let script = workspace_root().join(script_rel);
        if !script.exists() {
            self.logs
                .push(format!("脚本不存在：{}（阶段 6 补齐）", script.display()));
            return;
        }
        self.running = Some(platform);
        self.logs.push(format!("▶ {}", platform.command_hint()));

        let (sender, receiver) = channel::<String>();
        let script_for_thread = script.clone();
        std::thread::spawn(move || {
            let output = Command::new("powershell")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(&script_for_thread)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output();
            match output {
                Ok(output) => {
                    for line in String::from_utf8_lossy(&output.stdout).lines() {
                        let _ = sender.send(line.to_string());
                    }
                    for line in String::from_utf8_lossy(&output.stderr).lines() {
                        let _ = sender.send(format!("[stderr] {line}"));
                    }
                    let _ = sender.send(format!(
                        "■ 结束，退出码 {}",
                        output.status.code().unwrap_or(-1)
                    ));
                }
                Err(err) => {
                    let _ = sender.send(format!("[错误] 启动失败：{err}"));
                }
            }
        });
        self.receiver = Some(receiver);
        ctx.request_repaint();
    }

    fn poll_logs(&mut self, ctx: &egui::Context) {
        let mut finished = false;
        if let Some(receiver) = &self.receiver {
            while let Ok(line) = receiver.try_recv() {
                if line.starts_with('■') {
                    finished = true;
                }
                self.logs.push(line);
            }
        }
        if finished {
            self.receiver = None;
            self.running = None;
        }
        if self.receiver.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}

impl eframe::App for PackerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_logs(&ctx);

        egui::Panel::top("header").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("fdxmp 打包器");
                if ui.button("重新检测").clicked() {
                    self.cards = detect();
                    self.logs.push("已重新检测打包环境".to_string());
                }
                if ui.button("选择输出目录").clicked() {
                    if let Some(dir) = rfd::FileDialog::new()
                        .set_directory(workspace_root())
                        .pick_folder()
                    {
                        self.output_dir = dir;
                    }
                }
                ui.label(format!("输出：{}", self.output_dir.display()));
            });
        });

        egui::Panel::bottom("logs")
            .resizable(true)
            .default_size(200.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("日志");
                    if ui.button("清空").clicked() {
                        self.logs.clear();
                    }
                    if ui.button("打开 dist").clicked() {
                        let dist = workspace_root().join("dist");
                        let _ = Command::new("explorer").arg(dist).spawn();
                    }
                });
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &self.logs {
                            ui.monospace(line);
                        }
                    });
            });

        let mut pending: Option<Platform> = None;
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for card in &self.cards {
                    let platform = Platform::ALL
                        .iter()
                        .copied()
                        .find(|p| p.title() == card.platform.as_str());
                    let frame = egui::Frame::group(ui.style()).inner_margin(12.0);
                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.heading(card.platform.as_str());
                            let summary = card.summary();
                            ui.colored_label(
                                summary.color(),
                                format!("{} {:?}", summary.symbol(), summary),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if let Some(platform) = platform {
                                        let enabled =
                                            platform != Platform::Ios && self.running.is_none();
                                        let label = if platform == Platform::Ios {
                                            "暂不支持"
                                        } else {
                                            "打包"
                                        };
                                        if ui
                                            .add_enabled(enabled, egui::Button::new(label))
                                            .clicked()
                                        {
                                            pending = Some(platform);
                                        }
                                    }
                                },
                            );
                        });
                        ui.label(
                            egui::RichText::new(card.command_hint_placeholder())
                                .small()
                                .weak(),
                        );
                        if let Some(platform) = platform {
                            ui.label(
                                egui::RichText::new(format!("输出：{}", platform.dist_dir()))
                                    .small()
                                    .weak(),
                            );
                        }
                        ui.separator();
                        for check in &card.checks {
                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    check.status.color(),
                                    format!("{} {}", check.status.symbol(), check.name),
                                );
                                ui.label(egui::RichText::new(&check.detail).small().weak());
                            });
                        }
                    });
                    ui.add_space(8.0);
                }
            });
        });
        if let Some(platform) = pending {
            self.start_build(platform, &ctx);
        }
    }
}

trait CardExt {
    fn command_hint_placeholder(&self) -> String;
}

impl CardExt for Card {
    fn command_hint_placeholder(&self) -> String {
        match self.platform.as_str() {
            "Windows" => Platform::Windows.command_hint().to_string(),
            "Android" => Platform::Android.command_hint().to_string(),
            "HarmonyOS" => Platform::HarmonyOs.command_hint().to_string(),
            _ => Platform::Ios.command_hint().to_string(),
        }
    }
}
