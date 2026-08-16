use crate::{
    models::{custom_skin_file_name, valid_custom_skin_id, CustomSkinMetadata, WidgetPreferences},
    persist_preferences,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::Utc;
use image::{
    imageops::FilterType, DynamicImage, GenericImageView, ImageFormat, ImageReader, Pixel,
};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub(crate) const MAX_INPUT_BYTES: usize = 10 * 1024 * 1024;
const MAX_DECODED_PIXELS: u64 = 16_000_000;
const MAX_LONGEST_EDGE: u32 = 2_048;
const MAX_MANAGED_ASSET_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const DEFAULT_ACCENT_COLOR: &str = "#5A90D6";
static NEXT_ID_SUFFIX: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CustomSkinAsset {
    pub id: String,
    pub data_url: String,
}

fn accepted_format(bytes: &[u8]) -> Result<ImageFormat, String> {
    match image::guess_format(bytes).map_err(|_| "unsupported or malformed image".to_string())? {
        format @ (ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP) => Ok(format),
        _ => Err("only PNG, JPEG, and WebP images are supported".into()),
    }
}

fn decode_image(bytes: &[u8]) -> Result<DynamicImage, String> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err("image exceeds the 10 MiB limit".into());
    }
    let format = accepted_format(bytes)?;
    let dimensions = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|_| "malformed image".to_string())?;
    if u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_DECODED_PIXELS {
        return Err("decoded image exceeds the 16 megapixel limit".into());
    }
    image::load_from_memory_with_format(bytes, format).map_err(|_| "malformed image".to_string())
}

fn resized_png(mut image: DynamicImage) -> Result<(DynamicImage, Vec<u8>), String> {
    let (width, height) = image.dimensions();
    if width.max(height) > MAX_LONGEST_EDGE {
        image = image.resize(MAX_LONGEST_EDGE, MAX_LONGEST_EDGE, FilterType::Lanczos3);
    }
    let mut encoded = Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|_| "failed to encode imported image".to_string())?;
    Ok((image, encoded.into_inner()))
}

fn wcag_channel(value: u8) -> f64 {
    let value = f64::from(value) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn wcag_luminance(red: u8, green: u8, blue: u8) -> f64 {
    0.2126 * wcag_channel(red) + 0.7152 * wcag_channel(green) + 0.0722 * wcag_channel(blue)
}

fn image_color_summary(image: &DynamicImage) -> ([u8; 3], f64) {
    let mut linear_luminance = 0.0;
    let mut red = 0_u64;
    let mut green = 0_u64;
    let mut blue = 0_u64;
    let mut samples = 0_u64;
    let step = ((u64::from(image.width()) * u64::from(image.height()) / 65_536).max(1)) as usize;
    for (_, _, pixel) in image.pixels().step_by(step) {
        let channels = pixel.to_rgba().0;
        let alpha = f64::from(channels[3]) / 255.0;
        let composited = [
            (f64::from(channels[0]) * alpha + 255.0 * (1.0 - alpha)).round() as u8,
            (f64::from(channels[1]) * alpha + 255.0 * (1.0 - alpha)).round() as u8,
            (f64::from(channels[2]) * alpha + 255.0 * (1.0 - alpha)).round() as u8,
        ];
        red += u64::from(composited[0]);
        green += u64::from(composited[1]);
        blue += u64::from(composited[2]);
        linear_luminance += wcag_luminance(composited[0], composited[1], composited[2]);
        samples += 1;
    }
    let samples = samples.max(1);
    (
        [
            (red / samples) as u8,
            (green / samples) as u8,
            (blue / samples) as u8,
        ],
        linear_luminance / samples as f64,
    )
}

fn contrast_ratio(first: f64, second: f64) -> f64 {
    let (lighter, darker) = if first >= second {
        (first, second)
    } else {
        (second, first)
    };
    (lighter + 0.05) / (darker + 0.05)
}

fn accessible_fallback_accent(background_luminance: f64) -> &'static str {
    let blue_luminance = wcag_luminance(0x5A, 0x90, 0xD6);
    if contrast_ratio(blue_luminance, background_luminance) >= 3.0 {
        return DEFAULT_ACCENT_COLOR;
    }
    let black_contrast = contrast_ratio(0.0, background_luminance);
    let white_contrast = contrast_ratio(1.0, background_luminance);
    if black_contrast >= white_contrast {
        "#000000"
    } else {
        "#FFFFFF"
    }
}

fn derived_accent(average: [u8; 3], background_luminance: f64) -> String {
    let maximum = *average.iter().max().unwrap_or(&0);
    let minimum = *average.iter().min().unwrap_or(&0);
    if maximum.saturating_sub(minimum) < 24 {
        return accessible_fallback_accent(background_luminance).into();
    }
    let candidate = if background_luminance > 0.179 {
        average.map(|channel| (f64::from(channel) * 0.42).round() as u8)
    } else {
        average
            .map(|channel| (f64::from(channel) + (255.0 - f64::from(channel)) * 0.58).round() as u8)
    };
    let candidate_luminance = wcag_luminance(candidate[0], candidate[1], candidate[2]);
    if contrast_ratio(candidate_luminance, background_luminance) < 3.0 {
        return accessible_fallback_accent(background_luminance).into();
    }
    format!(
        "#{:02X}{:02X}{:02X}",
        candidate[0], candidate[1], candidate[2]
    )
}

