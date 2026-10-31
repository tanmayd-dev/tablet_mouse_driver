//! Wacom device discovery and selection.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use evdev::{AbsoluteAxisCode, Device, KeyCode};

/// Absolute axis metadata reported by the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisInfo {
    pub min: i32,
    pub max: i32,
    pub value: i32,
}

impl AxisInfo {
    pub fn range(self) -> crate::mapping::AxisRange {
        crate::mapping::AxisRange::new(self.min, self.max)
    }
}

/// A discovered input node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub path: PathBuf,
    pub name: String,
    pub vendor: Option<u16>,
    pub product: Option<u16>,
    pub x: Option<AxisInfo>,
    pub y: Option<AxisInfo>,
    pub pressure: Option<AxisInfo>,
    pub has_pen_tool: bool,
    pub score: i32,
}

impl DeviceInfo {
    pub fn is_pen(&self) -> bool {
        let name = self.name.to_ascii_lowercase();
        if name.contains("pad") || name.contains("expresskey") {
            return false;
        }
        (self.has_pen_tool && self.x.is_some()) || name.contains("wacom") || name.contains("pen")
    }

    pub fn is_pad(&self) -> bool {
        let name = self.name.to_ascii_lowercase();
        !self.has_pen_tool
            && name.contains("pad")
            && (name.contains("wacom") || name.contains("expresskey"))
    }

    pub fn x_range(&self) -> crate::mapping::AxisRange {
        self.x.map(AxisInfo::range).unwrap_or_default()
    }

    pub fn y_range(&self) -> crate::mapping::AxisRange {
        self.y.map(AxisInfo::range).unwrap_or_default()
    }

    pub fn pressure_max(&self) -> i32 {
        self.pressure.map(|axis| axis.max).unwrap_or(4095).max(1)
    }
}

/// A human-readable one-line summary used by `probe`.
impl fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = if self.is_pen() {
            "pen"
        } else if self.is_pad() {
            "pad"
        } else {
            "other"
        };
        write!(f, "{:<8} {}  '{}'", kind, self.path.display(), self.name)?;
        if let Some(x) = self.x {
            write!(f, "  x={}..{}", x.min, x.max)?;
        }
        if let Some(y) = self.y {
            write!(f, "  y={}..{}", y.min, y.max)?;
        }
        if let Some(p) = self.pressure {
            write!(f, "  pressure=0..{}", p.max)?;
        }
        Ok(())
    }
}

/// Errors from device discovery.
#[derive(Debug)]
pub enum DeviceError {
    NotFound(String),
    Io(io::Error),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(message) => f.write_str(message),
            Self::Io(err) => write!(f, "input device error: {err}"),
        }
    }
}

impl std::error::Error for DeviceError {}

impl From<io::Error> for DeviceError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// Scans the live system.
pub fn scan_devices() -> Vec<DeviceInfo> {
    scan_devices_in(Path::new("/sys/class/input"), Path::new("/dev/input"))
}

/// Scans configurable roots (used by tests).
pub fn scan_devices_in(sysfs_root: &Path, dev_root: &Path) -> Vec<DeviceInfo> {
    let mut devices = Vec::new();
    let Ok(entries) = fs::read_dir(sysfs_root) else {
        return devices;
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if !name.starts_with("event") {
            continue;
        }

        let sysfs_path = entry.path();
        let device_sub = sysfs_path.join("device");
        let device_name =
            read_trimmed(&device_sub.join("name")).unwrap_or_else(|| "Unknown".to_string());
        let vendor = read_hex(&device_sub.join("id/vendor"));
        let product = read_hex(&device_sub.join("id/product"));

        let dev_path = dev_root.join(&file_name);
        let mut x = None;
        let mut y = None;
        let mut pressure = None;
        let mut has_pen_tool = false;

        if let Ok(device) = Device::open(&dev_path) {
            if let Some(keys) = device.supported_keys() {
                has_pen_tool = keys.contains(KeyCode::BTN_TOOL_PEN);
            }
            if let Ok(absinfo) = device.get_absinfo() {
                for (axis, info) in absinfo {
                    let details = AxisInfo {
                        min: info.minimum(),
                        max: info.maximum(),
                        value: info.value(),
                    };
                    match axis {
                        AbsoluteAxisCode::ABS_X => x = Some(details),
                        AbsoluteAxisCode::ABS_Y => y = Some(details),
                        AbsoluteAxisCode::ABS_PRESSURE => pressure = Some(details),
                        _ => {}
                    }
                }
            }
        }

        let mut score = 0;
        if has_pen_tool {
            score += 100;
        }
        if x.is_some() {
            score += 80;
        }
        if device_name.to_ascii_lowercase().contains("pen") {
            score += 50;
        }
        if device_name.to_ascii_lowercase().contains("wacom") {
            score += 30;
        }
        if vendor == Some(0x056a) {
            score += 20;
        }

        devices.push(DeviceInfo {
            path: dev_path,
            name: device_name,
            vendor,
            product,
            x,
            y,
            pressure,
            has_pen_tool,
            score,
        });
    }

    devices.sort_by_key(|device| std::cmp::Reverse(device.score));
    devices
}

