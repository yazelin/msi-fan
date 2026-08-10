#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auto;
mod ec;

#[tauri::command]
fn status() -> ec::Status {
    ec::status()
}

#[tauri::command]
fn set(key: String, value: String) -> Result<(), String> {
    ec::set(&key, &value)?;
    // 記住使用者選的效能模式，開機時由守護行程套回去。
    // EC 會不會自己保留這個設定沒查證過，記在自己這邊才不必賭。
    if key == "shift_mode" {
        let mut c = auto::load();
        c.shift_mode = value;
        let _ = auto::save(&c);
    }
    Ok(())
}

#[tauri::command]
fn get_config() -> auto::Config {
    auto::load()
}

/// 存設定，並讓「自動控制」開關真的去開關 systemd 服務。
/// 先存檔再動服務：服務一啟動就會讀設定，順序反了會讓它先跑一圈舊值。
#[tauri::command]
fn set_config(cfg: auto::Config) -> Result<(), String> {
    let want = cfg.enabled;
    auto::save(&cfg)?;
    if auto::daemon_active() != want {
        auto::set_daemon(want)?;
    }
    Ok(())
}

/// 服務是不是真的在跑。UI 用它來拆穿「設定檔說開著、其實沒人在跑」這種狀態。
#[tauri::command]
fn auto_running() -> bool {
    auto::daemon_active()
}

fn main() {
    // 同一支執行檔兼作背景守護：systemd 服務用 --daemon 進來。
    // 分成兩支執行檔會讓「GUI 和守護跑不同版本」變成可能，那種 bug 很難查。
    if std::env::args().any(|a| a == "--daemon") {
        auto::daemon();
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            status,
            set,
            get_config,
            set_config,
            auto_running
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 啟動失敗");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 讀得到狀態且欄位合理() {
        let s = ec::status();
        assert!(s.available, "找不到 msi-ec，模組沒載入？");
        assert!(!s.fw_version.is_empty(), "讀不到韌體版本");
        assert!(s.cpu_temp > 0 && s.cpu_temp < 120, "CPU 溫度不合理：{}", s.cpu_temp);
        assert!(s.shift_modes.contains(&"turbo".to_string()), "模式清單裡沒有 turbo");
    }

    #[test]
    fn 擋掉不合法的值() {
        assert!(ec::set("shift_mode", "banana").is_err(), "應該擋下不支援的模式");
        assert!(ec::set("不存在的鍵", "on").is_err(), "應該擋下未知的控制項");
    }

    #[test]
    fn 門檻寫反時要擋下來() {
        let mut c = auto::Config::default();
        c.off_below = c.on_above + 1;          // 關閉門檻高於開啟門檻＝會反覆開關
        assert!(auto::save(&c).is_err(), "門檻寫反卻沒被擋下");
    }
}
