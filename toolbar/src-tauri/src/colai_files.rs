//! Files and folders brought into a send from the machine the toolbar is running on.
//!
//! The toolbar's own marks are pictures it took. This is the other half: the thing
//! somebody already has — a log, a stylesheet, a directory of them — dropped onto the
//! composer or chosen from a file dialog.
//!
//! Two jobs, and deliberately no store between them. What a mark photographed exists
//! only in memory and has to be held; a file on disk is already stored, by the file
//! system, better than this could. So a path is described when it arrives and read when
//! it is sent, and a file edited in between is sent as it is now — which is the answer
//! somebody would expect from a toolbar that shows them a path.

use base64::Engine as _;
use serde::Serialize;

use crate::gateway_ws::ChatAttachment;

/// A path somebody brought in, as the composer needs to show it.
///
/// `bytes` is the whole tree for a folder, so the composer can say how big the thing
/// somebody dropped actually is rather than reporting a directory as 4 KB.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Chosen {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub folder: bool,
}

/// How deep a dropped folder is measured, and how many entries are counted.
///
/// Only its size is wanted, and only to decide whether to say "12 MB" or "1.2 GB" next
/// to it. Walking a home directory to four decimal places would freeze the toolbar for
/// a number nobody reads that closely, so the walk stops and the size is a floor.
const DEEPEST: usize = 6;
const MOST_ENTRIES: usize = 20_000;

/// Describe paths that arrived by drag and drop.
///
/// Deliberately ungated on roots, unlike everything that *reads* a file: a drop is
/// somebody pointing at something, and the answer is a name, a size and whether it is a
/// folder. What may actually be read is decided later, by `readable`, at the moment bytes
/// are opened.
///
/// It still refuses the names that are never anybody's business. Reporting that
/// `~/.ssh/id_ed25519` exists and is 411 bytes is a smaller thing than reading it, and it
/// is not nothing.
///
/// Paths that cannot be described come back named, rather than quietly dropped: three
/// files dropped and two appearing is a toolbar that lost one without saying so.
#[tauri::command]
pub(crate) fn colai_describe_files(paths: Vec<String>) -> Described {
    let mut chosen = Vec::new();
    let mut refused = Vec::new();
    for path in &paths {
        let at = std::path::Path::new(path);
        let secret = std::fs::canonicalize(at)
            .map(|real| {
                real.components()
                    .filter_map(|part| part.as_os_str().to_str())
                    .any(never_named)
            })
            .unwrap_or(false);
        match (secret, describe(at)) {
            (false, Some(one)) => chosen.push(one),
            _ => refused.push(path.clone()),
        }
    }
    Described { chosen, refused }
}

/// What could be described, and what was named but could not.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Described {
    pub chosen: Vec<Chosen>,
    pub refused: Vec<String>,
}

/// Ask for files, or for a folder, through the desktop's own file dialog.
///
/// GTK's, on the main thread, because that is the loop the rest of this window lives
/// on and a chooser opened off it is a dialog the window manager cannot place. The
/// answer comes back over a channel rather than being returned from the closure, since
/// what `run_on_main_thread` gives back is only whether the closure was scheduled.
#[tauri::command]
pub(crate) async fn colai_pick_files(
    app: tauri::AppHandle,
    folders: bool,
) -> Result<Vec<Chosen>, String> {
    #[cfg(target_os = "linux")]
    {
        use tauri::Manager as _;
        let (say, heard) = tokio::sync::oneshot::channel::<Vec<String>>();
        let parent = app.get_webview_window(crate::colai::OVERLAY_LABEL);
        app.run_on_main_thread(move || {
            let _ = say.send(ask_gtk(
                parent,
                Want {
                    folders,
                    multiple: true,
                    title: if folders { "Add a folder" } else { "Add files" },
                    accept: "Add",
                    start: None,
                },
            ));
        })
        .map_err(|error| format!("Could not open the file chooser: {error}"))?;
        let picked = heard
            .await
            .map_err(|_| "The file chooser closed without answering.".to_string())?;
        Ok(picked
            .iter()
            .filter_map(|path| describe(path.as_ref()))
            .collect())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (app, folders);
        Err("Choosing files is only wired up on Linux.".to_string())
    }
}

