//! Tray icon management: menu building, icon animation, and tray construction.

use aztec_accelerator::versions;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::AppHandle;

// Tray icon variants (44x44 RGBA PNGs, macOS template mode)
static ICON_IDLE: &[u8] = include_bytes!("../icons/tray-idle.png");
static ICON_PROVING: [&[u8]; 24] = [
    include_bytes!("../icons/tray-proving-1.png"),
    include_bytes!("../icons/tray-proving-2.png"),
    include_bytes!("../icons/tray-proving-3.png"),
    include_bytes!("../icons/tray-proving-4.png"),
    include_bytes!("../icons/tray-proving-5.png"),
    include_bytes!("../icons/tray-proving-6.png"),
    include_bytes!("../icons/tray-proving-7.png"),
    include_bytes!("../icons/tray-proving-8.png"),
    include_bytes!("../icons/tray-proving-9.png"),
    include_bytes!("../icons/tray-proving-10.png"),
    include_bytes!("../icons/tray-proving-11.png"),
    include_bytes!("../icons/tray-proving-12.png"),
    include_bytes!("../icons/tray-proving-13.png"),
    include_bytes!("../icons/tray-proving-14.png"),
    include_bytes!("../icons/tray-proving-15.png"),
    include_bytes!("../icons/tray-proving-16.png"),
    include_bytes!("../icons/tray-proving-17.png"),
    include_bytes!("../icons/tray-proving-18.png"),
    include_bytes!("../icons/tray-proving-19.png"),
    include_bytes!("../icons/tray-proving-20.png"),
    include_bytes!("../icons/tray-proving-21.png"),
    include_bytes!("../icons/tray-proving-22.png"),
    include_bytes!("../icons/tray-proving-23.png"),
    include_bytes!("../icons/tray-proving-24.png"),
];

/// Build a "Versions" submenu listing the bundled + cached bb versions.
fn build_versions_submenu(
    app: &AppHandle,
    bundled_version: &str,
) -> Result<tauri::menu::Submenu<tauri::Wry>, Box<dyn std::error::Error>> {
    let mut builder = SubmenuBuilder::with_id(app, "versions", "Versions");

    // Bundled version always first
    let bundled_item = MenuItemBuilder::with_id(
        format!("version_{bundled_version}"),
        format!("{bundled_version} (bundled)"),
    )
    .enabled(false)
    .build(app)?;
    builder = builder.item(&bundled_item);

    // Cached versions (exclude bundled to avoid duplicate)
    let cached = versions::list_cached_versions();
    for v in &cached {
        if v != bundled_version {
            let item = MenuItemBuilder::with_id(format!("version_{v}"), v.as_str())
                .enabled(false)
                .build(app)?;
            builder = builder.item(&item);
        }
    }

    Ok(builder.build()?)
}

/// Build the tray menu. Used both for initial setup and for rebuilding when versions change.
/// The `status` item is passed in because it's shared state (text updated by callbacks).
pub fn build_tray_menu(
    app: &AppHandle,
    dev_mode: bool,
    bundled_version: &str,
    status: &tauri::menu::MenuItem<tauri::Wry>,
) -> Result<tauri::menu::Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    let settings = MenuItemBuilder::with_id("settings", "Settings").build(app)?;
    let app_version = env!("CARGO_PKG_VERSION");
    let aztec_bb_version = env!("AZTEC_BB_VERSION");
    let version_text = MenuItemBuilder::with_id(
        "version_info",
        format!("v{app_version} · Aztec {aztec_bb_version}"),
    )
    .enabled(false)
    .build(app)?;
    let github = MenuItemBuilder::with_id("open_github", "GitHub").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let separator = PredefinedMenuItem::separator(app)?;

    if dev_mode {
        let versions_submenu = build_versions_submenu(app, bundled_version)?;
        let show_logs = MenuItemBuilder::with_id("show_logs", "Show Logs").build(app)?;
        Ok(MenuBuilder::new(app)
            .items(&[
                status,
                &versions_submenu,
                &show_logs,
                &settings,
                &separator,
                &version_text,
                &github,
                &quit,
            ])
            .build()?)
    } else {
        // Production mode: no status item or Versions submenu in the menu.
        // The status MenuItem still exists and is updated by on_status — this is
        // intentional because on_status also sets the tray tooltip, which IS visible.
        Ok(MenuBuilder::new(app)
            .items(&[&settings, &separator, &version_text, &github, &quit])
            .build()?)
    }
}

/// Build the tray icon with the idle icon and initial menu.
/// Encapsulates icon creation so ICON_IDLE stays private to this module.
pub fn build_tray_icon(
    app: &tauri::App,
    menu: &tauri::menu::Menu<tauri::Wry>,
    on_menu_event: impl Fn(&AppHandle, tauri::menu::MenuEvent) + Send + Sync + 'static,
) -> Result<TrayIcon, Box<dyn std::error::Error>> {
    let tray_icon = tauri::image::Image::from_bytes(ICON_IDLE).expect("failed to load tray icon");

    Ok(TrayIconBuilder::new()
        .icon(tray_icon)
        .icon_as_template(true)
        .tooltip("Aztec Accelerator")
        .menu(menu)
        .on_menu_event(on_menu_event)
        .build(app)?)
}

/// Start the tray icon animation loop. Pulses proving frames at 20fps when
/// `is_animating` is true, resets to idle icon when false.
///
/// This is a fire-and-forget task with no shutdown path — it runs for the
/// lifetime of the app. Both set_icon + set_icon_as_template must run in a
/// single main-thread turn to avoid a black flash between the two calls.
pub fn start_animation_loop(tray: TrayIcon, handle: AppHandle, is_animating: Arc<AtomicBool>) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        let mut frame_idx: usize = 0;
        let mut was_animating = false;
        loop {
            interval.tick().await;
            let animating = is_animating.load(Ordering::Acquire);
            if animating {
                let tray = tray.clone();
                let frame = frame_idx;
                let _ = handle.run_on_main_thread(move || {
                    if let Ok(icon) = tauri::image::Image::from_bytes(ICON_PROVING[frame]) {
                        let _ = tray.set_icon(Some(icon));
                        let _ = tray.set_icon_as_template(true);
                    }
                });
                frame_idx = (frame_idx + 1) % ICON_PROVING.len();
                was_animating = true;
            } else if was_animating {
                let tray = tray.clone();
                let _ = handle.run_on_main_thread(move || {
                    if let Ok(icon) = tauri::image::Image::from_bytes(ICON_IDLE) {
                        let _ = tray.set_icon(Some(icon));
                        let _ = tray.set_icon_as_template(true);
                    }
                });
                frame_idx = 0;
                was_animating = false;
            }
        }
    });
}
