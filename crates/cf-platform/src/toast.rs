//! Windows toast notifications.
//!
//! An unpackaged app needs an App User Model ID registered under
//! `HKCU\Software\Classes\AppUserModelId\<id>` (display name) before Windows shows its
//! toasts; [`show`] writes that per-user key once per process. Buttons use *protocol*
//! activation with `file:` URIs, so the shell opens the image or folder itself — clicks work
//! even after the capture helper has exited, and no COM activator is needed.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::OnceLock;

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

use crate::com;

/// The App User Model ID toasts are shown under.
pub const APP_ID: &str = "ClipFrame.App";

/// What a toast says and offers.
#[derive(Debug, Clone, Default)]
pub struct Toast<'a> {
    /// First line.
    pub title: &'a str,
    /// Second line.
    pub body: &'a str,
    /// A picture shown in the toast (a PNG on disk).
    pub image: Option<&'a Path>,
    /// Opened when the toast or its *Open* button is clicked.
    pub open: Option<&'a Path>,
    /// Opened by *Show in folder*.
    pub folder: Option<&'a Path>,
}

/// Why a toast could not be shown.
#[derive(Debug, thiserror::Error)]
pub enum ToastError {
    /// The app id could not be registered.
    #[error("could not register the notification app id (error {0})")]
    Register(u32),
    /// A Windows Runtime call failed.
    #[error("toast failed: {0}")]
    Windows(#[from] windows::core::Error),
}

/// Shows `toast`. Safe to call from any thread.
///
/// # Errors
/// [`ToastError`] when registration or a WinRT call fails (e.g. notifications are
/// disabled by policy).
pub fn show(toast: &Toast<'_>) -> Result<(), ToastError> {
    static REGISTERED: OnceLock<Result<(), u32>> = OnceLock::new();
    REGISTERED
        .get_or_init(|| register_app_id(None))
        .map_err(ToastError::Register)?;
    com::keep_mta()?;
    let document = XmlDocument::new()?;
    document.LoadXml(&HSTRING::from(xml(toast)))?;
    let notification = ToastNotification::CreateToastNotification(&document)?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?;
    notifier.Show(&notification)?;
    Ok(())
}

/// The toast's XML payload.
pub fn xml(toast: &Toast<'_>) -> String {
    let launch = toast.open.map_or_else(String::new, |path| {
        format!(
            r#" activationType="protocol" launch="{}""#,
            escape(&file_uri(path))
        )
    });
    // Images take a plain absolute path (a percent-encoded URI isn't decoded there).
    let image = toast.image.map_or_else(String::new, |path| {
        format!(r#"<image src="{}"/>"#, escape(&path.to_string_lossy()))
    });
    let actions: String = [("Open", toast.open), ("Show in folder", toast.folder)]
        .into_iter()
        .filter_map(|(label, target)| {
            target.map(|path| {
                format!(
                    r#"<action content="{label}" activationType="protocol" arguments="{}"/>"#,
                    escape(&file_uri(path))
                )
            })
        })
        .collect();
    format!(
        r#"<toast{launch}><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text>{image}</binding></visual><actions>{actions}</actions></toast>"#,
        escape(toast.title),
        escape(toast.body)
    )
}

/// `C:\a b\c.png` → `file:///C:/a%20b/c.png`.
pub fn file_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from("file:///");
    for byte in text.trim_start_matches('/').bytes() {
        if byte.is_ascii_alphanumeric() || b"/:-._~".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Registers the app id with its display name and, when given, an icon (a PNG on disk)
/// shown in the toast header. Writes `HKCU\Software\Classes\AppUserModelId\<APP_ID>`.
///
/// # Errors
/// [`ToastError::Register`] when the registry key can't be written.
pub fn register(icon: Option<&Path>) -> Result<(), ToastError> {
    register_app_id(icon).map_err(ToastError::Register)
}

fn register_app_id(icon: Option<&Path>) -> Result<(), u32> {
    let key_path = HSTRING::from(format!(r"Software\Classes\AppUserModelId\{APP_ID}"));
    let mut key = HKEY::default();
    // SAFETY: valid null-terminated strings; `key` receives a handle closed below.
    let created = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &raw mut key,
            None,
        )
    };
    if created != ERROR_SUCCESS {
        return Err(created.0);
    }
    let mut result = set_string(key, "DisplayName", "ClipFrame");
    if let Some(icon) = icon {
        result = result.and(set_string(key, "IconUri", &icon.to_string_lossy()));
    }
    // SAFETY: `key` was opened above and is closed once.
    let _ = unsafe { RegCloseKey(key) };
    result
}

fn set_string(key: HKEY, name: &str, value: &str) -> Result<(), u32> {
    let name = HSTRING::from(name);
    let data: Vec<u8> = value
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    // SAFETY: `key` is open with KEY_SET_VALUE; `name` and `data` are null-terminated
    // UTF-16 strings that outlive the call.
    let set = unsafe { RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&data)) };
    if set == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(set.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_are_percent_encoded() {
        assert_eq!(
            file_uri(Path::new(r"C:\Users\Ana Đ\Pictures\a&b.png")),
            "file:///C:/Users/Ana%20%C4%90/Pictures/a%26b.png"
        );
    }

    #[test]
    fn xml_escapes_text_and_includes_only_given_actions() {
        let toast = Toast {
            title: "Saved <1>",
            body: "a & \"b\"",
            open: Some(Path::new(r"C:\x\shot.png")),
            ..Toast::default()
        };
        let xml = xml(&toast);
        assert!(xml.contains("<text>Saved &lt;1&gt;</text><text>a &amp; &quot;b&quot;</text>"));
        assert!(xml.contains(r#"launch="file:///C:/x/shot.png""#));
        assert!(xml.contains(r#"content="Open""#));
        assert!(!xml.contains("Show in folder"));
        assert!(!xml.contains("<image"));
        let with_image = Toast {
            image: Some(Path::new(r"C:\a b\c.png")),
            ..toast
        };
        assert!(super::xml(&with_image).contains(r#"<image src="C:\a b\c.png"/>"#));
    }
}
