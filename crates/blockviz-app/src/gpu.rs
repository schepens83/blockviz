//! Picking the graphics card that drives the display (GPUI renders on Vulkan device 0 unless told).

/// The PCI device id of the one card with a connected display, among several cards. `connectors` are
/// (`card0-DP-3`, status) and `cards` are (`card0`, device id), as /sys/class/drm lists them.
#[cfg(any(target_os = "linux", test))]
fn display_card(connectors: &[(String, String)], cards: &[(String, String)]) -> Option<String> {
    if cards.len() < 2 {
        return None;
    }
    let mut lit: Vec<&str> = connectors.iter().filter(|(_, status)| status.trim() == "connected").filter_map(|(name, _)| name.split_once('-').map(|(card, _)| card)).collect();
    lit.sort_unstable();
    lit.dedup();
    match lit[..] {
        [card] => cards.iter().find(|(name, _)| name == card).map(|(_, id)| id.trim().to_string()),
        _ => None,
    }
}

/// Points GPUI at the card that drives the display, unless `ZED_DEVICE_ID` already says. Call before any
/// thread starts: the environment is not safe to change after.
#[cfg(target_os = "linux")]
pub fn prefer_display_card() {
    if std::env::var_os("ZED_DEVICE_ID").is_some() {
        return;
    }
    if let Some(id) = read_display_card() {
        // SAFETY: main calls this before it starts any thread.
        unsafe { std::env::set_var("ZED_DEVICE_ID", id) };
    }
}

#[cfg(target_os = "linux")]
fn read_display_card() -> Option<String> {
    let drm = std::path::Path::new("/sys/class/drm");
    let (mut connectors, mut cards) = (Vec::new(), Vec::new());
    for entry in std::fs::read_dir(drm).ok()? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(number) = name.strip_prefix("card") else { continue };
        // What can't be read is left out: a card without an id is never chosen, a connector without a status never counts.
        let (list, file) = if number.chars().all(|c| c.is_ascii_digit()) { (&mut cards, "device/device") } else { (&mut connectors, "status") };
        if let Ok(text) = std::fs::read_to_string(drm.join(&name).join(file)) {
            list.push((name, text));
        }
    }
    display_card(&connectors, &cards)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn the_single_card_with_a_display_among_two_is_chosen() {
        let connectors = pairs(&[("card0-DP-3", "connected"), ("card0-HDMI-A-1", "disconnected"), ("card1-eDP-1", "unknown")]);
        let cards = pairs(&[("card0", "0x2705"), ("card1", "0x1681")]);
        assert_eq!(display_card(&connectors, &cards), Some("0x2705".to_string()));
    }

    #[test]
    fn displays_on_two_cards_leave_the_default() {
        let connectors = pairs(&[("card0-DP-3", "connected"), ("card1-HDMI-A-1", "connected")]);
        let cards = pairs(&[("card0", "0x2705"), ("card1", "0x1681")]);
        assert_eq!(display_card(&connectors, &cards), None);
    }

    #[test]
    fn a_lone_card_leaves_the_default() {
        let connectors = pairs(&[("card0-DP-3", "connected")]);
        let cards = pairs(&[("card0", "0x2705")]);
        assert_eq!(display_card(&connectors, &cards), None);
    }

    #[test]
    fn no_connected_display_leaves_the_default() {
        let connectors = pairs(&[("card0-DP-3", "disconnected"), ("card1-eDP-1", "unknown")]);
        let cards = pairs(&[("card0", "0x2705"), ("card1", "0x1681")]);
        assert_eq!(display_card(&connectors, &cards), None);
    }

    #[test]
    fn a_card_without_a_readable_id_leaves_the_default() {
        let connectors = pairs(&[("card0-DP-3", "connected")]);
        let cards = pairs(&[("card1", "0x1681"), ("card2", "0x1002")]);
        assert_eq!(display_card(&connectors, &cards), None);
    }
}