/// Selects the pen device, either by explicit path or by highest score.
pub fn select_pen(
    devices: &[DeviceInfo],
    explicit: Option<&Path>,
) -> Result<DeviceInfo, DeviceError> {
    if let Some(path) = explicit {
        if let Some(found) = devices.iter().find(|device| device.path == path) {
            return Ok(found.clone());
        }
        return inspect(path);
    }

    devices
        .iter()
        .filter(|device| device.is_pen())
        .max_by_key(|device| device.score)
        .cloned()
        .ok_or_else(|| {
            DeviceError::NotFound(
                "no Wacom pen device found; check that the tablet is connected and \
                 that you are in the 'input' group"
                    .to_string(),
            )
        })
}

/// Finds the ExpressKey pad that belongs to the same tablet.
pub fn find_pad<'a>(devices: &'a [DeviceInfo], pen: &DeviceInfo) -> Option<&'a DeviceInfo> {
    devices.iter().find(|candidate| {
        if candidate.path == pen.path || candidate.is_pen() {
            return false;
        }
        let same_ids = candidate.vendor == pen.vendor && candidate.product == pen.product;
        let name = candidate.name.to_ascii_lowercase();
        let looks_like_pad = name.contains("pad") || name.contains("expresskey");
        same_ids && looks_like_pad
    })
}

/// Opens an explicit device path, even if it is not in the scanned list.
pub fn inspect(path: &Path) -> Result<DeviceInfo, DeviceError> {
    let device = Device::open(path)?;
    let name = device.name().unwrap_or("Unknown").to_string();
    let id = device.input_id();

    let mut x = None;
    let mut y = None;
    let mut pressure = None;
    let mut has_pen_tool = false;

    if let Some(keys) = device.supported_keys() {
        has_pen_tool = keys.contains(KeyCode::BTN_TOOL_PEN);
    }
    if let Ok(absinfo) = device.get_absinfo() {
        for (axis, info) in absinfo {
            let details = AxisInfo {
                min: info.minimum(),
                max: info.maximum(),
                value: info.value(),
            };
            match axis {
                AbsoluteAxisCode::ABS_X => x = Some(details),
                AbsoluteAxisCode::ABS_Y => y = Some(details),
                AbsoluteAxisCode::ABS_PRESSURE => pressure = Some(details),
                _ => {}
            }
        }
    }

    Ok(DeviceInfo {
        path: path.to_path_buf(),
        name,
        vendor: Some(id.vendor()),
        product: Some(id.product()),
        x,
        y,
        pressure,
        has_pen_tool,
        score: 100,
    })
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
}

fn read_hex(path: &Path) -> Option<u16> {
    let text = read_trimmed(path)?;
    let text = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(&text);
    u16::from_str_radix(text, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pen() -> DeviceInfo {
        DeviceInfo {
            path: PathBuf::from("/dev/input/event15"),
            name: "Wacom Intuos M Pen".to_string(),
            vendor: Some(0x056a),
            product: Some(0x0375),
            x: Some(AxisInfo {
                min: 0,
                max: 21600,
                value: 0,
            }),
            y: Some(AxisInfo {
                min: 0,
                max: 13500,
                value: 0,
            }),
            pressure: Some(AxisInfo {
                min: 0,
                max: 4095,
                value: 0,
            }),
            has_pen_tool: true,
            score: 280,
        }
    }

    fn pad() -> DeviceInfo {
        DeviceInfo {
            path: PathBuf::from("/dev/input/event16"),
            name: "Wacom Intuos M Pad".to_string(),
            vendor: Some(0x056a),
            product: Some(0x0375),
            x: None,
            y: None,
            pressure: None,
            has_pen_tool: false,
            score: 50,
        }
    }

    #[test]
    fn selects_the_pen_over_the_pad() {
        let devices = vec![pad(), pen()];
        let selected = select_pen(&devices, None).unwrap();
        assert_eq!(selected.path, PathBuf::from("/dev/input/event15"));
    }

    #[test]
    fn finds_the_companion_pad() {
        let pen = pen();
        let devices = vec![pen.clone(), pad()];
        let found = find_pad(&devices, &pen).unwrap();
        assert_eq!(found.path, PathBuf::from("/dev/input/event16"));
    }

    #[test]
    fn reports_missing_pen() {
        let devices = vec![pad()];
        assert!(select_pen(&devices, None).is_err());
    }

    #[test]
    fn virtual_and_non_wacom_devices_are_not_mistaken_for_tablets() {
        let ydotool = DeviceInfo {
            path: PathBuf::from("/dev/input/event14"),
            name: "ydotoold virtual device".to_string(),
            vendor: Some(0x1),
            product: Some(0x1),
            x: None,
            y: None,
            pressure: None,
            has_pen_tool: true,
            score: 100,
        };
        assert!(!ydotool.is_pen());

        let touchpad = DeviceInfo {
            path: PathBuf::from("/dev/input/event8"),
            name: "ELAN06FA:00 04F3:31BE Touchpad".to_string(),
            vendor: Some(0x04f3),
            product: Some(0x31be),
            x: Some(AxisInfo {
                min: 0,
                max: 3217,
                value: 0,
            }),
            y: None,
            pressure: None,
            has_pen_tool: false,
            score: 80,
        };
        assert!(!touchpad.is_pad());
    }

    #[test]
    fn parses_sysfs_hex() {
        let dir = std::env::temp_dir().join("tablet-driver-test-hex");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("vendor");
        fs::write(&path, "056a\n").unwrap();
        assert_eq!(read_hex(&path), Some(0x056a));
        let _ = fs::remove_file(path);
    }
}
