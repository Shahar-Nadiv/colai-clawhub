//! A screenshot of an edited web page, rendered rather than re-photographed.
//!
//! The review card wants to show the *result* of an edit — but the live screen does not change
//! until the app is reloaded, and the card must never fall back to showing raw code. So for a web
//! file colai renders it headlessly (the Chrome or Edge already on the machine) and screenshots
//! that, which is a true picture of the change without waiting on a reload. It renders the page as
//! it loads: anything visible then shows exactly; a part of the UI that only appears after the
//! person interacts (a modal, a game-over card) is not reachable this way, and the card says so.

use base64::Engine as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a render may take before the browser is killed and the card told so.
///
/// A headless render of a local page settles in a second or two; the virtual-time budget
/// below is a second and a half of that. Twenty seconds is far past anything a working
/// render needs, and short enough that a browser wedged on a first-run dialog or a page
/// that never finishes loading is ended rather than left running behind the toolbar.
const RENDER_TIMEOUT: Duration = Duration::from_secs(20);

/// How often a running render is asked whether it has finished.
const RENDER_POLL: Duration = Duration::from_millis(50);

/// Render `file` (an .html page) to a PNG and return it as a `data:` URL for the card to show.
///
/// The browser runs on a blocking worker, not the async one this command arrives on. A
/// render is a whole browser starting, loading and exiting — seconds of waiting on a child
/// process — and waiting on it in place held an async worker the whole time, one of the
/// handful every other command shares.
#[tauri::command]
pub(crate) async fn colai_render_shot(file: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || render(&file))
        .await
        .map_err(|_| "the render was abandoned before it finished".to_string())?
}

/// The render itself, start to finish, on whatever thread may block.
fn render(file: &str) -> Result<String, String> {
    validate_web(file)?;
    let path = Path::new(file);
    let browser = find_browser().ok_or("no Chrome or Edge on this machine to render with")?;

    let out = std::env::temp_dir().join(format!(
        "colai-render-{}-{}.png",
        std::process::id(),
        now_ms()
    ));
    let url = file_url(path);
    let mut run = Command::new(&browser);
    run.args([
        "--headless=new",
        "--disable-gpu",
        "--hide-scrollbars",
        "--force-device-scale-factor=1",
        // Network lockdown. The HTML being rendered is agent-authored, and a real browser would
        // give it egress and JS execution — enough to phone home or pull in remote resources. So
        // every host is resolved to nothing (`MAP * ~NOTFOUND` blocks all DNS), no proxy can route
        // around that, and the browser's own background chatter (sync, default-app installs,
        // background networking) is switched off. The shot is of locally-written HTML/CSS only:
        // a `file://` page with inline styles renders fine with DNS blocked.
        "--host-resolver-rules=MAP * ~NOTFOUND",
        "--no-proxy-server",
        "--disable-background-networking",
        "--disable-sync",
        "--disable-default-apps",
        &format!("--screenshot={}", out.display()),
        "--window-size=900,700",
        // Advance virtual time so a page that paints on a timer (a fade-in, a first frame) has
        // settled before the shot is taken.
        "--virtual-time-budget=1500",
        &url,
    ]);
    crate::session::no_console_window(&mut run);
    // Nothing reads what the browser says, so it is not piped: a pipe nobody drains is a
    // browser that blocks on a full buffer and runs into the timeout for no reason.
    run.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut rendering = run
        .spawn()
        .map_err(|trouble| format!("could not run the browser: {trouble}"))?;
    if let Err(trouble) = wait_or_kill(&mut rendering, RENDER_TIMEOUT) {
        let _ = std::fs::remove_file(&out);
        return Err(trouble);
    }

    let bytes = std::fs::read(&out).map_err(|_| "the browser produced no screenshot".to_string())?;
    let _ = std::fs::remove_file(&out);
    if bytes.is_empty() {
        return Err("the browser produced an empty screenshot".into());
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:image/png;base64,{encoded}"))
}

/// Wait for a child to exit, for no longer than `limit`; past that it is killed and reaped.
///
/// Polled rather than waited on, because `Child::wait` has no deadline and a browser that
/// never exits would hold this thread — and the card's spinner — forever. Whether it exited
/// cleanly is not the question: the screenshot file existing is, and the caller checks it.
fn wait_or_kill(child: &mut std::process::Child, limit: Duration) -> Result<(), String> {
    let began = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if began.elapsed() < limit => std::thread::sleep(RENDER_POLL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "the browser took longer than {} seconds to render, so it was stopped",
                    limit.as_secs()
                ));
            }
            Err(trouble) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("could not wait for the browser: {trouble}"));
            }
        }
    }
}