/// Where a design document is going, once somebody has pointed at the folder.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Destination {
    /// What to put in the field: relative to the project when it sits inside one.
    pub said: String,
    /// The folder itself, whatever the field ends up saying.
    pub path: String,
}

/// Ask where a design document should go.
///
/// A folder, not a file: what the agent is asked for is a place to write into.
///
/// `within` is the project the marked window was working in, and it does two things.
/// The dialog opens there rather than in the home directory, and a folder chosen inside
/// it comes back as a path relative to it — because the destination travels to an agent
/// that may not be on this machine, and `/home/somebody/Desktop/colai/docs/Design` is an
/// instruction only this machine can follow. `docs/Design/` is one any checkout can.
#[tauri::command]
pub(crate) async fn colai_pick_folder(
    app: tauri::AppHandle,
    within: Option<String>,
) -> Result<Option<Destination>, String> {
    #[cfg(target_os = "linux")]
    {
        use tauri::Manager as _;
        let (say, heard) = tokio::sync::oneshot::channel::<Vec<String>>();
        let parent = app.get_webview_window(crate::colai::OVERLAY_LABEL);
        let start = within.clone();
        app.run_on_main_thread(move || {
            let _ = say.send(ask_gtk(
                parent,
                Want {
                    folders: true,
                    multiple: false,
                    title: "Where the document goes",
                    accept: "Use this folder",
                    start,
                },
            ));
        })
        .map_err(|error| format!("Could not open the file chooser: {error}"))?;
        let picked = heard
            .await
            .map_err(|_| "The file chooser closed without answering.".to_string())?;
        Ok(picked.first().map(|path| said_as(path, within.as_deref())))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (app, within);
        Err("Choosing a folder is only wired up on Linux.".to_string())
    }
}

/// What the field should say about a folder, given the project it may sit inside.
///
/// Split out from the dialog because the dialog cannot run in a test harness and this
/// is the part with a decision in it.
fn said_as(path: &str, within: Option<&str>) -> Destination {
    let said = within
        .map(|root| root.trim_end_matches('/'))
        // A root of `/` makes everything "inside" it and every path relative, which is
        // how an absolute path would come back looking repository-relative.
        .filter(|root| !root.is_empty())
        .and_then(|root| {
            let rest = path.strip_prefix(root)?.trim_start_matches('/');
            // The project root itself is not a folder within the project, and an empty
            // destination means "wherever this project keeps them" elsewhere.
            (!rest.is_empty()).then(|| format!("{rest}/"))
        })
        .unwrap_or_else(|| path.to_string());
    Destination {
        said,
        path: path.to_string(),
    }
}

/// What a dialog is being opened for.
///
/// One dialog function rather than one per caller: adding files to a send and choosing
/// where a document goes differ in what they ask for, not in how they ask.
#[cfg(target_os = "linux")]
struct Want {
    folders: bool,
    multiple: bool,
    title: &'static str,
    accept: &'static str,
    /// Where the dialog opens. A chooser that starts in the home directory when the
    /// project is three levels down is a chooser somebody navigates out of every time.
    start: Option<String>,
}

/// The dialog itself. Runs a nested main loop, which is what makes it modal.
#[cfg(target_os = "linux")]
fn ask_gtk(parent: Option<tauri::WebviewWindow>, want: Want) -> Vec<String> {
    use gtk::prelude::*;

    let window = parent.and_then(|window| window.gtk_window().ok());
    let chooser = gtk::FileChooserNative::new(
        Some(want.title),
        window.as_ref(),
        if want.folders {
            gtk::FileChooserAction::SelectFolder
        } else {
            gtk::FileChooserAction::Open
        },
        Some(want.accept),
        Some("Cancel"),
    );
    chooser.set_select_multiple(want.multiple);
    // A document can be destined for a folder that does not exist yet, which is most of
    // the point of choosing rather than typing: the dialog makes it, and the path that
    // comes back is real.
    chooser.set_create_folders(true);
    if let Some(start) = want.start.as_deref() {
        let path = std::path::Path::new(start);
        if path.is_dir() {
            let _ = chooser.set_current_folder(path);
        }
    }
    let answered = chooser.run();
    let picked = if answered == gtk::ResponseType::Accept {
        chooser
            .filenames()
            .iter()
            .filter_map(|path| path.to_str().map(str::to_string))
            .collect()
    } else {
        Vec::new()
    };
    // Native choosers are not destroyed by running them, and one left alive keeps a
    // reference to the toolbar window it was parented to.
    chooser.destroy();
    picked
}

