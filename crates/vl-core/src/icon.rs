//! Profile icons: the app's own icon with a small colored badge, so several
//! shortcuts to the same app can be told apart at a glance.
//!
//! The app icon is read from the user's installation at run time; nothing is
//! bundled or redistributed.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::{imageops, Rgba, RgbaImage};

use crate::install::AppInstall;
use crate::model::Profile;
use crate::paths::Paths;

pub const SIZE: u32 = 256;

pub fn parse_hex(c: &str) -> [u8; 3] {
    let h = c.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0x888888);
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// Load an icon file (PNG, or ICNS on macOS) as RGBA at [`SIZE`].
pub fn load_base(path: &Path) -> Option<RgbaImage> {
    let img = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        #[cfg(target_os = "macos")]
        Some("icns") => load_icns(path)?,
        _ => image::open(path).ok()?.to_rgba8(),
    };
    Some(if img.width() == SIZE && img.height() == SIZE {
        img
    } else {
        imageops::resize(&img, SIZE, SIZE, imageops::FilterType::Lanczos3)
    })
}

#[cfg(target_os = "macos")]
fn load_icns(path: &Path) -> Option<RgbaImage> {
    let f = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let fam = icns::IconFamily::read(f).ok()?;
    let ty = fam
        .available_icons()
        .into_iter()
        .filter(|t| t.pixel_width() >= 128)
        .min_by_key(|t| (t.pixel_width() as i64 - 512).abs())
        .or_else(|| {
            fam.available_icons()
                .into_iter()
                .max_by_key(|t| t.pixel_width())
        })?;
    let img = fam
        .get_icon_with_type(ty)
        .ok()?
        .convert_to(icns::PixelFormat::RGBA);
    RgbaImage::from_raw(img.width(), img.height(), img.into_data().into_vec())
}

/// A plain rounded square in `color`, used when the app's icon isn't found.
fn placeholder(color: [u8; 3]) -> RgbaImage {
    let mut img = RgbaImage::new(SIZE, SIZE);
    let r = 56.0;
    let (lo, hi) = (16.0, SIZE as f32 - 16.0);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let cx = fx.clamp(lo + r, hi - r);
        let cy = fy.clamp(lo + r, hi - r);
        let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() - r;
        let inside = fx >= lo && fx <= hi && fy >= lo && fy <= hi;
        let a = if inside {
            (0.5 - d).clamp(0.0, 1.0)
        } else {
            0.0
        };
        *px = Rgba([color[0], color[1], color[2], (a * 255.0) as u8]);
    }
    img
}

fn blend(dst: &mut Rgba<u8>, src: [u8; 3], alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let out_a = alpha + da * (1.0 - alpha);
    for i in 0..3 {
        let c = (src[i] as f32 * alpha + dst[i] as f32 * da * (1.0 - alpha)) / out_a.max(1e-6);
        dst[i] = c.round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round() as u8;
}

/// Draw the badge: a filled circle with a white ring and a soft shadow, in
/// the bottom-right corner.
pub fn badge(base: &mut RgbaImage, color: [u8; 3]) {
    let s = base.width() as f32;
    let (cx, cy) = (s * 0.76, s * 0.76);
    let r_fill = s * 0.17;
    let r_ring = r_fill + s * 0.03;
    for (x, y, px) in base.enumerate_pixels_mut() {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        // Shadow, offset down a little.
        let ds = ((fx - cx).powi(2) + (fy - cy - s * 0.012).powi(2)).sqrt();
        let shadow = ((r_ring + s * 0.02 - ds) / (s * 0.03)).clamp(0.0, 1.0) * 0.28;
        blend(px, [0, 0, 0], shadow);
        let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
        blend(px, [255, 255, 255], (r_ring - d + 0.5).clamp(0.0, 1.0));
        blend(px, color, (r_fill - d + 0.5).clamp(0.0, 1.0));
    }
}

pub fn compose(install: Option<&AppInstall>, profile: &Profile) -> RgbaImage {
    let color = parse_hex(&profile.color);
    let mut img = install
        .and_then(|i| i.icon.as_deref())
        .and_then(load_base)
        .unwrap_or_else(|| placeholder(parse_hex(profile.provider.brand_color())));
    if profile.isolated {
        badge(&mut img, color);
    }
    img
}

/// Write the profile's icon files and return the one the platform's
/// shortcuts use (PNG on Linux, ICNS on macOS, ICO on Windows).
pub fn write_profile_icon(
    paths: &Paths,
    install: Option<&AppInstall>,
    profile: &Profile,
) -> Result<PathBuf> {
    let dir = paths.icons_dir();
    std::fs::create_dir_all(&dir)?;
    let img = compose(install, profile);
    let png = dir.join(format!("{}.png", profile.id));
    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )?;
    crate::paths::write_atomic(&png, &bytes).context("writing icon")?;
    #[cfg(target_os = "macos")]
    {
        let icns_path = dir.join(format!("{}.icns", profile.id));
        let mut fam = icns::IconFamily::new();
        for sz in [256u32, 128, 32, 16] {
            let small = imageops::resize(&img, sz, sz, imageops::FilterType::Lanczos3);
            let im = icns::Image::from_data(icns::PixelFormat::RGBA, sz, sz, small.into_raw())?;
            fam.add_icon(&im)?;
        }
        let mut out = Vec::new();
        fam.write(&mut out)?;
        crate::paths::write_atomic(&icns_path, &out)?;
        return Ok(icns_path);
    }
    #[cfg(target_os = "windows")]
    {
        let ico_path = dir.join(format!("{}.ico", profile.id));
        let mut icondir = ico::IconDir::new(ico::ResourceType::Icon);
        for sz in [256u32, 64, 48, 32, 16] {
            let small = imageops::resize(&img, sz, sz, imageops::FilterType::Lanczos3);
            let im = ico::IconImage::from_rgba_data(sz, sz, small.into_raw());
            icondir.add_entry(ico::IconDirEntry::encode(&im)?);
        }
        let mut out = Vec::new();
        icondir.write(&mut out)?;
        crate::paths::write_atomic(&ico_path, &out)?;
        return Ok(ico_path);
    }
    #[allow(unreachable_code)]
    Ok(png)
}

/// Base64 PNG of the profile icon at `size`, for the UI.
pub fn data_url(install: Option<&AppInstall>, profile: &Profile, size: u32) -> Option<String> {
    use base64::Engine;
    let img = compose(install, profile);
    let img = imageops::resize(&img, size, size, imageops::FilterType::Lanczos3);
    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// Base64 PNG of the app icon itself (no badge).
pub fn app_icon_data_url(install: &AppInstall, size: u32) -> Option<String> {
    use base64::Engine;
    let img = load_base(install.icon.as_deref()?)?;
    let img = imageops::resize(&img, size, size, imageops::FilterType::Lanczos3);
    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_is_drawn() {
        let mut img = placeholder([10, 10, 10]);
        badge(&mut img, [255, 0, 0]);
        let c = (SIZE as f32 * 0.76) as u32;
        assert_eq!(img.get_pixel(c, c).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(40, 40).0[0], 10);
    }

    #[test]
    fn hex() {
        assert_eq!(parse_hex("#D97757"), [0xD9, 0x77, 0x57]);
    }
}
