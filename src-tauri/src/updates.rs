//! Lightweight "is there a newer release on GitHub?" check.
//!
//! Mirrors the in-app update notice used by the sibling apps: it only *tells* the user
//! a newer version exists and links them to the GitHub release page — it never
//! downloads or installs anything (no Tauri updater plugin). The check runs in Rust so
//! it needs no CSP/`connect-src` changes and no extra dependency (reuses `reqwest`).
//!
//! To reuse in another app, copy this file, set `OWNER`/`REPO`, register
//! `updates::check_for_update` in the Tauri `invoke_handler`, and add the frontend
//! `updateChecker.ts` + `UpdateNotice.svelte`.

use serde::{Deserialize, Serialize};

/// The GitHub repository whose releases are checked.
const OWNER: &str = "L-K-M";
const REPO: &str = "Baegun";

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    #[serde(default)]
    draft: bool,
}

/// What the frontend needs to show the notice. `None` from the command means "no newer
/// release" (or the check couldn't run); the UI then shows nothing.
#[derive(Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub url: String,
    pub notes: Option<String>,
}

/// Queries the repo's latest published release and returns it only when it's newer than
/// the running app (`CARGO_PKG_VERSION`, kept in step with the release tag by
/// `scripts/release.sh`). A missing release or any network/parse failure is reported as
/// an error string; the frontend treats failures as "nothing to show".
#[tauri::command]
pub async fn check_self_update() -> Result<Option<UpdateInfo>, String> {
    let endpoint = format!("https://api.github.com/repos/{OWNER}/{REPO}/releases/latest");
    let client = reqwest::Client::builder()
        // GitHub's API requires a User-Agent.
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get(endpoint)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None); // the repo has no published release yet
    }
    if !response.status().is_success() {
        return Err(format!(
            "GitHub returned HTTP {}",
            response.status().as_u16()
        ));
    }

    let release: GitHubRelease = response.json().await.map_err(|e| e.to_string())?;
    if release.draft {
        return Ok(None);
    }

    if is_newer(&release.tag_name, env!("CARGO_PKG_VERSION")) {
        Ok(Some(UpdateInfo {
            version: normalize(&release.tag_name),
            url: release.html_url,
            notes: release.body.filter(|b| !b.trim().is_empty()),
        }))
    } else {
        Ok(None)
    }
}

/// Opens a release page in the user's default browser. Restricted to http(s) so the
/// frontend can only ever hand it a web link. Bundled here so this update feature is a
/// self-contained drop-in (no dependency on the app's own URL-opener command).
#[tauri::command]
pub fn open_release_url(url: String) -> Result<(), String> {
    let lower = url.to_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err("Only http(s) URLs may be opened".to_string());
    }
    open::that(&url).map_err(|e| e.to_string()).map(|_| ())
}

/// Drops a leading `v`/`V` (Git tags) leaving the bare version.
fn normalize(version: &str) -> String {
    version.trim_start_matches(['v', 'V']).to_string()
}

/// Numeric, component-wise "is `latest` a higher version than `current`?" — so `1.10`
/// beats `1.9`, `1.2` equals `1.2.0`, and any `-pre`/`+build` suffix is ignored.
fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        normalize(v)
            .split(['-', '+'])
            .next()
            .unwrap_or("")
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    }
    let (a, b) = (parts(latest), parts(current));
    for i in 0..a.len().max(b.len()) {
        let (l, r) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if l != r {
            return l > r;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("v1.10.0", "1.9.0"));
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(!is_newer("1.2", "1.2.0")); // equal
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
    }
}
