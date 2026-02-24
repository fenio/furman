mod commands;
mod models;

use commands::s3::S3State;
use commands::search::SearchState;
use commands::terminal::TerminalState;
use commands::watcher::WatcherState;
use std::collections::HashMap;
use std::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(WatcherState(Mutex::new(HashMap::new())))
        .manage(TerminalState(Mutex::new(HashMap::new())))
        .manage(S3State(Mutex::new(HashMap::new())))
        .manage(SearchState(Mutex::new(HashMap::new())))
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

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
            // metadata / content commands
            commands::metadata::read_file_text,
            commands::metadata::write_file_text,
            commands::metadata::read_file_binary,
            commands::metadata::set_permissions,
            commands::metadata::open_file_default,
            commands::metadata::open_in_editor,
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
            commands::s3::s3_disconnect,
            commands::s3::s3_list_objects,
            commands::s3::s3_download,
            commands::s3::s3_upload,
            commands::s3::s3_copy_objects,
            commands::s3::s3_delete_objects,
            // archive commands
            commands::archive::list_archive,
            // search commands
            commands::search::search_files,
            commands::search::cancel_search,
            // keychain commands
            commands::keychain::keychain_set,
            commands::keychain::keychain_get,
            commands::keychain::keychain_delete,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