fn describe(path: &std::path::Path) -> Option<Chosen> {
    let facts = std::fs::metadata(path).ok()?;
    let folder = facts.is_dir();
    Some(Chosen {
        path: path.to_str()?.to_string(),
        name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("(unnamed)")
            .to_string(),
        bytes: if folder { weigh(path) } else { facts.len() },
        folder,
    })
}

/// How much a folder holds, to the depth worth walking.
/// How big a dropped folder is, to a useful approximation.
///
/// One implementation, not two. There was a `cfg(not(unix))` arm returning `0`, which
/// would have reported every dropped folder on Windows as empty — and nothing in the body
/// below is unix-specific: it is `read_dir` and `DirEntry::metadata`, which both work
/// everywhere. A second implementation that answers wrongly is worse than no second
/// implementation at all.
fn weigh(root: &std::path::Path) -> u64 {
    let mut total = 0u64;
    let mut seen = 0usize;
    let mut walking = vec![(root.to_path_buf(), 0usize)];
    while let Some((here, depth)) = walking.pop() {
        let Ok(entries) = std::fs::read_dir(&here) else {
            continue;
        };
        for entry in entries.flatten() {
            if seen >= MOST_ENTRIES {
                return total;
            }
            seen += 1;
            // `DirEntry::metadata` does not follow the link, so a link into a parent
            // directory is counted as the few bytes it is rather than walked into a loop.
            // Named for what is called, because `fs::metadata` here would reintroduce it.
            let Ok(facts) = entry.metadata() else {
                continue;
            };
            if facts.is_dir() {
                if depth + 1 < DEEPEST {
                    walking.push((entry.path(), depth + 1));
                }
            } else {
                total = total.saturating_add(facts.len());
            }
        }
    }
    total
}

/// Directories never worth walking, because nothing in them is what somebody meant.
///
/// Not a security rule — `may_read` is that. This is about the walk finishing: a single
/// `node_modules` holds more entries than the rest of a checkout put together, and a
/// search that has to cross it before reaching `src` is a search nobody waits for.
const NOT_WORTH_WALKING: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    ".next",
    ".venv",
    "venv",
    "__pycache__",
    ".cache",
    "vendor",
];

/// How far a search goes before it stops looking.
const SEARCH_DEEPEST: usize = 8;
const SEARCH_ENTRIES: usize = 60_000;
const SEARCH_MOST: usize = 40;

/// A path `@` could mean.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Found {
    pub path: String,
    /// What to show: the path relative to the folder it was found in, because the whole
    /// of an absolute path is mostly the part every result has in common.
    pub shown: String,
    pub folder: bool,
}

/// Files under the folders somebody works in, matching what they have typed so far.
///
/// The roots come from the Gateway, not from the caller. That is the whole shape of
/// this: a page that could name its own roots could name `/`.
///
/// Matched on the shown path rather than the file name, so `src/ap` finds
/// `src/app.tsx` — which is how somebody types a path they half remember.
#[tauri::command]
pub(crate) async fn colai_search_files(
    gateway: tauri::State<'_, crate::gateway_ws::GatewayClient>,
    query: String,
) -> Result<Vec<Found>, String> {
    let roots = crate::colai_receivers::work_roots(&gateway).await;
    Ok(search_within(&roots, &query))
}

