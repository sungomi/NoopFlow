use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::protocol::{Cell, Layout, Os};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    /// Mac: tapping right ⌘ alone toggles 한/영 (locally, and sends 한/영 when controlling another computer).
    pub right_cmd_hangul: bool,
    /// Mac: the Windows 한/영 key (LANG1) toggles the Mac input source.
    pub lang1_hangul: bool,
    /// Mac: tapping right Alt/Option alone (from a remote keyboard) toggles 한/영.
    pub right_alt_hangul: bool,
    /// How the Mac switches input source: "direct" (TIS API) or "ctrl_space" (sends ⌃Space).
    pub hangul_method: String,
    /// Modifier layout when the keyboard comes from the other OS:
    /// "position" (Alt↔⌘, Win↔⌥), "ctrl_cmd" (Ctrl↔⌘) or "none".
    pub key_layout: String,
    pub clipboard_sync: bool,
    /// Ctrl/⌘+V on another computer sends files you copied here.
    pub paste_files: bool,
    pub invert_scroll: bool,
    pub mouse_speed: f32,
    pub receive_dir: Option<String>,
    /// Ignore screen edges this close (px) to a corner.
    pub corner_guard: i32,
    /// Mac: map plain F1/F2/F7..F12 from another keyboard to Mac brightness/media controls.
    pub mac_fn_media: bool,
    /// Windows: what the window's close button does: "tray" (default) or "quit".
    /// Older configs may hold "ask"; it is read as "tray".
    pub close_action: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            right_cmd_hangul: true,
            lang1_hangul: true,
            right_alt_hangul: false,
            hangul_method: "direct".into(),
            key_layout: "position".into(),
            clipboard_sync: true,
            paste_files: true,
            invert_scroll: false,
            mouse_speed: 1.0,
            receive_dir: None,
            corner_guard: 6,
            mac_fn_media: false,
            close_action: "tray".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TrustedPeer {
    pub id: String,
    pub name: String,
    pub os: Os,
    pub public_key: String,
    #[serde(default)]
    pub last_addr: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub id: String,
    pub name: String,
    pub private_key: String,
    pub public_key: String,
    #[serde(default)]
    pub trusted: Vec<TrustedPeer>,
    #[serde(default)]
    pub layout: Layout,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub locked: bool,
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

pub fn default_device_name() -> String {
    if cfg!(target_os = "macos") {
        if let Ok(out) = std::process::Command::new("scutil").args(["--get", "ComputerName"]).output() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
        return "Mac".into();
    }
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Windows PC".into())
}

impl Config {
    fn fresh() -> Config {
        let kp = snow::Builder::new(crate::net::NOISE_XX.parse().unwrap())
            .generate_keypair()
            .expect("keypair");
        let id = hex(&rand::random::<[u8; 8]>());
        Config {
            layout: Layout { version: 1, cells: vec![Cell { id: id.clone(), x: 0, y: 0 }] },
            id,
            name: default_device_name(),
            private_key: hex(&kp.private),
            public_key: hex(&kp.public),
            trusted: vec![],
            settings: Settings::default(),
            locked: false,
        }
    }

    pub fn load_or_create(dir: &Path) -> Config {
        let path = dir.join("config.json");
        let backup = dir.join("config.json.bak");
        let load = |p: &Path| {
            std::fs::read_to_string(p)
                .ok()
                .and_then(|s| serde_json::from_str::<Config>(&s).ok())
        };
        let mut cfg = load(&path).or_else(|| load(&backup)).unwrap_or_else(|| {
            let c = Config::fresh();
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(&path, serde_json::to_string_pretty(&c).unwrap());
            c
        });
        // The close button used to ask by default; it now minimizes to the tray.
        if cfg.settings.close_action != "quit" {
            cfg.settings.close_action = "tray".into();
        }
        if cfg.layout.cell(&cfg.id).is_none() {
            let x = cfg.layout.cells.iter().map(|c| c.x).min().unwrap_or(1) - 1;
            cfg.layout.cells.push(Cell { id: cfg.id.clone(), x, y: 0 });
        }
        cfg
    }

    pub fn save(&self, dir: &Path) {
        let _ = std::fs::create_dir_all(dir);
        let json = serde_json::to_string_pretty(self).unwrap();
        let path = dir.join("config.json");
        let tmp = dir.join("config.json.tmp");
        let backup = dir.join("config.json.bak");
        if path.exists() {
            let _ = std::fs::copy(&path, &backup);
        }
        if std::fs::write(&tmp, &json).is_ok() {
            if std::fs::rename(&tmp, &path).is_err() {
                let _ = std::fs::write(&path, &json);
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }

    pub fn receive_dir(&self) -> PathBuf {
        if let Some(d) = &self.settings.receive_dir {
            if !d.is_empty() {
                return PathBuf::from(d);
            }
        }
        dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("NoopFlow")
    }

    pub fn trusted_by_key(&self, key: &[u8]) -> Option<&TrustedPeer> {
        let k = hex(key);
        self.trusted.iter().find(|t| t.public_key == k)
    }
}
