//! Identity colors. Port of `colorFromPubkey` and `colorForPubkey` in the extension's
//! `identity/appearance.ts`. The shared record always carries a color, so this must match exactly.

use crate::identity::normalize_color_loose;

/// A stored `#rrggbb` wins; otherwise the color comes from the public key.
pub fn color_for_pubkey(pubkey: &str, stored: Option<&str>) -> String {
    stored
        .and_then(normalize_color_loose)
        .unwrap_or_else(|| color_from_pubkey(pubkey))
}

pub fn color_from_pubkey(pubkey_hex: &str) -> String {
    let hex: String = pubkey_hex
        .to_lowercase()
        .chars()
        .filter(char::is_ascii_hexdigit)
        .collect();
    let first = format!("{hex:0<2}");
    let byte = u8::from_str_radix(&first[..2], 16).unwrap_or(0);
    let hue = js_round(f64::from(byte) / 255.0 * 359.0);
    hsl_to_hex(hue, 64.0, 58.0)
}

/// `Math.round`: halves round up, toward positive infinity.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

fn hsl_to_hex(hue: f64, saturation: f64, lightness: f64) -> String {
    let sat = saturation / 100.0;
    let light = lightness / 100.0;
    let chroma = (1.0 - (2.0 * light - 1.0).abs()) * sat;
    let hp = hue / 60.0;
    let x = chroma * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (red, green, blue) = if hp < 1.0 {
        (chroma, x, 0.0)
    } else if hp < 2.0 {
        (x, chroma, 0.0)
    } else if hp < 3.0 {
        (0.0, chroma, x)
    } else if hp < 4.0 {
        (0.0, x, chroma)
    } else if hp < 5.0 {
        (x, 0.0, chroma)
    } else {
        (chroma, 0.0, x)
    };
    let matched = light - chroma / 2.0;
    let channel = |value: f64| format!("{:02x}", js_round((value + matched) * 255.0) as i64);
    format!("#{}{}{}", channel(red), channel(green), channel(blue))
}
