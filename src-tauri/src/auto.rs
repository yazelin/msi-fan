//! 自動控制。
//!
//! 硬體只給 cooler_boost 這個開關，沒有連續調速，所以「自動」= 依溫度開關 Boost。
//!
//! 遲滯（hysteresis）是必要的：只用單一門檻的話，溫度在門檻上下抖動時
//! 會讓風扇每秒開開關關，比不做還糟。所以分成上下兩個門檻，
//! 而且要「持續超過 hold 秒」才動作。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    /// 自動控制總開關
    pub enabled: bool,
    /// 超過這個溫度就開 Boost
    pub on_above: i32,
    /// 降到這個溫度以下才關 Boost（必須低於 on_above，否則會抖動）
    pub off_below: i32,
    /// 要持續幾秒才動作，避免瞬間尖峰觸發
    pub hold_secs: u32,
    /// 自動控制期間維持哪個效能模式；空字串＝不動它
    pub shift_mode: String,
}

impl Default for Config {
    fn default() -> Self {
        // 預設值的由來：實測 comfort 模式下重載會到 95–97°C，turbo 降到 66–71°C。
        // 所以日常掛 turbo 就夠，Boost 留給真的壓不住的時候。
        Self {
            enabled: false,
            on_above: 85,
            off_below: 72,
            hold_secs: 10,
            shift_mode: "turbo".into(),
        }
    }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
        });
    base.join("msi-fan").join("config.json")
}

pub fn load() -> Config {
    fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

const UNIT: &str = "msi-fan-auto";

/// 服務現在真的在跑嗎。
/// 這一格不能用設定檔的 `enabled` 代替：實測踩過一次，設定檔寫著 enabled=true、
/// 服務其實是 inactive，介面顯示「自動控制：開」，結果 CPU 在 95°C 待了 45 秒
/// 都沒有人去開 Boost。開關要對應真實狀態，不能對應「我曾經按過」。
pub fn daemon_active() -> bool {
    std::process::Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", UNIT])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 讓「自動控制」開關真的生效：寫設定檔之外，也要開關 systemd 服務。
/// enable/disable 帶 --now，這樣「下次開機」跟「現在」一起處理掉。
pub fn set_daemon(on: bool) -> Result<(), String> {
    let verb = if on { "enable" } else { "disable" };
    let out = std::process::Command::new("systemctl")
        .args(["--user", verb, "--now", UNIT])
        .output()
        .map_err(|e| format!("叫不動 systemctl：{e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "{verb} {UNIT} 失敗：{}",
        String::from_utf8_lossy(&out.stderr).trim()
    ))
}

pub fn save(c: &Config) -> Result<(), String> {
    if c.off_below >= c.on_above {
        return Err("關閉門檻必須低於開啟門檻，否則會在門檻附近反覆開關".into());
    }
    let p = config_path();
    if let Some(d) = p.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    fs::write(&p, serde_json::to_string_pretty(c).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

/// 背景守護迴圈。用 `msi-fan --daemon` 進來，由 systemd 使用者服務常駐。
/// 做成獨立行程而不是塞在 GUI 裡，是因為「自動」不該只在視窗開著時有效。
pub fn daemon() -> ! {
    let mut over = 0u32; // 連續超過上門檻的秒數
    let mut under = 0u32; // 連續低於下門檻的秒數
    let mut applied_shift = false;

    loop {
        let cfg = load(); // 每圈重讀，GUI 改設定後不必重啟服務
        let s = crate::ec::status();

        // 效能模式跟自動 Boost 是兩件事，不綁在一起：
        // 「開機就套用 turbo」不該被迫連自動 Boost 一起開。
        // 只在啟動時套一次，之後使用者從 GUI 手動改就尊重他的選擇。
        if s.available && s.writable && !applied_shift && !cfg.shift_mode.is_empty() {
            if s.shift_mode != cfg.shift_mode {
                let _ = crate::ec::set("shift_mode", &cfg.shift_mode);
            }
            applied_shift = true;
        }

        if cfg.enabled && s.available && s.writable {
            let t = s.cpu_temp.max(s.gpu_temp);
            if t >= cfg.on_above {
                over += 1;
                under = 0;
            } else if t <= cfg.off_below {
                under += 1;
                over = 0;
            } else {
                // 兩個門檻之間＝維持現狀，這段就是遲滯區
                over = 0;
                under = 0;
            }
            if over >= cfg.hold_secs && s.cooler_boost != "on" {
                let _ = crate::ec::set("cooler_boost", "on");
            }
            if under >= cfg.hold_secs && s.cooler_boost != "off" {
                let _ = crate::ec::set("cooler_boost", "off");
            }
        } else {
            over = 0;
            under = 0;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
