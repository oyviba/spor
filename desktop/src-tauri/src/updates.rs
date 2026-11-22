//! Updating the app from inside itself (#152), with Tauri's updater plugin.
//!
//! Updates are signed with the updater's own key pair, apart from Apple's
//! signing (#62). The public half goes into the build as the
//! `SPOR_UPDATER_PUBKEY` environment variable (the release workflow sets it
//! from a repository variable); a build without it, like `pnpm tauri dev`
//! or CI's, has no updater and says so. The private half is only ever a
//! secret of the release workflow. Releases are published to GitHub, and
//! the app asks for `latest.json` among the latest release's files.
//!
//! The commands are only ever asked by the UI, which holds an install back
//! while agents are running.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_updater::UpdaterExt;

/// The public key this build trusts, if it was built with one.
fn pubkey() -> Option<&'static str> {
    option_env!("SPOR_UPDATER_PUBKEY")
        .map(str::trim)
        .filter(|k| !k.is_empty())
}

/// The updater plugin for this build, if it has a key. Registered by
/// `run`, which has no plugin at all without one.
pub(crate) fn plugin<R: Runtime>(
) -> Option<tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config>> {
    pubkey().map(|key| tauri_plugin_updater::Builder::new().pubkey(key).build())
}

#[derive(Serialize)]
pub(crate) struct Available {
    version: String,
    notes: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct UpdateCheck {
    /// Whether this build can update itself.
    enabled: bool,
    current: String,
    available: Option<Available>,
}

/// Ask the release feed whether a newer version is out.
#[tauri::command]
pub(crate) async fn update_check(handle: AppHandle) -> Result<UpdateCheck, String> {
    let current = handle.package_info().version.to_string();
    if pubkey().is_none() {
        return Ok(UpdateCheck {
            enabled: false,
            current,
            available: None,
        });
    }
    let update = handle
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    Ok(UpdateCheck {
        enabled: true,
        current,
        available: update.map(|u| Available {
            version: u.version,
            notes: u.body,
        }),
    })
}

#[derive(Serialize, Clone)]
struct Progress {
    downloaded: usize,
    total: Option<u64>,
}

/// Download the newer version, verify its signature, install it and
/// restart. Sends `update-progress` while it downloads. Only returns on
/// failure (or when there's nothing newer).
#[tauri::command]
pub(crate) async fn update_install(handle: AppHandle) -> Result<(), String> {
    if pubkey().is_none() {
        return Err("This build can't update itself".into());
    }
    let update = handle
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Already up to date")?;
    let mut downloaded = 0;
    let to = handle.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk;
                let _ = to.emit("update-progress", Progress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    handle.restart()
}