fn search_within(roots: &[std::path::PathBuf], query: &str) -> Vec<Found> {
    let want = query.trim().to_ascii_lowercase();
    let mut found: Vec<Found> = Vec::new();
    let mut seen = 0usize;
    for root in roots {
        let Ok(root) = std::fs::canonicalize(root) else {
            continue;
        };
        let mut walking = vec![(root.clone(), 0usize)];
        while let Some((here, depth)) = walking.pop() {
            if found.len() >= SEARCH_MOST || seen >= SEARCH_ENTRIES {
                break;
            }
            let Ok(entries) = std::fs::read_dir(&here) else {
                continue;
            };
            for entry in entries.flatten() {
                if found.len() >= SEARCH_MOST || seen >= SEARCH_ENTRIES {
                    break;
                }
                seen += 1;
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                // `DirEntry::metadata` again, for the same reason as the folder walk
                // above: it does not follow the link, so a link back into a parent is not
                // walked into a loop.
                let Ok(facts) = entry.metadata() else {
                    continue;
                };
                let folder = facts.is_dir();
                if folder && (NOT_WORTH_WALKING.contains(&name) || name.starts_with('.')) {
                    continue;
                }
                if folder && depth < SEARCH_DEEPEST {
                    walking.push((path.clone(), depth + 1));
                }
                // Offered only if it could actually be read. A picker that lists what the
                // gate will refuse teaches somebody the toolbar is broken.
                if !folder && !may_read(&path, roots) {
                    continue;
                }
                let shown = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                if !want.is_empty() && !shown.to_ascii_lowercase().contains(&want) {
                    continue;
                }
                if folder {
                    continue;
                }
                found.push(Found {
                    path: path.to_string_lossy().to_string(),
                    shown,
                    folder,
                });
            }
        }
    }
    // Shortest first: the closest match to what was typed is usually the shallowest one.
    found.sort_by(|a, b| {
        a.shown
            .len()
            .cmp(&b.shown.len())
            .then_with(|| a.shown.cmp(&b.shown))
    });
    found.truncate(SEARCH_MOST);
    found
}

/*
 * ── what may be read ─────────────────────────────────────────────────────────
 *
 * Everything reachable from here can be put in front of a model, so this is the widest
 * thing the toolbar does and the part worth being narrow about.
 *
 * Two rules, and the order matters. A path must sit inside a root somebody actually
 * works in, and it must not be one of the things that are never readable however far
 * inside a root they sit. Deny wins: a `.env` in the middle of a project is still a
 * `.env`.
 *
 * The check is on the *resolved* path. `..` and symlinks are how a path that looks like
 * it is inside a project turns out to be `~/.ssh`, and comparing the string somebody
 * typed would miss both.
 */

/// Never readable, wherever they are. Names, not globs — a glob language here would be
/// one more thing to get subtly wrong on the one list that must not be wrong.
const NEVER_READ: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".azure",
    ".kube",
    ".docker",
    ".netrc",
    ".pgpass",
    ".git-credentials",
    ".npmrc",
    ".pypirc",
    "id_rsa",
    "id_ed25519",
    "credentials",
    "shadow",
];

/// And these, by extension, because a key is a key whatever it is called.
const NEVER_ENDS: &[&str] = &[".pem", ".key", ".p12", ".pfx", ".keystore", ".jks"];

/// Whether one path component is one of the things that are never readable.
///
/// A `.env` matches, and so does `.env.local`, because the interesting part of the name
/// is the front of it — but `.environment` does not, since a prefix match on a bare name
/// would refuse half of somebody's project.
fn never_named(part: &str) -> bool {
    let lower = part.to_ascii_lowercase();
    if lower == ".env" || lower.starts_with(".env.") {
        return true;
    }
    if NEVER_READ.iter().any(|name| lower == *name) {
        return true;
    }
    NEVER_ENDS.iter().any(|end| lower.ends_with(end))
}

/// Whether a path may be read into a prompt.
///
/// Resolved first, so `..` and symlinks are answered rather than trusted. A path that
/// does not exist is refused: there is nothing to read, and saying yes to it would make
/// the answer depend on what appears there later.
pub(crate) fn may_read(path: &std::path::Path, roots: &[std::path::PathBuf]) -> bool {
    readable(path, roots).is_some()
}

/// The path to actually open, once it is allowed.
///
/// The resolved path comes back rather than a yes, and the caller reads *that*. Deciding
/// on the canonical path and then opening the original leaves a window in which a symlink
/// component can be swapped — a build script inside a project could point `notes.txt` at
/// `~/.ssh/id_ed25519` between the two, and the file would travel.
pub(crate) fn readable(
    path: &std::path::Path,
    roots: &[std::path::PathBuf],
) -> Option<std::path::PathBuf> {
    let real = std::fs::canonicalize(path).ok()?;
    // Every component, not just the last: a file inside `.ssh` is inside `.ssh`.
    if real
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .any(never_named)
    {
        return None;
    }
    let inside = roots.iter().any(|root| {
        std::fs::canonicalize(root)
            .is_ok_and(|root| !root.as_os_str().is_empty() && real.starts_with(root))
    });
    inside.then_some(real)
}

