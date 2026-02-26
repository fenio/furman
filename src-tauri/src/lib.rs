mod commands;
pub mod models;
pub mod s3;

use commands::file::FileOpState;
use s3::S3State;
use commands::search::SearchState;
use commands::sync::SyncState;
use commands::terminal::TerminalState;
use commands::watcher::WatcherState;
use std::collections::HashMap;
use std::sync::Mutex;

/// Ensure common tool directories are on PATH.
///
/// macOS GUI apps launched from Finder/Spotlight get a minimal PATH
/// that excludes Homebrew and MacPorts. Prepend the usual locations
/// so child processes (`git`, `7z`, editors, etc.) can be found.
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
    ensure_path();

    tauri::Builder::default()
        .manage(WatcherState(Mutex::new(HashMap::new())))
        .manage(TerminalState(Mutex::new(HashMap::new())))
        .manage(S3State(Mutex::new(HashMap::new())))
        .manage(SearchState(Mutex::new(HashMap::new())))
        .manage(FileOpState(Mutex::new(HashMap::new())))
        .manage(SyncState(Mutex::new(HashMap::new())))
        .setup(|app| {
            let mut targets = vec![
                tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir { file_name: None }),
            ];
            if cfg!(debug_assertions) {
                targets.push(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout));
                targets.push(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview));
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
                        unsafe { ns_app.setApplicationIconImage(Some(&image)); }
                    }
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // directory commands
            commands::directory::list_directory,
            commands::directory::create_directory,
            commands::directory::get_directory_size,
            // file commands
            commands::file::copy_files,
            commands::file::move_files,
            commands::file::delete_files,
            commands::file::rename_file,
            commands::file::check_conflicts,
            commands::file::cancel_file_operation,
            commands::file::pause_file_operation,
            // metadata / content commands
            commands::metadata::read_file_text,
            commands::metadata::write_file_text,
            commands::metadata::read_file_binary,
            commands::metadata::set_permissions,
            commands::metadata::open_file_default,
            commands::metadata::open_in_editor,
            commands::metadata::get_file_properties,
            commands::metadata::get_log_path,
            // volume commands
            commands::volumes::list_volumes,
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
            commands::s3::s3_set_bandwidth_limit,
            // archive commands
            commands::archive::list_archive,
            commands::archive::extract_archive,
            // search commands
            commands::search::search_files,
            commands::search::cancel_search,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