fn editable_name(source_name: &str) -> String {
    let leaf = source_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(source_name)
        .trim();
    Path::new(leaf)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|stem| !stem.is_empty())
        .unwrap_or("Custom Skin")
        .to_string()
}

fn valid_accent_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn metadata_for<'a>(
    preferences: &'a WidgetPreferences,
    id: &str,
) -> Result<&'a CustomSkinMetadata, String> {
    if !valid_custom_skin_id(id) {
        return Err("invalid custom skin id".into());
    }
    let metadata = preferences
        .custom_skins
        .iter()
        .find(|metadata| metadata.id == id)
        .ok_or_else(|| "custom skin not found".to_string())?;
    if metadata.file_name != custom_skin_file_name(id) {
        return Err("invalid custom skin asset metadata".into());
    }
    Ok(metadata)
}

fn skins_directory(config_dir: &Path) -> PathBuf {
    config_dir.join("skins")
}

fn existing_skins_directory(config_dir: &Path) -> Result<Option<PathBuf>, String> {
    let directory = skins_directory(config_dir);
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(Some(directory)),
        Ok(_) => Err("custom skin directory is not a real directory".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("failed to inspect custom skin directory".into()),
    }
}

fn required_skins_directory(config_dir: &Path) -> Result<PathBuf, String> {
    existing_skins_directory(config_dir)?.ok_or_else(|| "custom skin directory missing".to_string())
}

fn ensure_skins_directory(config_dir: &Path) -> Result<PathBuf, String> {
    match fs::symlink_metadata(config_dir) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Err("app config path is not a real directory".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(config_dir)
                .map_err(|_| "failed to create app config directory".to_string())?;
            let metadata = fs::symlink_metadata(config_dir)
                .map_err(|_| "failed to inspect app config directory".to_string())?;
            if !metadata.file_type().is_dir() {
                return Err("app config path is not a real directory".into());
            }
        }
        Err(_) => return Err("failed to inspect app config directory".into()),
    }
    if let Some(directory) = existing_skins_directory(config_dir)? {
        return Ok(directory);
    }
    let directory = skins_directory(config_dir);
    match fs::create_dir(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("failed to create custom skin directory".into()),
    }
    // This rejects stable symlink substitution. Eliminating the remaining
    // check/use race requires platform-specific directory handles/openat.
    existing_skins_directory(config_dir)?
        .ok_or_else(|| "custom skin directory missing after creation".to_string())
}

fn generated_custom_skin_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("custom-") else {
        return false;
    };
    let Some((timestamp, suffix)) = rest.rsplit_once('-') else {
        return false;
    };
    !timestamp.is_empty()
        && timestamp.bytes().all(|byte| byte.is_ascii_digit())
        && !suffix.is_empty()
        && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn controlled_artifact_id(file_name: &str) -> Option<&str> {
    let id = file_name
        .strip_suffix(".png")
        .or_else(|| file_name.strip_prefix('.')?.strip_suffix(".importing"))
        .or_else(|| file_name.strip_prefix('.')?.strip_suffix(".deleting"))?;
    generated_custom_skin_id(id).then_some(id)
}

fn remove_managed_artifact(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    let result = if metadata.file_type().is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };
    if let Err(error) = result {
        eprintln!("failed to clean invalid custom skin artifact: {error}");
    }
}

pub(crate) fn validate_managed_asset(path: &Path) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "custom skin asset missing".to_string())?;
    if !metadata.file_type().is_file() {
        return Err("custom skin asset is not a regular file".into());
    }
    if metadata.len() > MAX_MANAGED_ASSET_BYTES {
        return Err("custom skin asset exceeds the normalized size limit".into());
    }
    let capacity = usize::try_from(metadata.len())
        .map_err(|_| "custom skin asset exceeds the platform size limit".to_string())?;
    let mut bytes = Vec::with_capacity(capacity);
    fs::File::open(path)
        .map_err(|_| "custom skin asset unavailable".to_string())?
        .take(MAX_MANAGED_ASSET_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "failed to read custom skin asset".to_string())?;
    if bytes.len() as u64 > MAX_MANAGED_ASSET_BYTES {
        return Err("custom skin asset exceeds the normalized size limit".into());
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || image::guess_format(&bytes).ok() != Some(ImageFormat::Png)
    {
        return Err("custom skin asset is not PNG".into());
    }
    let (width, height) = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png)
        .into_dimensions()
        .map_err(|_| "custom skin PNG header is invalid".to_string())?;
    if u64::from(width) * u64::from(height) > MAX_DECODED_PIXELS
        || width.max(height) > MAX_LONGEST_EDGE
    {
        return Err("custom skin asset exceeds normalized dimensions".into());
    }
    let decoded = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
        .map_err(|_| "custom skin PNG data is invalid".to_string())?;
    let mut canonical = Cursor::new(Vec::new());
    decoded
        .write_to(&mut canonical, ImageFormat::Png)
        .map_err(|_| "custom skin asset cannot be normalized".to_string())?;
    let canonical = canonical.into_inner();
    if !canonical.starts_with(b"\x89PNG\r\n\x1a\n") || canonical != bytes {
        return Err("custom skin asset is not canonical PNG".into());
    }
    Ok(bytes)
}

