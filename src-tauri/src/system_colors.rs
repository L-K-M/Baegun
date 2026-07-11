use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SystemColors {
    pub accent_color: Option<String>,
    pub accent_text_color: Option<String>,
    pub highlight_color: Option<String>,
    pub highlight_text_color: Option<String>,
}

#[cfg(target_os = "macos")]
use objc2_app_kit::{NSColor, NSColorSpace};

#[cfg(target_os = "macos")]
type Rgb = (u8, u8, u8);

#[cfg(target_os = "macos")]
const DEFAULT_ACCENT: Rgb = (0, 122, 255);

#[cfg(target_os = "macos")]
pub fn get_system_colors() -> SystemColors {
    let accent_rgb = ns_color_to_rgb(&NSColor::controlAccentColor()).unwrap_or(DEFAULT_ACCENT);
    let highlight_rgb =
        ns_color_to_rgb(&NSColor::selectedContentBackgroundColor()).unwrap_or(accent_rgb);

    let accent_text_rgb = ns_color_to_rgb(&NSColor::selectedControlTextColor())
        .unwrap_or_else(|| contrast_text_rgb(accent_rgb));

    let highlight_text_rgb = ns_color_to_rgb(&NSColor::selectedTextColor())
        .or_else(|| ns_color_to_rgb(&NSColor::selectedControlTextColor()))
        .unwrap_or_else(|| contrast_text_rgb(highlight_rgb));

    SystemColors {
        accent_color: Some(rgb_to_hex(accent_rgb)),
        accent_text_color: Some(rgb_to_hex(accent_text_rgb)),
        highlight_color: Some(rgb_to_hex(highlight_rgb)),
        highlight_text_color: Some(rgb_to_hex(highlight_text_rgb)),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn get_system_colors() -> SystemColors {
    SystemColors {
        accent_color: None,
        accent_text_color: None,
        highlight_color: None,
        highlight_text_color: None,
    }
}

#[cfg(target_os = "macos")]
fn ns_color_to_rgb(color: &NSColor) -> Option<Rgb> {
    let srgb = NSColorSpace::sRGBColorSpace();
    let converted = color.colorUsingColorSpace(&srgb)?;

    let mut red = 0.0;
    let mut green = 0.0;
    let mut blue = 0.0;
    let mut _alpha_component = 0.0;

    unsafe {
        converted.getRed_green_blue_alpha(&mut red, &mut green, &mut blue, &mut _alpha_component);
    }

    Some((
        normalized_channel_to_u8(red),
        normalized_channel_to_u8(green),
        normalized_channel_to_u8(blue),
    ))
}

#[cfg(target_os = "macos")]
fn normalized_channel_to_u8(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(target_os = "macos")]
fn rgb_to_hex((r, g, b): Rgb) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[cfg(target_os = "macos")]
fn contrast_text_rgb(rgb: Rgb) -> Rgb {
    if relative_luminance(rgb) > 0.53 {
        (0, 0, 0)
    } else {
        (255, 255, 255)
    }
}

#[cfg(target_os = "macos")]
fn relative_luminance((r, g, b): Rgb) -> f32 {
    0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b)
}

#[cfg(target_os = "macos")]
fn srgb_to_linear(channel: u8) -> f32 {
    let value = channel as f32 / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
