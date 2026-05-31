pub mod cloudfront;
mod commands;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod local;
pub mod model_inspector;
pub mod models;
pub mod oidc;
pub mod s3;
pub mod sftp;

use commands::disk_usage::DiskUsageState;
use commands::file::FileOpState;
use commands::search::SearchState;
use commands::sync::SyncState;
use commands::terminal::TerminalState;
use commands::watcher::WatcherState;
use s3::S3State;
use sftp::SftpState;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::menu::{
    AboutMetadataBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::Emitter;

/// Ensure common tool directories are on PATH.
///
/// macOS GUI apps launched from Finder/Spotlight get a minimal PATH
/// that excludes Homebrew and MacPorts. Prepend the usual locations
/// so child processes (`git`, `7z`, editors, etc.) can be found.
#[cfg(target_os = "macos")]
fn ensure_path() {
    let extra_dirs = ["/opt/homebrew/bin", "/usr/local/bin", "/opt/homebrew/sbin"];
    let current = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<&str> = Vec::new();
    for dir in &extra_dirs {
        if !current.split(':').any(|p| p == *dir) {
            parts.push(dir);
        }
    }
    if !parts.is_empty() {
        parts.push(&current);
        std::env::set_var("PATH", parts.join(":"));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "macos")]
    ensure_path();

    // Work around WebKitGTK compositing issues that cause a blank screen
    // on some Linux systems.
    #[cfg(target_os = "linux")]
    if std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").is_err() {
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }

    if let Err(e) = commands::keychain::init() {
        eprintln!("Failed to initialize keychain store: {e}");
    }

    tauri::Builder::default()
        .manage(WatcherState(Mutex::new(HashMap::new())))
        .manage(TerminalState(Mutex::new(HashMap::new())))
        .manage(S3State(Mutex::new(HashMap::new())))
        .manage(SftpState::default())
        .manage(SearchState(Mutex::new(HashMap::new())))
        .manage(FileOpState(Mutex::new(HashMap::new())))
        .manage(SyncState(Mutex::new(HashMap::new())))
        .manage(DiskUsageState(Mutex::new(HashMap::new())))
        .plugin(tauri_plugin_drag::init())
        .setup(|app| {
            let mut targets = vec![tauri_plugin_log::Target::new(
                tauri_plugin_log::TargetKind::LogDir { file_name: None },
            )];
            if cfg!(debug_assertions) {
                targets.push(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ));
                targets.push(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Webview,
                ));
            }
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .targets(targets)
                    .level(log::LevelFilter::Info)
                    .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(3))
                    .max_file_size(5_000_000)
                    .build(),
            )?;

            // Set dock icon programmatically (needed for dev mode on macOS)
            #[cfg(target_os = "macos")]
            {
                use objc2::{AnyThread, MainThreadMarker};
                use objc2_app_kit::{NSApplication, NSImage};
                use objc2_foundation::NSData;

                let icon_bytes = include_bytes!("../icons/icon.png");
                let data = NSData::with_bytes(icon_bytes);
                if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
                    if let Some(mtm) = MainThreadMarker::new() {
                        let ns_app = NSApplication::sharedApplication(mtm);
                        unsafe {
                            ns_app.setApplicationIconImage(Some(&image));
                        }
                    }
                }
            }

            // ── Native menu bar ──────────────────────────────────────────
            let handle = app.handle();

            // Furman (app) menu
            let about_meta = AboutMetadataBuilder::new()
                .name(Some("Furman"))
                .version(Some(env!("CARGO_PKG_VERSION")))
                .comments(Some("Dual-pane file manager"))
                .license(Some("GPL-3.0-only"))
                .authors(Some(vec!["Bartosz Fenski".into()]))
                .build();
            let furman_menu = SubmenuBuilder::new(handle, "Furman")
                .item(&PredefinedMenuItem::about(handle, Some("About Furman"), Some(about_meta))?)
                .separator()
                .item(&MenuItemBuilder::with_id("preferences", "Preferences…").accelerator("CmdOrCtrl+,").build(handle)?)
                .separator()
                .item(&MenuItemBuilder::with_id("quit", "Quit Furman").accelerator("CmdOrCtrl+Q").build(handle)?)
                .build()?;

            // File menu
            let file_menu = SubmenuBuilder::new(handle, "File")
                .item(&MenuItemBuilder::with_id("mkdir", "New Folder").accelerator("CmdOrCtrl+N").build(handle)?)
                .item(&MenuItemBuilder::with_id("rename", "Rename").accelerator("CmdOrCtrl+R").build(handle)?)
                .item(&MenuItemBuilder::with_id("delete", "Delete").accelerator("CmdOrCtrl+Backspace").build(handle)?)
                .separator()
                .item(&MenuItemBuilder::with_id("view", "View").accelerator("CmdOrCtrl+3").build(handle)?)
                .item(&MenuItemBuilder::with_id("edit", "Edit").accelerator("CmdOrCtrl+E").build(handle)?)
                .separator()
                .item(&MenuItemBuilder::with_id("search", "Search…").accelerator("CmdOrCtrl+F").build(handle)?)
                .item(&MenuItemBuilder::with_id("disk-usage", "Disk Usage…").build(handle)?)
                .separator()
                .item(&MenuItemBuilder::with_id("properties", "Properties").accelerator("CmdOrCtrl+I").build(handle)?)
                .build()?;

            // Edit menu
            let edit_menu = SubmenuBuilder::new(handle, "Edit")
                .item(&MenuItemBuilder::with_id("copy", "Copy to Panel").accelerator("CmdOrCtrl+C").build(handle)?)
                .item(&MenuItemBuilder::with_id("move", "Move to Panel").accelerator("CmdOrCtrl+M").build(handle)?)
                .separator()
                .item(&MenuItemBuilder::with_id("clipboard-copy", "Clipboard Copy").accelerator("CmdOrCtrl+Shift+C").build(handle)?)
                .item(&MenuItemBuilder::with_id("clipboard-cut", "Clipboard Cut").accelerator("CmdOrCtrl+Shift+X").build(handle)?)
                .item(&MenuItemBuilder::with_id("clipboard-paste", "Clipboard Paste").accelerator("CmdOrCtrl+Shift+V").build(handle)?)
                .separator()
                .item(&MenuItemBuilder::with_id("select-all", "Select All").build(handle)?)
                .item(&MenuItemBuilder::with_id("undo", "Undo").accelerator("CmdOrCtrl+Z").build(handle)?)
                .separator()
                .item(&PredefinedMenuItem::cut(handle, Some("Cut"))?)
                .item(&PredefinedMenuItem::paste(handle, Some("Paste"))?)
                .build()?;

            // View menu
            let view_menu = SubmenuBuilder::new(handle, "View")
                .item(&MenuItemBuilder::with_id("toggle-sidebar", "Toggle Sidebar").accelerator("CmdOrCtrl+B").build(handle)?)
                .item(&MenuItemBuilder::with_id("toggle-layout", "Toggle Single/Dual").accelerator("CmdOrCtrl+P").build(handle)?)
                .item(&MenuItemBuilder::with_id("toggle-preview", "Toggle Preview").accelerator("Alt+P").build(handle)?)
                .item(&MenuItemBuilder::with_id("toggle-theme", "Toggle Theme").accelerator("CmdOrCtrl+Shift+L").build(handle)?)
                .separator()
                .item(&MenuItemBuilder::with_id("refresh", "Refresh").accelerator("CmdOrCtrl+Shift+R").build(handle)?)
                .item(&MenuItemBuilder::with_id("swap-panels", "Swap Panels").build(handle)?)
                .item(&MenuItemBuilder::with_id("equal-panels", "Equal Panels").build(handle)?)
                .item(&MenuItemBuilder::with_id("compare", "Compare Dirs").accelerator("CmdOrCtrl+Shift+D").build(handle)?)
                .build()?;

            // Go menu
            let go_menu = SubmenuBuilder::new(handle, "Go")
                .item(&MenuItemBuilder::with_id("connect", "Connect…").accelerator("CmdOrCtrl+S").build(handle)?)
                .separator()
                .item(&MenuItemBuilder::with_id("go-home", "Go Home").build(handle)?)
                .item(&MenuItemBuilder::with_id("go-parent", "Go Parent").accelerator("CmdOrCtrl+Up").build(handle)?)
                .separator()
                .item(&MenuItemBuilder::with_id("history-back", "History Back").accelerator("Alt+Left").build(handle)?)
                .item(&MenuItemBuilder::with_id("history-forward", "History Forward").accelerator("Alt+Right").build(handle)?)
                .build()?;

            // Terminal menu
            let terminal_menu = SubmenuBuilder::new(handle, "Terminal")
                .item(&MenuItemBuilder::with_id("terminal-bottom", "Bottom Terminal").accelerator("CmdOrCtrl+T").build(handle)?)
                .item(&MenuItemBuilder::with_id("terminal-inpane", "In-Pane Terminal").accelerator("CmdOrCtrl+Shift+T").build(handle)?)
                .item(&MenuItemBuilder::with_id("terminal-quake", "Quake Console").accelerator("CmdOrCtrl+`").build(handle)?)
                .build()?;

            // Window menu
            let window_menu = SubmenuBuilder::new(handle, "Window")
                .item(&PredefinedMenuItem::minimize(handle, Some("Minimize"))?)
                .separator()
                .item(&MenuItemBuilder::with_id("new-tab", "New Tab").accelerator("CmdOrCtrl+Alt+T").build(handle)?)
                .item(&MenuItemBuilder::with_id("close-tab", "Close Tab").accelerator("CmdOrCtrl+Alt+W").build(handle)?)
                .separator()
                .item(&MenuItemBuilder::with_id("toggle-transfers", "Toggle Transfers").accelerator("CmdOrCtrl+J").build(handle)?)
                .item(&MenuItemBuilder::with_id("sync", "Sync…").accelerator("CmdOrCtrl+Y").build(handle)?)
                .build()?;

            // Help menu
            let help_menu = SubmenuBuilder::new(handle, "Help")
                .item(&MenuItemBuilder::with_id("shortcuts", "Keyboard Shortcuts").accelerator("CmdOrCtrl+/").build(handle)?)
                .item(&MenuItemBuilder::with_id("github", "GitHub").build(handle)?)
                .separator()
                .item(&MenuItemBuilder::with_id("command-palette", "Command Palette").accelerator("CmdOrCtrl+Shift+P").build(handle)?)
                .build()?;

            let menu = MenuBuilder::new(handle)
                .item(&furman_menu)
                .item(&file_menu)
                .item(&edit_menu)
                .item(&view_menu)
                .item(&go_menu)
                .item(&terminal_menu)
                .item(&window_menu)
                .item(&help_menu)
                .build()?;

            app.set_menu(menu)?;

            app.on_menu_event(move |app_handle, event| {
                let _ = app_handle.emit("menu-action", event.id().as_ref());
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // directory commands
            commands::directory::list_directory,
            commands::directory::list_directory_streamed,
            commands::directory::create_directory,
            commands::directory::get_directory_size,
            // file commands
            commands::file::copy_files,
            commands::file::move_files,
            commands::file::delete_files,
            commands::file::rename_file,
            commands::file::check_conflicts,
            commands::file::delete_files_undoable,
            commands::file::restore_from_trash,
            commands::file::cancel_file_operation,
            commands::file::pause_file_operation,
            // metadata / content commands
            commands::metadata::read_file_text,
            commands::metadata::write_file_text,
            commands::metadata::read_file_binary,
            commands::metadata::set_permissions,
            commands::metadata::open_file_default,
            commands::metadata::open_url,
            commands::metadata::open_in_editor,
            commands::metadata::get_file_properties,
            commands::metadata::get_log_path,
            commands::metadata::batch_chmod,
            commands::metadata::batch_touch,
            // volume commands
            commands::volumes::list_volumes,
            commands::volumes::eject_volume,
            // watcher commands
            commands::watcher::watch_directory,
            commands::watcher::unwatch_directory,
            // terminal commands
            commands::terminal::terminal_spawn,
            commands::terminal::terminal_write,
            commands::terminal::terminal_resize,
            commands::terminal::terminal_close,
            // s3 commands
            commands::s3::s3_connect,
            commands::s3::s3_check_credentials,
            commands::s3::s3_list_buckets,
            commands::s3::s3_disconnect,
            commands::s3::s3_list_objects,
            commands::s3::s3_download,
            commands::s3::s3_upload,
            commands::s3::s3_copy_objects,
            commands::s3::s3_delete_objects,
            commands::s3::s3_head_object,
            commands::s3::s3_create_folder,
            commands::s3::s3_rename_object,
            commands::s3::s3_search_objects,
            commands::s3::s3_presign_url,
            commands::s3::s3_download_temp,
            commands::s3::s3_put_text,
            commands::s3::s3_change_storage_class,
            commands::s3::s3_restore_object,
            commands::s3::s3_list_object_versions,
            commands::s3::s3_download_version,
            commands::s3::s3_restore_version,
            commands::s3::s3_delete_version,
            commands::s3::s3_create_bucket,
            commands::s3::s3_delete_bucket,
            commands::s3::s3_get_bucket_versioning,
            commands::s3::s3_put_bucket_versioning,
            commands::s3::s3_get_bucket_encryption,
            commands::s3::s3_get_object_metadata,
            commands::s3::s3_put_object_metadata,
            commands::s3::s3_get_object_tags,
            commands::s3::s3_put_object_tags,
            commands::s3::s3_get_bucket_tags,
            commands::s3::s3_put_bucket_tags,
            commands::s3::s3_list_multipart_uploads,
            commands::s3::s3_abort_multipart_upload,
            commands::s3::s3_get_bucket_lifecycle,
            commands::s3::s3_put_bucket_lifecycle,
            commands::s3::s3_get_bucket_cors,
            commands::s3::s3_put_bucket_cors,
            commands::s3::s3_bulk_change_storage_class,
            commands::s3::s3_get_public_access_block,
            commands::s3::s3_put_public_access_block,
            commands::s3::s3_get_bucket_policy,
            commands::s3::s3_put_bucket_policy,
            commands::s3::s3_get_bucket_acl,
            commands::s3::s3_put_bucket_acl,
            commands::s3::s3_put_bucket_encryption,
            commands::s3::s3_get_bucket_website,
            commands::s3::s3_put_bucket_website,
            commands::s3::s3_get_request_payment,
            commands::s3::s3_put_request_payment,
            commands::s3::s3_get_bucket_ownership,
            commands::s3::s3_put_bucket_ownership,
            commands::s3::s3_get_bucket_logging,
            commands::s3::s3_put_bucket_logging,
            commands::s3::s3_set_bandwidth_limit,
            commands::s3::s3_set_multipart_config,
            commands::s3::s3_list_kms_keys,
            commands::s3::s3_upload_encrypted,
            commands::s3::s3_is_object_encrypted,
            commands::s3::s3_get_object_lock_configuration,
            commands::s3::s3_put_object_lock_configuration,
            commands::s3::s3_get_object_retention,
            commands::s3::s3_put_object_retention,
            commands::s3::s3_get_object_legal_hold,
            commands::s3::s3_put_object_legal_hold,
            commands::s3::s3_bulk_put_object_retention,
            commands::s3::s3_batch_put_object_metadata,
            commands::s3::s3_batch_put_object_tags,
            // inventory commands
            commands::s3::s3_list_inventory_configurations,
            commands::s3::s3_put_inventory_configuration,
            commands::s3::s3_delete_inventory_configuration,
            // replication commands
            commands::s3::s3_get_replication_configuration,
            commands::s3::s3_put_replication_configuration,
            commands::s3::s3_delete_replication_configuration,
            // notification commands
            commands::s3::s3_get_notification_configuration,
            commands::s3::s3_put_notification_configuration,
            // access point commands
            commands::s3::s3_list_access_points,
            commands::s3::s3_get_access_point,
            commands::s3::s3_create_access_point,
            commands::s3::s3_delete_access_point,
            commands::s3::s3_get_access_point_policy,
            commands::s3::s3_put_access_point_policy,
            commands::s3::s3_delete_access_point_policy,
            // sftp commands
            commands::sftp::sftp_connect,
            commands::sftp::sftp_disconnect,
            commands::sftp::sftp_list_objects,
            commands::sftp::sftp_delete,
            commands::sftp::sftp_rename,
            commands::sftp::sftp_create_folder,
            commands::sftp::sftp_download,
            commands::sftp::sftp_upload,
            commands::sftp::sftp_download_temp,
            commands::sftp::sftp_put_text,
            commands::sftp::sftp_head,
            commands::sftp::sftp_batch_chmod,
            // cloudfront commands
            commands::cloudfront::cf_list_distributions,
            commands::cloudfront::cf_get_distribution,
            commands::cloudfront::cf_create_distribution,
            commands::cloudfront::cf_update_distribution,
            commands::cloudfront::cf_delete_distribution,
            commands::cloudfront::cf_create_invalidation,
            commands::cloudfront::cf_list_invalidations,
            // oidc commands
            commands::oidc::oidc_start_auth,
            commands::oidc::oidc_refresh,
            // archive commands
            commands::archive::list_archive,
            commands::archive::extract_archive,
            commands::archive::extract_archive_to_temp,
            // search commands
            commands::search::search_files,
            commands::search::cancel_search,
            // disk usage commands
            commands::disk_usage::analyze_disk_usage,
            commands::disk_usage::cancel_disk_usage,
            // sync commands
            commands::sync::sync_diff,
            commands::sync::cancel_sync,
            // keychain commands
            commands::keychain::keychain_set,
            commands::keychain::keychain_get,
            commands::keychain::keychain_delete,
            // git commands
            commands::git::git_repo_info,
            commands::git::git_pull,
            commands::git::git_list_branches,
            commands::git::git_checkout,
            // model inspector
            commands::model::inspect_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