pub(crate) fn reconcile_skin_storage(
    config_dir: &Path,
    current: &WidgetPreferences,
) -> Result<(WidgetPreferences, bool), String> {
    let mut next = current.clone();
    let Some(directory) = existing_skins_directory(config_dir)? else {
        let changed = !next.custom_skins.is_empty();
        next.custom_skins.clear();
        return Ok((next.normalized(), changed));
    };
    let mut available_ids = std::collections::HashSet::new();
    for metadata in &next.custom_skins {
        let final_asset = directory.join(custom_skin_file_name(&metadata.id));
        let tombstone = directory.join(format!(".{}.deleting", metadata.id));
        let importing = directory.join(format!(".{}.importing", metadata.id));
        if !final_asset.exists() && tombstone.exists() {
            fs::rename(&tombstone, &final_asset)
                .map_err(|_| "failed to restore interrupted custom skin delete".to_string())?;
        } else if !final_asset.exists() && importing.exists() {
            fs::rename(&importing, &final_asset)
                .map_err(|_| "failed to restore interrupted custom skin import".to_string())?;
        }
        if final_asset.exists() && validate_managed_asset(&final_asset).is_ok() {
            available_ids.insert(metadata.id.clone());
            for stale in [&tombstone, &importing] {
                if stale.exists() {
                    remove_managed_artifact(stale);
                }
            }
        } else {
            remove_managed_artifact(&final_asset);
            remove_managed_artifact(&tombstone);
            remove_managed_artifact(&importing);
        }
    }
    let previous_catalog_len = next.custom_skins.len();
    next.custom_skins
        .retain(|metadata| available_ids.contains(&metadata.id));
    let changed = next.custom_skins.len() != previous_catalog_len;
    next = next.normalized();
    let known_ids = next
        .custom_skins
        .iter()
        .map(|metadata| metadata.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    for entry in fs::read_dir(&directory)
        .map_err(|_| "failed to inspect custom skin directory".to_string())?
    {
        let entry = entry.map_err(|_| "failed to inspect custom skin asset".to_string())?;
        let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(id) = controlled_artifact_id(&file_name) else {
            continue;
        };
        if !known_ids.contains(id) {
            remove_managed_artifact(&entry.path());
        }
    }
    Ok((next, changed))
}

fn unique_id(directory: &Path, preferences: &WidgetPreferences) -> String {
    loop {
        let timestamp = Utc::now().timestamp_millis();
        let suffix = NEXT_ID_SUFFIX.fetch_add(1, Ordering::Relaxed);
        let id = format!("custom-{timestamp}-{suffix:08x}");
        let known = preferences
            .custom_skins
            .iter()
            .any(|metadata| metadata.id == id);
        if !known && !directory.join(format!("{id}.png")).exists() {
            return id;
        }
    }
}

pub(crate) fn import_skin(
    config_dir: &Path,
    preferences_path: &PathBuf,
    current: &WidgetPreferences,
    source_name: &str,
    bytes: &[u8],
) -> Result<(CustomSkinMetadata, WidgetPreferences), String> {
    let decoded = decode_image(bytes)?;
    let (processed, png) = resized_png(decoded)?;
    let (average, luminance) = image_color_summary(&processed);
    let directory = ensure_skins_directory(config_dir)?;
    let id = unique_id(&directory, current);
    let file_name = custom_skin_file_name(&id);
    let metadata = CustomSkinMetadata {
        id: id.clone(),
        name: editable_name(source_name),
        file_name,
        detected_tone: if luminance > 0.179 { "dark" } else { "light" }.into(),
        text_tone: "auto".into(),
        accent_color: derived_accent(average, luminance),
    };
    let mut next = current.clone();
    next.custom_skins.push(metadata.clone());
    let asset_path = directory.join(&metadata.file_name);
    let importing_path = directory.join(format!(".{id}.importing"));
    let mut asset = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&importing_path)
        .map_err(|_| "failed to create custom skin asset".to_string())?;
    if let Err(error) = asset.write_all(&png).and_then(|_| asset.sync_all()) {
        let _ = fs::remove_file(&importing_path);
        return Err(format!("failed to write custom skin asset: {error}"));
    }
    drop(asset);
    if let Err(error) = fs::rename(&importing_path, &asset_path) {
        let _ = fs::remove_file(&importing_path);
        return Err(format!("failed to commit custom skin asset: {error}"));
    }
    if let Err(error) = persist_preferences(preferences_path, &next) {
        let _ = fs::remove_file(&asset_path);
        return Err(error);
    }
    Ok((metadata, next))
}

