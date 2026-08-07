//! 跟 msi-ec 這支核心模組打交道的那一層。
//!
//! 硬體只給三個開關，沒有「指定轉速」也沒有「自訂曲線」——
//! 這台（Cyborg 14 A13VF / EC 14P1IMS1.106）的 msi-ec 設定檔沒有暴露曲線控制點。
//! 所以「自動控制」只能用 cooler_boost 這個開關做，不是連續調速。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const EC: &str = "/sys/devices/platform/msi-ec";
const HWMON: &str = "/sys/class/hwmon";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Status {
    pub available: bool,
    pub writable: bool,
    pub fw_version: String,
    pub shift_mode: String,
    pub fan_mode: String,
    pub cooler_boost: String,
    pub cpu_temp: i32,
    pub gpu_temp: i32,
    pub cpu_fan_pct: i32,
    pub gpu_fan_pct: i32,
    pub fan_rpm: i32,
    pub shift_modes: Vec<String>,
    pub fan_modes: Vec<String>,
}

fn read(rel: &str) -> String {
    fs::read_to_string(format!("{EC}/{rel}"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn read_i32(rel: &str) -> i32 {
    read(rel).parse().unwrap_or(-1)
}

fn read_list(rel: &str) -> Vec<String> {
    read(rel)
        .split_whitespace()
        .map(|s| s.to_string())
        .collect()
}

/// 風扇轉速在 msi-ec 裡只有百分比，實際 RPM 要去 msi_wmi_platform 那支 hwmon 拿。
fn fan_rpm() -> i32 {
    let Ok(dir) = fs::read_dir(HWMON) else { return -1 };
    for e in dir.flatten() {
        let p = e.path();
        let name = fs::read_to_string(p.join("name")).unwrap_or_default();
        if name.trim() != "msi_wmi_platform" {
            continue;
        }
        // fan1 是唯一讀得到值的；fan2–4 恆為 0，那是驅動的通用欄位不是實體風扇
        if let Ok(v) = fs::read_to_string(p.join("fan1_input")) {
            return v.trim().parse().unwrap_or(-1);
        }
    }
    -1
}

/// 能不能寫，決定 UI 要不要把控制項變灰並提示安裝步驟。
/// 用實際試寫判斷，不猜權限位元——群組加了但沒重新登入時，
/// 檔案看起來可寫但這個 process 其實沒有那個群組。
fn writable() -> bool {
    let p = format!("{EC}/shift_mode");
    let Ok(cur) = fs::read_to_string(&p) else { return false };
    fs::write(&p, cur.trim()).is_ok()
}

pub fn status() -> Status {
    let available = Path::new(EC).exists();
    if !available {
        return Status {
            available: false,
            writable: false,
            fw_version: String::new(),
            shift_mode: String::new(),
            fan_mode: String::new(),
            cooler_boost: String::new(),
            cpu_temp: -1,
            gpu_temp: -1,
            cpu_fan_pct: -1,
            gpu_fan_pct: -1,
            fan_rpm: -1,
            shift_modes: vec![],
            fan_modes: vec![],
        };
    }
    Status {
        available: true,
        writable: writable(),
        fw_version: read("fw_version"),
        shift_mode: read("shift_mode"),
        fan_mode: read("fan_mode"),
        cooler_boost: read("cooler_boost"),
        cpu_temp: read_i32("cpu/realtime_temperature"),
        gpu_temp: read_i32("gpu/realtime_temperature"),
        cpu_fan_pct: read_i32("cpu/realtime_fan_speed"),
        gpu_fan_pct: read_i32("gpu/realtime_fan_speed"),
        fan_rpm: fan_rpm(),
        shift_modes: read_list("available_shift_modes"),
        fan_modes: read_list("available_fan_modes"),
    }
}

/// 只允許寫入該檔案自己宣告支援的值。
/// 這層擋的是「UI 傳了奇怪的字串進來就直接寫進 EC」——寫 EC 出錯的代價比擋下來高。
pub fn set(key: &str, value: &str) -> Result<(), String> {
    let allowed: Vec<String> = match key {
        "shift_mode" => read_list("available_shift_modes"),
        "fan_mode" => read_list("available_fan_modes"),
        "cooler_boost" => vec!["on".into(), "off".into()],
        _ => return Err(format!("不認識的控制項：{key}")),
    };
    if !allowed.iter().any(|a| a == value) {
        return Err(format!("{key} 不接受「{value}」，可用的是 {allowed:?}"));
    }
    fs::write(format!("{EC}/{key}"), value)
        .map_err(|e| format!("寫入 {key} 失敗：{e}。多半是權限——見 README 的安裝步驟"))
}
