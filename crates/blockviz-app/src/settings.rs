//! What the app remembers between runs (the web keeps these in localStorage):
//! the color scheme chosen, the Tidy arrows switch, the details panel's
//! width, and the folders opened lately.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    /// "light" or "dark" when chosen; following the system otherwise.
    pub scheme: Option<String>,
    pub trunks: bool,
    pub panel_w: Option<f64>,
    /// The changes list shows everything, not only what the view is zoomed into.
    pub unscoped: bool,
    /// Folders opened, most recent first.
    pub recent: Vec<String>,
    /// How large text is drawn, as a factor of its usual size (1.0 is usual).
    pub text_size: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { scheme: None, trunks: false, panel_w: None, unscoped: false, recent: vec![], text_size: 1.0 }
    }
}

/// The text sizes to step through, as factors.
const TEXT_STEPS: [f64; 7] = [0.85, 1.0, 1.15, 1.3, 1.5, 1.75, 2.0];

/// The next size up or down from `now`; between steps it goes to the neighbouring one, and it stops at the ends.
pub fn text_step(now: f64, up: bool) -> f64 {
    let eps = 1e-9;
    let next = if up { TEXT_STEPS.iter().copied().find(|&s| s > now + eps) } else { TEXT_STEPS.iter().rev().copied().find(|&s| s < now - eps) };
    next.unwrap_or(if up { TEXT_STEPS[TEXT_STEPS.len() - 1] } else { TEXT_STEPS[0] })
}

/// How many folders the Open dialog lists.
const RECENT: usize = 8;

fn path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/blockviz/settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    /// Saved as soon as something changes; failing to save only forgets it.
    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(t) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, t);
        }
    }

    pub fn opened(&mut self, folder: &std::path::Path) {
        let f = folder.to_string_lossy().to_string();
        self.recent.retain(|r| *r != f);
        self.recent.insert(0, f);
        self.recent.truncate(RECENT);
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_up_and_down() {
        assert_eq!(text_step(1.0, true), 1.15);
        assert_eq!(text_step(1.0, false), 0.85);
        assert_eq!(text_step(1.5, true), 1.75);
    }

    #[test]
    fn stops_at_the_ends() {
        assert_eq!(text_step(2.0, true), 2.0);
        assert_eq!(text_step(0.85, false), 0.85);
        assert_eq!(text_step(5.0, true), 2.0);
        assert_eq!(text_step(0.1, false), 0.85);
    }

    #[test]
    fn off_list_goes_to_the_neighbour() {
        assert_eq!(text_step(1.1, true), 1.15);
        assert_eq!(text_step(1.1, false), 1.0);
    }

    #[test]
    fn old_files_load_as_usual_size() {
        let s: Settings = serde_json::from_str(r#"{"trunks":true,"recent":["/a"]}"#).unwrap();
        assert_eq!(s.text_size, 1.0);
        assert!(s.trunks);
        assert_eq!(Settings::default().text_size, 1.0);
    }
}