pub(crate) fn load_skin_asset(
    config_dir: &Path,
    preferences: &WidgetPreferences,
    id: &str,
) -> Result<CustomSkinAsset, String> {
    let metadata = metadata_for(preferences, id)?;
    let directory = required_skins_directory(config_dir)?;
    let bytes = validate_managed_asset(&directory.join(&metadata.file_name))?;
    Ok(CustomSkinAsset {
        id: id.into(),
        data_url: format!("data:image/png;base64,{}", BASE64_STANDARD.encode(bytes)),
    })
}

pub(crate) fn update_skin(
    preferences_path: &PathBuf,
    current: &WidgetPreferences,
    id: &str,
    name: &str,
    text_tone: &str,
    accent_color: &str,
) -> Result<WidgetPreferences, String> {
    if !valid_custom_skin_id(id)
        || name.trim().is_empty()
        || !matches!(text_tone, "auto" | "light" | "dark")
        || !valid_accent_color(accent_color)
    {
        return Err("invalid custom skin update".into());
    }
    let mut next = current.clone();
    let metadata = next
        .custom_skins
        .iter_mut()
        .find(|metadata| metadata.id == id)
        .ok_or_else(|| "custom skin not found".to_string())?;
    metadata.name = name.trim().into();
    metadata.text_tone = text_tone.into();
    metadata.accent_color = accent_color.into();
    let next = next.normalized();
    persist_preferences(preferences_path, &next)?;
    Ok(next)
}

pub(crate) fn delete_skin(
    config_dir: &Path,
    preferences_path: &PathBuf,
    current: &WidgetPreferences,
    id: &str,
) -> Result<WidgetPreferences, String> {
    delete_skin_with_cleanup(config_dir, preferences_path, current, id, |path| {
        fs::remove_file(path)
    })
}