/// A web page that exists — the only thing worth handing a browser. Separated from the async
/// command so it can be tested without one.
fn validate_web(file: &str) -> Result<(), String> {
    let lower = file.to_lowercase();
    if !(lower.ends_with(".html") || lower.ends_with(".htm")) {
        return Err("that edit isn't a web page colai can render".into());
    }
    if !Path::new(file).exists() {
        return Err("the file isn't there to render".into());
    }
    Ok(())
}

/// A `file://` URL for a local path, slashes the way a browser wants them.
fn file_url(path: &Path) -> String {
    let full = path.to_string_lossy().replace('\\', "/");
    if full.starts_with('/') {
        format!("file://{full}")
    } else {
        format!("file:///{full}")
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// The first Chrome or Edge this machine has, or nothing. Absolute installs are checked for real;
/// bare names are left for the OS to resolve on `PATH` (Linux), and a spawn that fails is handled
/// by the caller.
fn find_browser() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(&local).join(r"Google\Chrome\Application\chrome.exe"),
        );
    }
    for abs in [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ] {
        candidates.push(PathBuf::from(abs));
    }
    if let Some(found) = candidates.into_iter().find(|p| p.exists()) {
        return Some(found);
    }
    // Linux: trust PATH for a known name; the caller surfaces a spawn failure as "no browser".
    for name in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser"] {
        if which_on_path(name) {
            return Some(PathBuf::from(name));
        }
    }
    None
}

/// Whether a bare command resolves on PATH, without running it.
fn which_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(name).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_web_page_that_exists_is_rendered() {
        assert!(
            validate_web("styles.css").unwrap_err().contains("web page"),
            "a non-web edit is refused, never rendered as code",
        );
        assert!(validate_web("/no/such/page-xyzzy.html").unwrap_err().contains("isn't there"));
        // A real .html file passes the check.
        let p = std::env::temp_dir().join(format!("colai-render-ok-{}.html", std::process::id()));
        std::fs::write(&p, "<html></html>").unwrap();
        assert!(validate_web(p.to_str().unwrap()).is_ok());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_render_that_never_finishes_is_stopped_rather_than_waited_on() {
        // Something that would run for half a minute, given a deadline of a fraction of a
        // second: it has to come back as a refusal, promptly, with the process gone.
        #[cfg(windows)]
        let mut slow = Command::new("ping");
        #[cfg(windows)]
        slow.args(["-n", "30", "127.0.0.1"]);
        #[cfg(not(windows))]
        let mut slow = Command::new("sleep");
        #[cfg(not(windows))]
        slow.arg("30");
        slow.stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = slow.spawn().expect("start something slow");
        let began = Instant::now();
        let refused = wait_or_kill(&mut child, Duration::from_millis(300));
        assert!(refused.unwrap_err().contains("stopped"));
        assert!(began.elapsed() < Duration::from_secs(5), "the deadline was not kept");
        assert!(child.try_wait().expect("ask after it").is_some(), "the process was left running");
    }

    #[test]
    fn a_local_path_becomes_a_file_url() {
        assert_eq!(file_url(Path::new("/home/a/b.html")), "file:///home/a/b.html");
        let win = file_url(Path::new(r"C:\x\y.html"));
        assert!(win.starts_with("file:///C:/x/y.html"), "{win}");
    }
}