/// Read the files that are travelling with a message.
///
/// Only the ones the page decided could travel — the size rules live next to the
/// composer that shows them, and are tested there. A path that has gone since it was
/// dropped is skipped rather than failing the send: the message still names it, and
/// losing a whole send because one of four files moved is the worse outcome.
pub(crate) fn carry(paths: &[String], roots: &[std::path::PathBuf]) -> Carried {
    let mut travelling = Vec::new();
    let mut refused = Vec::new();
    for path in paths {
        let asked = std::path::Path::new(path);
        // The gate, at the only moment that counts. The picker offers what is inside a
        // project, but a path can reach this list by drag and drop, by a dialog, or by
        // anything else the page decides to put in it — so what may be read is decided
        // here, once, rather than by whichever surface happened to add it.
        //
        // And the resolved path is what gets opened, so nothing can be swapped underneath
        // between the decision and the read.
        let Some(real) = readable(asked, roots) else {
            refused.push(path.clone());
            continue;
        };
        let (Ok(bytes), Some(name)) = (
            std::fs::read(&real),
            asked.file_name().and_then(|name| name.to_str()),
        ) else {
            refused.push(path.clone());
            continue;
        };
        travelling.push(ChatAttachment {
            kind: "file".to_string(),
            mime_type: mime_of(asked).to_string(),
            file_name: name.to_string(),
            content: base64::engine::general_purpose::STANDARD.encode(bytes),
            // Not a picture, so it has no size on screen. The Gateway sniffs the real
            // type off the bytes anyway; these are a hint, not a claim.
            width: 0,
            height: 0,
        });
    }
    Carried {
        travelling,
        refused,
    }
}

/// What travelled, and what was named but did not.
///
/// The refusals come back because the message has already told the agent those files are
/// attached. Dropping them quietly means a message that says "the log is attached" with
/// no log — the agent answers about something it cannot see, and nobody is told why.
pub(crate) struct Carried {
    pub travelling: Vec<ChatAttachment>,
    pub refused: Vec<String>,
}

