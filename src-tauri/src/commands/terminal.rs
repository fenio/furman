use crate::models::FmError;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

/// Managed state holding one PTY session per terminal id.
pub struct TerminalState(pub Mutex<HashMap<String, TerminalSession>>);

pub struct TerminalSession {
    _master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TerminalOutput {
    pub id: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TerminalExit {
    pub id: String,
    pub code: Option<u32>,
}

/// Create a temp directory with a .zshenv that sets up OSC 7 directory tracking,
/// then restores the user's original ZDOTDIR so all other startup files load normally.
fn ensure_zsh_osc7_init() -> Result<PathBuf, FmError> {
    let dir = std::env::temp_dir().join("furman-shell-init");
    std::fs::create_dir_all(&dir)
        .map_err(|e| FmError::Other(format!("create shell init dir: {e}")))?;

    let zshenv = r#"# Furman file manager — OSC 7 directory tracking
# Restore original ZDOTDIR so all other zsh startup files load correctly
if [ -n "${__FM_ZDOTDIR+x}" ]; then
  ZDOTDIR="$__FM_ZDOTDIR"
  unset __FM_ZDOTDIR
else
  unset ZDOTDIR
fi
# Source the user's original .zshenv
[ -f "${ZDOTDIR:-$HOME}/.zshenv" ] && source "${ZDOTDIR:-$HOME}/.zshenv"
# Report cwd via OSC 7 escape sequence
__furman_report_cwd() {
  [ "$__furman_last_dir" = "$PWD" ] && return
  __furman_last_dir="$PWD"
  printf '\033]7;file://%s%s\007' "${HOST:-$(hostname -s)}" "$PWD"
}
chpwd_functions+=(__furman_report_cwd)
precmd_functions+=(__furman_report_cwd)
"#;

    std::fs::write(dir.join(".zshenv"), zshenv)
        .map_err(|e| FmError::Other(format!("write .zshenv: {e}")))?;

    Ok(dir)
}

/// Spawn a new PTY shell session.
#[tauri::command]
pub fn terminal_spawn(
    id: String,
    cwd: String,
    app_handle: AppHandle,
    state: State<'_, TerminalState>,
) -> Result<(), FmError> {
    let pty_system = native_pty_system();

    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| FmError::Other(format!("openpty: {e}")))?;

    // Detect shell from $SHELL, fallback to /bin/zsh
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    let mut cmd = CommandBuilder::new(&shell);
    cmd.arg("-l"); // login shell
    cmd.cwd(&cwd);

    // Set up OSC 7 directory tracking so the file panels can follow terminal cwd
    if shell.contains("zsh") {
        if let Ok(init_dir) = ensure_zsh_osc7_init() {
            // Preserve original ZDOTDIR if set
            if let Ok(orig) = std::env::var("ZDOTDIR") {
                cmd.env("__FM_ZDOTDIR", &orig);
            }
            cmd.env("ZDOTDIR", init_dir);
        }
    } else if shell.contains("bash") {
        // Best effort for bash: set PROMPT_COMMAND (may be overridden by .bashrc)
        let existing = std::env::var("PROMPT_COMMAND").unwrap_or_default();
        cmd.env("PROMPT_COMMAND", format!(
            r#"__furman_report_cwd(){{ [ "$__furman_last_dir" = "$PWD" ]&&return;__furman_last_dir="$PWD";printf '\033]7;file://%s%s\007' "$(hostname -s)" "$PWD";}};__furman_report_cwd;{existing}"#
        ));
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| FmError::Other(format!("spawn: {e}")))?;

    // Drop slave — we only need the master side
    drop(pair.slave);

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| FmError::Other(format!("take_writer: {e}")))?;

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| FmError::Other(format!("try_clone_reader: {e}")))?;

    // Spawn reader thread that emits "terminal-output" events.
    // When the reader hits EOF the shell has exited — emit "terminal-exit".
    let read_id = id.clone();
    let read_handle = app_handle.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let payload = TerminalOutput {
                        id: read_id.clone(),
                        data: text,
                    };
                    let _ = read_handle.emit("terminal-output", &payload);
                }
                Err(_) => break,
            }
        }
        // Shell exited
        let exit_payload = TerminalExit {
            id: read_id,
            code: None,
        };
        let _ = read_handle.emit("terminal-exit", &exit_payload);
    });

    // Store the session
    let session = TerminalSession {
        _master: pair.master,
        writer,
        child,
    };

    let mut map = state
        .0
        .lock()
        .map_err(|e| FmError::Other(format!("lock poisoned: {e}")))?;
    map.insert(id, session);

    Ok(())
}

/// Write data (keystrokes) to a PTY session.
#[tauri::command]
pub fn terminal_write(
    id: String,
    data: String,
    state: State<'_, TerminalState>,
) -> Result<(), FmError> {
    let mut map = state
        .0
        .lock()
        .map_err(|e| FmError::Other(format!("lock poisoned: {e}")))?;

    let session = map
        .get_mut(&id)
        .ok_or_else(|| FmError::NotFound(format!("no terminal with id: {id}")))?;

    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| FmError::Other(format!("write: {e}")))?;

    session
        .writer
        .flush()
        .map_err(|e| FmError::Other(format!("flush: {e}")))?;

    Ok(())
}

/// Resize a PTY session.
#[tauri::command]
pub fn terminal_resize(
    id: String,
    cols: u16,
    rows: u16,
    state: State<'_, TerminalState>,
) -> Result<(), FmError> {
    let map = state
        .0
        .lock()
        .map_err(|e| FmError::Other(format!("lock poisoned: {e}")))?;

    let session = map
        .get(&id)
        .ok_or_else(|| FmError::NotFound(format!("no terminal with id: {id}")))?;

    session
        ._master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| FmError::Other(format!("resize: {e}")))?;

    Ok(())
}

/// Close (kill) a PTY session and remove it from state.
#[tauri::command]
pub fn terminal_close(
    id: String,
    state: State<'_, TerminalState>,
) -> Result<(), FmError> {
    let mut map = state
        .0
        .lock()
        .map_err(|e| FmError::Other(format!("lock poisoned: {e}")))?;

    if let Some(mut session) = map.remove(&id) {
        let _ = session.child.kill();
    }

    Ok(())
}
