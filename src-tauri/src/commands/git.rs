use crate::models::FmError;
use serde::Serialize;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct GitRepoInfo {
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
}

#[tauri::command]
pub fn git_repo_info(path: String) -> Result<Option<GitRepoInfo>, FmError> {
    // Check if inside a git work tree
    let check = Command::new("git")
        .args(["-C", &path, "rev-parse", "--is-inside-work-tree"])
        .output();
    match check {
        Ok(o) if o.status.success() => {}
        _ => return Ok(None),
    }

    // Branch name
    let branch_out = Command::new("git")
        .args(["-C", &path, "branch", "--show-current"])
        .output()
        .map_err(|e| FmError::Other(e.to_string()))?;
    let branch = String::from_utf8_lossy(&branch_out.stdout).trim().to_string();
    let branch = if branch.is_empty() {
        // Detached HEAD — show short SHA
        let head_out = Command::new("git")
            .args(["-C", &path, "rev-parse", "--short", "HEAD"])
            .output()
            .map_err(|e| FmError::Other(e.to_string()))?;
        let sha = String::from_utf8_lossy(&head_out.stdout).trim().to_string();
        if sha.is_empty() { "HEAD".to_string() } else { sha }
    } else {
        branch
    };

    // Ahead / behind
    let (ahead, behind) = match Command::new("git")
        .args(["-C", &path, "rev-list", "--count", "--left-right", "@{upstream}...HEAD"])
        .output()
    {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() == 2 {
                (
                    parts[1].parse::<u32>().unwrap_or(0),
                    parts[0].parse::<u32>().unwrap_or(0),
                )
            } else {
                (0, 0)
            }
        }
        _ => (0, 0), // No upstream configured
    };

    // Dirty check
    let dirty = match Command::new("git")
        .args(["-C", &path, "status", "--porcelain=v1", "-unormal"])
        .output()
    {
        Ok(o) if o.status.success() => !o.stdout.is_empty(),
        _ => false,
    };

    Ok(Some(GitRepoInfo {
        branch,
        ahead,
        behind,
        dirty,
    }))
}

#[tauri::command]
pub fn git_pull(path: String) -> Result<String, FmError> {
    let output = Command::new("git")
        .args(["-C", &path, "pull"])
        .output()
        .map_err(|e| FmError::Other(e.to_string()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(FmError::Other(format!("{}{}", stdout, stderr)));
    }

    Ok(format!("{}{}", stdout, stderr))
}

#[tauri::command]
pub fn git_list_branches(path: String) -> Result<Vec<String>, FmError> {
    let output = Command::new("git")
        .args(["-C", &path, "branch", "--format=%(refname:short)"])
        .output()
        .map_err(|e| FmError::Other(e.to_string()))?;

    if !output.status.success() {
        return Err(FmError::Other(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    let branches = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Ok(branches)
}

#[tauri::command]
pub fn git_checkout(path: String, branch: String) -> Result<String, FmError> {
    let output = Command::new("git")
        .args(["-C", &path, "checkout", &branch])
        .output()
        .map_err(|e| FmError::Other(e.to_string()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(FmError::Other(format!("{}{}", stdout, stderr)));
    }

    Ok(format!("{}{}", stdout, stderr).trim().to_string())
}