/// A guess at the type, from the extension.
///
/// Deliberately shallow. The Gateway sniffs the actual bytes and warns when the two
/// disagree, so the only job here is to be a useful hint for the handful of things
/// somebody actually drags onto a toolbar — and to say "unknown" honestly otherwise
/// rather than guessing text and having something binary arrive mangled.
fn mime_of(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|end| end.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("json") => "application/json",
        Some("md") => "text/markdown",
        Some("csv") => "text/csv",
        Some("html" | "htm") => "text/html",
        Some("css") => "text/css",
        Some(
            "txt" | "log" | "ts" | "tsx" | "js" | "jsx" | "rs" | "py" | "sh" | "toml" | "yml"
            | "yaml",
        ) => "text/plain",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_to_carry_needs_no_permission_to_carry_it() {
        /*
         * `colai_send` used to ask the Gateway for every catalog, host and session it
         * knows before every message, to work out which folders may be read — including
         * when the message carried no files at all. That is a Gateway round trip in
         * front of every send, for a question about an empty list.
         *
         * It is skipped now when there is nothing attached, which is only safe because
         * the gate has nothing to let through either way. This is that equivalence: with
         * no paths, the roots cannot change the answer.
         */
        assert!(carry(&[], &[]).travelling.is_empty());
        assert!(carry(&[], &[std::path::PathBuf::from("/")]).travelling.is_empty());
        assert!(carry(&[], &[std::env::temp_dir()]).travelling.is_empty());
    }

    #[test]
    fn a_folder_weighs_what_is_in_it_not_what_the_directory_entry_says() {
        let root = std::env::temp_dir().join(format!("colai-weigh-{}", std::process::id()));
        let inner = root.join("deeper");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(root.join("a.txt"), vec![0u8; 100]).unwrap();
        std::fs::write(inner.join("b.txt"), vec![0u8; 250]).unwrap();

        let seen = describe(&root).unwrap();
        assert!(seen.folder);
        assert_eq!(seen.bytes, 350);
        assert_eq!(seen.name, root.file_name().unwrap().to_str().unwrap());

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A project, a secret beside it, and somewhere private outside it.
    ///
    /// `canonicalize` on the temp root first: on some systems it is a symlink, and a
    /// containment test written against the un-resolved path passes on Linux and fails
    /// wherever `/tmp` is `/private/tmp`.
    fn a_desk(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("colai-gate-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("project/src")).unwrap();
        std::fs::create_dir_all(root.join("private/.ssh")).unwrap();
        std::fs::write(root.join("project/src/main.rs"), b"fn main() {}").unwrap();
        std::fs::write(root.join("project/.env"), b"TOKEN=hunter2").unwrap();
        std::fs::write(root.join("private/.ssh/id_rsa"), b"-----BEGIN").unwrap();
        // An innocent name inside a directory that is not: this is the case a check on
        // the last component alone would wave straight through.
        std::fs::write(root.join("private/.ssh/config"), b"Host *").unwrap();
        std::fs::write(root.join("private/secrets.pem"), b"-----BEGIN").unwrap();
        (root.clone(), root.join("project"))
    }

    #[test]
    fn only_what_is_inside_a_folder_somebody_works_in_can_be_read() {
        let (root, project) = a_desk("inside");
        let roots = vec![project.clone()];

        assert!(may_read(&project.join("src/main.rs"), &roots));
        // Outside the project entirely.
        assert!(!may_read(&root.join("private/secrets.pem"), &roots));
        // Nothing to read is not permission to read it later.
        assert!(!may_read(&project.join("src/nothing-here.rs"), &roots));
        // And no roots at all refuses everything, which is what an unreachable Gateway
        // leaves behind — no answer is not permission.
        assert!(!may_read(&project.join("src/main.rs"), &[]));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_path_that_climbs_out_of_the_project_is_refused_however_it_is_written() {
        /*
         * The two ways a path that looks inside turns out not to be. Comparing the string
         * somebody typed would let both of these through, which is why the check is on
         * the resolved path.
         */
        let (root, project) = a_desk("escape");
        let roots = vec![project.clone()];

        // `..` back out and into somewhere private.
        let climbed = project.join("src/../../private/secrets.pem");
        assert!(
            climbed.starts_with(&project),
            "the written path looks inside"
        );
        assert!(!may_read(&climbed, &roots), "the resolved path is not");

        // A symlink inside the project pointing out of it.
        #[cfg(unix)]
        {
            let link = project.join("src/way-out");
            std::os::unix::fs::symlink(root.join("private/secrets.pem"), &link).unwrap();
            assert!(!may_read(&link, &roots), "a link out is a way out");
        }

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn some_things_are_never_read_however_far_inside_a_project_they_sit() {
        // Deny wins over allow. A `.env` in the middle of somebody's checkout is still a
        // `.env`, and this is the list that must not be wrong.
        let (root, project) = a_desk("never");
        let roots = vec![project.clone()];

        assert!(may_read(&project.join("src/main.rs"), &roots));
        assert!(
            !may_read(&project.join(".env"), &roots),
            ".env is never read"
        );

        // Every component, not just the last: a file inside `.ssh` is inside `.ssh`.
        let wide = vec![root.clone()];
        assert!(!may_read(&root.join("private/.ssh/id_rsa"), &wide));
        // The one that matters: `config` is a perfectly ordinary name, and it is inside
        // `.ssh`. Checking only the last component would read it.
        assert!(
            !may_read(&root.join("private/.ssh/config"), &wide),
            "a plain name inside a denied directory is still denied"
        );
        assert!(
            !may_read(&root.join("private/secrets.pem"), &wide),
            "a key by extension"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_picker_never_offers_what_the_gate_would_refuse() {
        // A picker that lists what the gate then refuses teaches somebody the toolbar is
        // broken. The two answer the same question, so they answer it the same way.
        let (root, project) = a_desk("search");
        let roots = vec![project.clone()];

        let all = search_within(&roots, "");
        let shown: Vec<&str> = all.iter().map(|one| one.shown.as_str()).collect();
        assert!(
            shown.contains(&"src/main.rs"),
            "found what is inside: {shown:?}"
        );
        assert!(
            !shown.iter().any(|one| one.contains(".env")),
            "never the secret"
        );

        // Matched on the path, not the file name, because that is how somebody types a
        // path they half remember.
        let some = search_within(&roots, "src/ma");
        assert_eq!(
            some.iter()
                .map(|one| one.shown.as_str())
                .collect::<Vec<_>>(),
            ["src/main.rs"]
        );

        // Nothing outside a root is reachable, whatever is asked for.
        assert!(search_within(&roots, "secrets").is_empty());
        assert!(
            search_within(&[], "main").is_empty(),
            "no roots offers nothing"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_name_is_refused_for_being_the_thing_and_not_for_starting_like_it() {
        // The list refuses `.env` and `.env.local` and leaves `.environment` alone. A
        // prefix match on a bare name would quietly refuse half of somebody's project.
        assert!(never_named(".env"));
        assert!(never_named(".env.local"));
        assert!(never_named(".ENV"));
        assert!(never_named("id_rsa"));
        assert!(never_named("cluster.pem"));
        assert!(!never_named(".environment"));
        assert!(!never_named("environment.ts"));
        assert!(!never_named("main.rs"));
        assert!(!never_named("readme.md"));
    }

    #[test]
    fn a_path_that_is_not_there_is_left_out_rather_than_described_as_empty() {
        assert!(describe(std::path::Path::new("/nowhere/at/all/really")).is_none());
        // Named but not travelling, and the caller is told which — a message that says a
        // file is attached with no file is the outcome this refusal list exists to stop.
        let brought = carry(
            &["/nowhere/at/all/really".to_string()],
            &[std::path::PathBuf::from("/")],
        );
        assert!(brought.travelling.is_empty());
        assert_eq!(brought.refused, vec!["/nowhere/at/all/really".to_string()]);
    }

    #[test]
    fn a_file_travels_under_its_own_name_with_a_type_worth_having() {
        let path = std::env::temp_dir().join(format!("colai-carry-{}.md", std::process::id()));
        std::fs::write(&path, b"# hello").unwrap();

        // The temp directory stands in for a project root here; what may be read is
        // its own test below.
        let roots = vec![std::fs::canonicalize(std::env::temp_dir()).unwrap()];
        let carried = carry(&[path.to_str().unwrap().to_string()], &roots).travelling;
        assert_eq!(carried.len(), 1);
        assert_eq!(
            carried[0].file_name,
            path.file_name().unwrap().to_str().unwrap()
        );
        assert_eq!(carried[0].mime_type, "text/markdown");
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(&carried[0].content)
                .unwrap(),
            b"# hello"
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_folder_inside_the_project_is_said_the_way_any_checkout_could_follow() {
        // The destination travels to an agent that may not be on this machine, so an
        // absolute path here is an instruction only this machine can follow.
        assert_eq!(
            said_as(
                "/home/me/code/colai/docs/Design",
                Some("/home/me/code/colai")
            ),
            Destination {
                said: "docs/Design/".to_string(),
                path: "/home/me/code/colai/docs/Design".to_string(),
            }
        );
        // A trailing slash on the project is the same project.
        assert_eq!(
            said_as("/home/me/code/colai/docs", Some("/home/me/code/colai/")).said,
            "docs/"
        );
    }

    #[test]
    fn a_folder_outside_the_project_is_said_in_full_rather_than_pretended_about() {
        assert_eq!(
            said_as("/etc/design", Some("/home/me/code/colai")).said,
            "/etc/design"
        );
        // Nothing known to be inside: the honest answer is the path itself.
        assert_eq!(said_as("/etc/design", None).said, "/etc/design");
        // A root of `/` would make every absolute path look repository-relative.
        assert_eq!(said_as("/etc/design", Some("/")).said, "/etc/design");
        assert_eq!(said_as("/etc/design", Some("")).said, "/etc/design");
    }

    #[test]
    fn the_project_root_itself_is_not_a_folder_within_the_project() {
        // An empty destination already means "wherever this project keeps them", so
        // producing one here would quietly change what was asked for.
        assert_eq!(
            said_as("/home/me/code/colai", Some("/home/me/code/colai")).said,
            "/home/me/code/colai"
        );
    }

    #[test]
    fn an_unfamiliar_extension_says_so_rather_than_guessing_text() {
        assert_eq!(
            mime_of(std::path::Path::new("a.wasm")),
            "application/octet-stream"
        );
        assert_eq!(
            mime_of(std::path::Path::new("a")),
            "application/octet-stream"
        );
        assert_eq!(mime_of(std::path::Path::new("A.PNG")), "image/png");
    }
}
