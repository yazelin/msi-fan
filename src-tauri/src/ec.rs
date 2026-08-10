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
    /// `cpu|gpu/realtime_fan_speed`，來自 EC 0x71 / 0x89。
    ///
    /// **上游文件宣稱這是百分比**：msi-ec 的 README 寫 "reports the current cpu fan speed,
    /// valid values 0 - 100 or 0 - 150 (percent)"。但在這台（14P1IMS1.106）上對不起來，
    /// 所以這裡不叫它 percent，也不要拿去畫進度條。
    ///
    /// 2026-08-10 實測，同一段時間內 RPM 差 2.4 倍，這兩個值完全沒動：
    ///
    /// | 狀態              | 讀數 | 實際 RPM |
    /// |-------------------|------|----------|
    /// | turbo + Boost 開  | 50   | 6233     |
    /// | turbo + Boost 關  | 50   | 2944     |
    /// | eco   + Boost 關  | 50   | 2651     |
    ///
    /// 唯一會讓它變的是溫度，而且是跳階的：67°C 以下讀 50、68°C 讀 60、75°C 讀 70。
    /// 就算按 0-150 那個刻度換算也對不上（50/150 = 33%，當下風扇卻在全速）。
    /// 驅動那邊也沒有做任何換算，`cpu_realtime_fan_speed_show()` 就是
    /// `ec_read(0x71)` 之後直接 `sysfs_emit("%i")`。
    ///
    /// 所以它比較像「風扇曲線在當前溫度對應的那一階」，不是風扇現在的出力。
    /// 要看出力只有 fan_rpm 可信。這個位址在 msi-ec.c 裡被 21 個機型設定共用，
    /// 所以要嘛這批機型的 EC 語意跟文件不同，要嘛文件本來就寫得太籠統，還沒問上游。
    pub cpu_fan_step: i32,
    pub gpu_fan_step: i32,
    pub fan_rpm: i32,
    /// NVMe 的 Composite 溫度，硬碟自己用來判斷降速的那一顆（不是最熱的那顆感測器）。
    pub nvme_temp: i32,
    /// 機器忙不忙。溫度高但這個低，代表不是散熱問題。
    pub load1: f64,
    pub cpu_threads: i32,
    /// 獨顯使用率，抓不到就 -1（要靠 nvidia-smi，不保證存在）。
    pub gpu_util: i32,
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

/// 找出 hwmon 底下叫某個名字的那一支，回傳它的目錄。
fn hwmon_dir(want: &str) -> Option<std::path::PathBuf> {
    fs::read_dir(HWMON).ok()?.flatten().find_map(|e| {
        let p = e.path();
        (fs::read_to_string(p.join("name")).unwrap_or_default().trim() == want).then_some(p)
    })
}

/// NVMe 溫度。**一定要用 Composite（temp1）**，那是硬碟自己拿來判斷要不要降速的值。
/// 別用最熱的那顆：Sensor 2 是主控晶片，實測比 Composite 高 15°C，拿它嚇自己沒意義
/// （2026-08-10 SMART 實查：Composite 57°C、警告門檻 77°C、累計超標時間 0 分鐘）。
fn nvme_temp() -> i32 {
    let Some(p) = hwmon_dir("nvme") else { return -1 };
    fs::read_to_string(p.join("temp1_input"))
        .ok()
        .and_then(|v| v.trim().parse::<i32>().ok())
        .map(|m| m / 1000)
        .unwrap_or(-1)
}

/// 執行緒數，load 要跟它比才有意義（這台 i7-13620H 是 10 核 16 緒）。
fn threads() -> i32 {
    std::thread::available_parallelism().map_or(-1, |n| n.get() as i32)
}

/// 一分鐘平均負載。溫度高的時候，先看這個再決定要不要怪散熱。
fn load1() -> f64 {
    fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse().ok())
        .unwrap_or(-1.0)
}

/// 獨顯使用率。要開子行程，所以做成 best-effort：沒有 nvidia-smi、逾時、看不懂都回 -1，
/// 由 UI 顯示 `--`。這一格再有用也不值得讓整個狀態輪詢卡住。
fn gpu_util() -> i32 {
    std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=utilization.gpu", "--format=csv,noheader,nounits"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(-1)
}

/// 風扇轉速。msi-ec 給的是不明刻度的原始值，真正的 RPM 要去 msi_wmi_platform 那支 hwmon 拿。
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
            cpu_fan_step: -1,
            gpu_fan_step: -1,
            fan_rpm: -1,
            nvme_temp: nvme_temp(),
            load1: load1(),
            cpu_threads: threads(),
            gpu_util: -1,
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
        cpu_fan_step: read_i32("cpu/realtime_fan_speed"),
        gpu_fan_step: read_i32("gpu/realtime_fan_speed"),
        fan_rpm: fan_rpm(),
        nvme_temp: nvme_temp(),
        load1: load1(),
        cpu_threads: threads(),
        gpu_util: gpu_util(),
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