fn delete_skin_with_cleanup<F>(
    config_dir: &Path,
    preferences_path: &PathBuf,
    current: &WidgetPreferences,
    id: &str,
    cleanup: F,
) -> Result<WidgetPreferences, String>
where
    F: FnOnce(&Path) -> std::io::Result<()>,
{
    let metadata = metadata_for(current, id)?;
    let directory = required_skins_directory(config_dir)?;
    let asset_path = directory.join(&metadata.file_name);
    let tombstone = directory.join(format!(".{id}.deleting"));
    fs::rename(&asset_path, &tombstone).map_err(|_| "custom skin asset missing".to_string())?;

    let mut next = current.clone();
    next.custom_skins.retain(|metadata| metadata.id != id);
    if next.selected_skin == format!("custom:{id}") {
        next.selected_skin = "glass".into();
    }
    if let Err(error) = persist_preferences(preferences_path, &next) {
        if fs::rename(&tombstone, &asset_path).is_err() {
            return Err(format!("{error}; failed to restore custom skin asset"));
        }
        return Err(error);
    }
    if let Err(error) = cleanup(&tombstone) {
        eprintln!("failed to clean up deleted custom skin tombstone: {error}");
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::WidgetPreferences;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
    use std::{
        fs,
        io::Cursor,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let suffix = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "quota-float-custom-skins-{}-{suffix}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn encoded_image(format: ImageFormat, width: u32, height: u32, color: Rgba<u8>) -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(width, height, color));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }

    fn imported_files(config_dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(config_dir.join("skins"))
            .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
            .unwrap_or_default()
    }

    fn stored_metadata(id: &str) -> CustomSkinMetadata {
        CustomSkinMetadata {
            id: id.into(),
            name: "Stored".into(),
            file_name: custom_skin_file_name(id),
            detected_tone: "dark".into(),
            text_tone: "auto".into(),
            accent_color: DEFAULT_ACCENT_COLOR.into(),
        }
    }

    #[test]
    fn reconciliation_removes_only_controlled_import_orphans() {
        let directory = TestDirectory::new();
        let skins = directory.path().join("skins");
        fs::create_dir(&skins).unwrap();
        let orphan = skins.join("custom-100-00000001.png");
        let importing = skins.join(".custom-100-00000002.importing");
        let deleting = skins.join(".custom-100-00000005.deleting");
        let unrelated = skins.join("notes.png");
        let uncontrolled = skins.join(".not-a-managed-id.importing");
        for path in [&orphan, &importing, &deleting, &unrelated, &uncontrolled] {
            fs::write(path, b"fixture").unwrap();
        }

        let (preferences, changed) =
            reconcile_skin_storage(directory.path(), &WidgetPreferences::default()).unwrap();

        assert!(!changed);
        assert!(preferences.custom_skins.is_empty());
        assert!(!orphan.exists());
        assert!(!importing.exists());
        assert!(!deleting.exists());
        assert!(unrelated.exists());
        assert!(uncontrolled.exists());
    }

    #[test]
    fn reconciliation_restores_a_delete_interrupted_before_preferences_commit() {
        let directory = TestDirectory::new();
        let skins = directory.path().join("skins");
        fs::create_dir(&skins).unwrap();
        let id = "custom-100-00000003";
        let final_asset = skins.join(custom_skin_file_name(id));
        let tombstone = skins.join(format!(".{id}.deleting"));
        let png = encoded_image(ImageFormat::Png, 2, 2, Rgba([10, 20, 30, 255]));
        fs::write(&tombstone, &png).unwrap();
        let preferences = WidgetPreferences {
            selected_skin: format!("custom:{id}"),
            custom_skins: vec![stored_metadata(id)],
            ..WidgetPreferences::default()
        };

        let (reconciled, changed) = reconcile_skin_storage(directory.path(), &preferences).unwrap();

        assert!(!changed);
        assert_eq!(reconciled.selected_skin, format!("custom:{id}"));
        assert_eq!(reconciled.custom_skins.len(), 1);
        assert_eq!(fs::read(final_asset).unwrap(), png);
        assert!(!tombstone.exists());
    }

    #[test]
    fn reconciliation_drops_catalog_entries_with_no_recoverable_asset() {
        let directory = TestDirectory::new();
        fs::create_dir(directory.path().join("skins")).unwrap();
        let id = "custom-100-00000004";
        let preferences = WidgetPreferences {
            selected_skin: format!("custom:{id}"),
            custom_skins: vec![stored_metadata(id)],
            ..WidgetPreferences::default()
        };

        let (reconciled, changed) = reconcile_skin_storage(directory.path(), &preferences).unwrap();

        assert!(changed);
        assert_eq!(reconciled.selected_skin, "glass");
        assert!(reconciled.custom_skins.is_empty());
    }

    #[test]
    fn reconciliation_promotes_a_cataloged_import_temp_to_the_final_asset() {
        let directory = TestDirectory::new();
        let skins = directory.path().join("skins");
        fs::create_dir(&skins).unwrap();
        let id = "custom-100-00000006";
        let importing = skins.join(format!(".{id}.importing"));
        let final_asset = skins.join(custom_skin_file_name(id));
        let png = encoded_image(ImageFormat::Png, 2, 2, Rgba([30, 20, 10, 255]));
        fs::write(&importing, &png).unwrap();
        let preferences = WidgetPreferences {
            custom_skins: vec![stored_metadata(id)],
            ..WidgetPreferences::default()
        };

        let (reconciled, changed) = reconcile_skin_storage(directory.path(), &preferences).unwrap();

        assert!(!changed);
        assert_eq!(reconciled.custom_skins.len(), 1);
        assert_eq!(fs::read(final_asset).unwrap(), png);
        assert!(!importing.exists());
    }

    #[test]
    fn corrupt_managed_asset_is_rejected_and_reconciled_out_of_the_catalog() {
        let directory = TestDirectory::new();
        let skins = directory.path().join("skins");
        fs::create_dir(&skins).unwrap();
        let id = "custom-100-00000009";
        let asset = skins.join(custom_skin_file_name(id));
        fs::write(&asset, b"not a png").unwrap();
        let preferences = WidgetPreferences {
            selected_skin: format!("custom:{id}"),
            custom_skins: vec![stored_metadata(id)],
            ..WidgetPreferences::default()
        };

        assert!(load_skin_asset(directory.path(), &preferences, id).is_err());
        let (reconciled, changed) = reconcile_skin_storage(directory.path(), &preferences).unwrap();

        assert!(changed);
        assert_eq!(reconciled.selected_skin, "glass");
        assert!(reconciled.custom_skins.is_empty());
        assert!(!asset.exists());
    }

    #[test]
    fn noncanonical_png_with_trailing_payload_is_not_returned_as_a_data_url() {
        let directory = TestDirectory::new();
        let skins = directory.path().join("skins");
        fs::create_dir(&skins).unwrap();
        let id = "custom-100-00000013";
        let asset = skins.join(custom_skin_file_name(id));
        let mut png = encoded_image(ImageFormat::Png, 2, 2, Rgba([4, 5, 6, 255]));
        png.extend_from_slice(b"trailing payload");
        fs::write(&asset, png).unwrap();
        let preferences = WidgetPreferences {
            custom_skins: vec![stored_metadata(id)],
            ..WidgetPreferences::default()
        };

        assert!(load_skin_asset(directory.path(), &preferences, id).is_err());
    }

    #[test]
    fn managed_asset_directory_is_rejected_and_cleaned_without_catalog_exposure() {
        let directory = TestDirectory::new();
        let skins = directory.path().join("skins");
        fs::create_dir(&skins).unwrap();
        let id = "custom-100-00000010";
        let asset = skins.join(custom_skin_file_name(id));
        fs::create_dir(&asset).unwrap();
        let preferences = WidgetPreferences {
            selected_skin: format!("custom:{id}"),
            custom_skins: vec![stored_metadata(id)],
            ..WidgetPreferences::default()
        };

        assert!(load_skin_asset(directory.path(), &preferences, id).is_err());
        let (reconciled, changed) = reconcile_skin_storage(directory.path(), &preferences).unwrap();

        assert!(changed);
        assert_eq!(reconciled.selected_skin, "glass");
        assert!(reconciled.custom_skins.is_empty());
        assert!(!asset.exists());
    }

    #[test]
    fn oversized_managed_asset_is_rejected_before_reading_and_cleaned() {
        let directory = TestDirectory::new();
        let skins = directory.path().join("skins");
        fs::create_dir(&skins).unwrap();
        let id = "custom-100-00000011";
        let asset = skins.join(custom_skin_file_name(id));
        let file = fs::File::create(&asset).unwrap();
        file.set_len(MAX_MANAGED_ASSET_BYTES + 1).unwrap();
        let preferences = WidgetPreferences {
            selected_skin: format!("custom:{id}"),
            custom_skins: vec![stored_metadata(id)],
            ..WidgetPreferences::default()
        };

        assert!(load_skin_asset(directory.path(), &preferences, id).is_err());
        let (reconciled, changed) = reconcile_skin_storage(directory.path(), &preferences).unwrap();

        assert!(changed);
        assert_eq!(reconciled.selected_skin, "glass");
        assert!(reconciled.custom_skins.is_empty());
        assert!(!asset.exists());
    }

    #[cfg(unix)]
    #[test]
    fn managed_asset_symlink_is_rejected_and_removed_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let directory = TestDirectory::new();
        let skins = directory.path().join("skins");
        fs::create_dir(&skins).unwrap();
        let id = "custom-100-00000012";
        let target = directory.path().join("outside.png");
        fs::write(
            &target,
            encoded_image(ImageFormat::Png, 2, 2, Rgba([1, 2, 3, 255])),
        )
        .unwrap();
        let asset = skins.join(custom_skin_file_name(id));
        symlink(&target, &asset).unwrap();
        let preferences = WidgetPreferences {
            selected_skin: format!("custom:{id}"),
            custom_skins: vec![stored_metadata(id)],
            ..WidgetPreferences::default()
        };

        assert!(load_skin_asset(directory.path(), &preferences, id).is_err());
        let (reconciled, changed) = reconcile_skin_storage(directory.path(), &preferences).unwrap();

        assert!(changed);
        assert_eq!(reconciled.selected_skin, "glass");
        assert!(reconciled.custom_skins.is_empty());
        assert!(fs::symlink_metadata(&asset).is_err());
        assert!(target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn managed_skins_directory_symlink_is_never_followed_or_cleaned() {
        use std::os::unix::fs::symlink;

        let directory = TestDirectory::new();
        let outside = directory.path().join("outside-skins");
        fs::create_dir(&outside).unwrap();
        let id = "custom-100-00000014";
        let outside_asset = outside.join(custom_skin_file_name(id));
        let png = encoded_image(ImageFormat::Png, 2, 2, Rgba([7, 8, 9, 255]));
        fs::write(&outside_asset, &png).unwrap();
        symlink(&outside, directory.path().join("skins")).unwrap();
        let preferences_path = directory.path().join("preferences.json");
        let preferences = WidgetPreferences {
            selected_skin: format!("custom:{id}"),
            custom_skins: vec![stored_metadata(id)],
            ..WidgetPreferences::default()
        };

        assert!(reconcile_skin_storage(directory.path(), &preferences).is_err());
        assert!(load_skin_asset(directory.path(), &preferences, id).is_err());
        assert!(delete_skin(directory.path(), &preferences_path, &preferences, id,).is_err());
        assert!(import_skin(
            directory.path(),
            &preferences_path,
            &preferences,
            "new.png",
            &png,
        )
        .is_err());
        assert_eq!(fs::read(&outside_asset).unwrap(), png);
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
        assert!(fs::symlink_metadata(directory.path().join("skins"))
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn imports_png_jpeg_and_webp_as_metadata_only_preferences_and_png_assets() {
        for (format, file_name) in [
            (ImageFormat::Png, "Quiet Lake.png"),
            (ImageFormat::Jpeg, "Quiet Lake.jpg"),
            (ImageFormat::WebP, "Quiet Lake.webp"),
        ] {
            let directory = TestDirectory::new();
            let preferences_path = directory.path().join("preferences.json");
            let source = encoded_image(format, 12, 8, Rgba([245, 245, 245, 255]));

            let (metadata, preferences) = import_skin(
                directory.path(),
                &preferences_path,
                &WidgetPreferences::default(),
                file_name,
                &source,
            )
            .unwrap();

            assert!(metadata.id.starts_with("custom-"));
            assert_eq!(metadata.name, "Quiet Lake");
            assert_eq!(metadata.file_name, format!("{}.png", metadata.id));
            assert_eq!(metadata.detected_tone, "dark");
            assert_eq!(metadata.text_tone, "auto");
            assert_eq!(preferences.custom_skins.len(), 1);
            let persisted = fs::read_to_string(&preferences_path).unwrap();
            assert!(!persisted.contains("data:image"));
            assert!(!persisted.contains("245,245,245"));
            let stored =
                fs::read(directory.path().join("skins").join(&metadata.file_name)).unwrap();
            assert_eq!(&stored[..8], b"\x89PNG\r\n\x1a\n");
        }
    }

    #[test]
    fn rejected_inputs_do_not_write_assets_or_preferences() {
        let cases = [
            (
                "vector.svg",
                br#"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"#.to_vec(),
            ),
            ("broken.png", b"not an image".to_vec()),
            ("too-large.png", vec![0; MAX_INPUT_BYTES + 1]),
        ];
        for (file_name, bytes) in cases {
            let directory = TestDirectory::new();
            let preferences_path = directory.path().join("preferences.json");
            let result = import_skin(
                directory.path(),
                &preferences_path,
                &WidgetPreferences::default(),
                file_name,
                &bytes,
            );
            assert!(result.is_err(), "{file_name} should be rejected");
            assert!(imported_files(directory.path()).is_empty());
            assert!(!preferences_path.exists());
        }
    }

    #[test]
    fn rejects_decoded_images_over_sixteen_megapixels_without_writing() {
        let directory = TestDirectory::new();
        let preferences_path = directory.path().join("preferences.json");
        let bytes = encoded_image(ImageFormat::Png, 4_001, 4_000, Rgba([1, 2, 3, 255]));

        let result = import_skin(
            directory.path(),
            &preferences_path,
            &WidgetPreferences::default(),
            "oversized.png",
            &bytes,
        );

        assert!(result.is_err());
        assert!(imported_files(directory.path()).is_empty());
        assert!(!preferences_path.exists());
    }

    #[test]
    fn resizes_the_longest_edge_and_returns_an_image_png_data_url() {
        let directory = TestDirectory::new();
        let preferences_path = directory.path().join("preferences.json");
        let source = encoded_image(ImageFormat::Jpeg, 3_000, 1_500, Rgba([10, 20, 30, 255]));
        let (metadata, preferences) = import_skin(
            directory.path(),
            &preferences_path,
            &WidgetPreferences::default(),
            "wide.photo.jpg",
            &source,
        )
        .unwrap();

        let stored = image::open(directory.path().join("skins").join(&metadata.file_name)).unwrap();
        assert_eq!((stored.width(), stored.height()), (2_048, 1_024));
        let asset = load_skin_asset(directory.path(), &preferences, &metadata.id).unwrap();
        assert_eq!(asset.id, metadata.id);
        assert!(asset
            .data_url
            .starts_with("data:image/png;base64,iVBORw0KGgo"));
    }

    #[test]
    fn derives_readable_text_and_accessible_fallback_accents_for_light_mid_and_dark_images() {
        for (value, expected_tone, expected_accent) in [
            (255, "dark", DEFAULT_ACCENT_COLOR),
            (128, "dark", "#000000"),
            (0, "light", DEFAULT_ACCENT_COLOR),
        ] {
            let directory = TestDirectory::new();
            let preferences_path = directory.path().join("preferences.json");
            let source = encoded_image(ImageFormat::Png, 2, 2, Rgba([value, value, value, 255]));
            let (metadata, _) = import_skin(
                directory.path(),
                &preferences_path,
                &WidgetPreferences::default(),
                "neutral.png",
                &source,
            )
            .unwrap();
            assert_eq!(metadata.detected_tone, expected_tone);
            assert_eq!(metadata.accent_color, expected_accent);
        }
    }

    #[test]
    fn ids_are_collision_safe_even_when_imports_share_a_timestamp_and_name() {
        let directory = TestDirectory::new();
        let preferences_path = directory.path().join("preferences.json");
        let source = encoded_image(ImageFormat::Png, 2, 2, Rgba([255, 255, 255, 255]));
        let (_, first) = import_skin(
            directory.path(),
            &preferences_path,
            &WidgetPreferences::default(),
            "same.png",
            &source,
        )
        .unwrap();
        let (second_metadata, second) = import_skin(
            directory.path(),
            &preferences_path,
            &first,
            "same.png",
            &source,
        )
        .unwrap();
        assert_ne!(first.custom_skins[0].id, second_metadata.id);
        assert_eq!(second.custom_skins.len(), 2);
        assert_eq!(imported_files(directory.path()).len(), 2);
    }

    #[test]
    fn updates_editable_metadata_without_rewriting_the_asset() {
        let directory = TestDirectory::new();
        let preferences_path = directory.path().join("preferences.json");
        let source = encoded_image(ImageFormat::Png, 2, 2, Rgba([255, 255, 255, 255]));
        let (metadata, preferences) = import_skin(
            directory.path(),
            &preferences_path,
            &WidgetPreferences::default(),
            "before.png",
            &source,
        )
        .unwrap();
        let asset_path = directory.path().join("skins").join(&metadata.file_name);
        let before = fs::read(&asset_path).unwrap();

        let updated = update_skin(
            &preferences_path,
            &preferences,
            &metadata.id,
            "After",
            "light",
            "#12ab34",
        )
        .unwrap();

        assert_eq!(updated.custom_skins[0].name, "After");
        assert_eq!(updated.custom_skins[0].text_tone, "light");
        assert_eq!(updated.custom_skins[0].accent_color, "#12AB34");
        assert_eq!(fs::read(asset_path).unwrap(), before);
    }

    #[test]
    fn rejects_invalid_tone_and_accent_without_rewriting_preferences() {
        let directory = TestDirectory::new();
        let preferences_path = directory.path().join("preferences.json");
        let source = encoded_image(ImageFormat::Png, 2, 2, Rgba([255, 255, 255, 255]));
        let (metadata, preferences) = import_skin(
            directory.path(),
            &preferences_path,
            &WidgetPreferences::default(),
            "strict.png",
            &source,
        )
        .unwrap();
        let before = fs::read(&preferences_path).unwrap();

        for (text_tone, accent_color) in [("neon", "#123456"), ("auto", "blue")] {
            let result = update_skin(
                &preferences_path,
                &preferences,
                &metadata.id,
                "Strict",
                text_tone,
                accent_color,
            );
            assert!(result.is_err());
            assert_eq!(fs::read(&preferences_path).unwrap(), before);
        }
    }

    #[test]
    fn deleting_the_active_skin_falls_back_and_removes_file_and_metadata() {
        let directory = TestDirectory::new();
        let preferences_path = directory.path().join("preferences.json");
        let source = encoded_image(ImageFormat::Png, 2, 2, Rgba([255, 255, 255, 255]));
        let (metadata, mut preferences) = import_skin(
            directory.path(),
            &preferences_path,
            &WidgetPreferences::default(),
            "active.png",
            &source,
        )
        .unwrap();
        preferences.selected_skin = format!("custom:{}", metadata.id);
        crate::persist_preferences(&preferences_path, &preferences).unwrap();

        let deleted = delete_skin(
            directory.path(),
            &preferences_path,
            &preferences,
            &metadata.id,
        )
        .unwrap();

        assert_eq!(deleted.selected_skin, "glass");
        assert!(deleted.custom_skins.is_empty());
        assert!(imported_files(directory.path()).is_empty());
    }

    #[test]
    fn failed_delete_persistence_restores_the_asset_and_leaves_preferences_unchanged() {
        let directory = TestDirectory::new();
        let good_preferences_path = directory.path().join("preferences.json");
        let source = encoded_image(ImageFormat::Png, 2, 2, Rgba([255, 255, 255, 255]));
        let (metadata, preferences) = import_skin(
            directory.path(),
            &good_preferences_path,
            &WidgetPreferences::default(),
            "kept.png",
            &source,
        )
        .unwrap();
        let asset_path = directory.path().join("skins").join(&metadata.file_name);
        let before = fs::read(&asset_path).unwrap();
        let blocked_parent = directory.path().join("blocked-parent");
        fs::write(&blocked_parent, b"not a directory").unwrap();
        let invalid_preferences_path = blocked_parent.join("preferences.json");

        let result = delete_skin(
            directory.path(),
            &invalid_preferences_path,
            &preferences,
            &metadata.id,
        );

        assert!(result.is_err());
        assert_eq!(fs::read(asset_path).unwrap(), before);
        assert_eq!(preferences.custom_skins.len(), 1);
    }

    #[test]
    fn failed_tombstone_cleanup_keeps_the_committed_delete_successful() {
        let directory = TestDirectory::new();
        let preferences_path = directory.path().join("preferences.json");
        let source = encoded_image(ImageFormat::Png, 2, 2, Rgba([255, 255, 255, 255]));
        let (metadata, mut preferences) = import_skin(
            directory.path(),
            &preferences_path,
            &WidgetPreferences::default(),
            "active.png",
            &source,
        )
        .unwrap();
        preferences.selected_skin = format!("custom:{}", metadata.id);
        crate::persist_preferences(&preferences_path, &preferences).unwrap();

        let deleted = delete_skin_with_cleanup(
            directory.path(),
            &preferences_path,
            &preferences,
            &metadata.id,
            |_| Err(std::io::Error::other("simulated cleanup failure")),
        )
        .unwrap();

        assert_eq!(deleted.selected_skin, "glass");
        assert!(deleted.custom_skins.is_empty());
        assert!(!directory
            .path()
            .join("skins")
            .join(&metadata.file_name)
            .exists());
        let persisted = crate::load_preferences(&preferences_path);
        assert_eq!(persisted.selected_skin, "glass");
        assert!(persisted.custom_skins.is_empty());
    }

    #[test]
    fn rejects_path_traversal_ids_without_touching_files() {
        let directory = TestDirectory::new();
        let preferences_path = directory.path().join("preferences.json");
        let outside = directory.path().join("outside.png");
        fs::write(&outside, b"keep").unwrap();

        assert!(load_skin_asset(
            directory.path(),
            &WidgetPreferences::default(),
            "../outside"
        )
        .is_err());
        assert!(delete_skin(
            directory.path(),
            &preferences_path,
            &WidgetPreferences::default(),
            "../outside",
        )
        .is_err());
        assert_eq!(fs::read(outside).unwrap(), b"keep");
    }
}
