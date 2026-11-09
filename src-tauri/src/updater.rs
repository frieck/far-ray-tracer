//! Updates the app from the GitHub releases of frieck/far-ray-tracer.
//!
//! The app checks quietly at startup and from the "Check for Updates…" menu item. When a
//! newer version exists it asks before downloading, installs it and offers to restart.
//!
//! Two channels, picked by the running version:
//! - A stable version (3.0.0) reads `latest.json` from the latest published release
//!   (the endpoint in `tauri.conf.json`). GitHub never counts pre-releases as latest.
//! - A pre-release (3.0.0-beta.2) reads `beta.json` from the release tagged `updater`,
//!   which the release workflow refreshes each time a release is published, stable or
//!   not. Beta users get the next beta and then the stable version; once on a stable
//!   version they follow the stable channel.

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

pub const MENU_CHECK_UPDATES: &str = "check-updates";
const BETA_ENDPOINT: &str = "https://github.com/frieck/far-ray-tracer/releases/download/updater/beta.json";

/// Checks for a newer version in the background. When `quiet`, nothing is shown unless an
/// update exists (startup); otherwise "up to date" and errors are reported too (menu).
pub fn check(app: &AppHandle, quiet: bool) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match find_update(&app).await {
            Ok(Some(update)) => offer(&app, update),
            Ok(None) if !quiet => {
                let version = app.package_info().version.to_string();
                message(&app, MessageDialogKind::Info, "No Updates",
                    format!("Far Ray Tracer {version} is the newest version."));
            }
            Err(error) if !quiet => {
                message(&app, MessageDialogKind::Error, "Update Check Failed",
                    format!("Could not check for updates.\n\n{error}"));
            }
            Err(error) => eprintln!("Update check failed: {error}"),
            Ok(None) => {}
        }
    });
}

async fn find_update(app: &AppHandle) -> tauri_plugin_updater::Result<Option<tauri_plugin_updater::Update>> {
    let mut builder = app.updater_builder();
    if !app.package_info().version.pre.is_empty() {
        builder = builder.endpoints(vec![BETA_ENDPOINT.parse().expect("valid beta endpoint")])?;
    }
    builder.build()?.check().await
}

fn offer(app: &AppHandle, update: tauri_plugin_updater::Update) {
    // The release notes are Markdown written for the releases page, so the dialog leaves them out.
    let text = format!(
        "Far Ray Tracer {} is available (you have {}). Download and install it now?",
        update.version, update.current_version);
    let app = app.clone();
    app.dialog()
        .message(text)
        .title("Update Available")
        .buttons(MessageDialogButtons::OkCancelCustom("Install".into(), "Later".into()))
        .show(move |install| {
            if install {
                tauri::async_runtime::spawn(async move { install_update(app, update).await });
            }
        });
}

async fn install_update(app: AppHandle, update: tauri_plugin_updater::Update) {
    // On Windows the installer closes the app and starts the new version by itself.
    if let Err(error) = update.download_and_install(|_, _| {}, || {}).await {
        message(&app, MessageDialogKind::Error, "Update Failed",
            format!("The update could not be installed.\n\n{error}"));
        return;
    }
    let version = update.version.clone();
    let handle = app.clone();
    app.dialog()
        .message(format!("Far Ray Tracer {version} is installed. Restart now to use it?"))
        .title("Update Installed")
        .buttons(MessageDialogButtons::OkCancelCustom("Restart".into(), "Later".into()))
        .show(move |restart| {
            if restart {
                handle.restart();
            }
        });
}

fn message(app: &AppHandle, kind: MessageDialogKind, title: &str, text: String) {
    let mut dialog = app.dialog().message(text).title(title).kind(kind);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.parent(&window);
    }
    dialog.show(|_| {});
}
