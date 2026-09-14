// Where this toolbar is, written down for whoever started it.
//
// The plugin has to answer two questions from outside this process: is a toolbar running,
// and has it gone yet. It used to answer both by reading `/proc` — scanning every process
// for one whose `exe` matched, with a Linux-kernel-specific `" (deleted)"` suffix to
// handle in-place updates and a path-tail heuristic to recognise a copy from a previous
// install generation. All of that is Linux, and none of it is necessary: the process that
// knows where it is can simply say so.
//
// So the toolbar writes a small file when it starts and removes it when it goes. Present
// and holding a live pid means a toolbar; absent means none. The path is given on the
// command line rather than derived, so both sides cannot disagree about it.

use std::path::{Path, PathBuf};

/// The word that precedes the path, when the plugin is the one starting us.
const SAYS_WHERE: &str = "--pidfile";

/// Where to write, if we were told.
pub(crate) fn asked_to_record(args: &[String]) -> Option<PathBuf> {
    let at = args.iter().position(|word| word == SAYS_WHERE)?;
    let path = args.get(at + 1)?;
    (!path.trim().is_empty()).then(|| PathBuf::from(path))
}

/// Say we are here.
///
/// Failure is not fatal and not silent. A toolbar that cannot write this still draws; what
/// is lost is the plugin's ability to say whether one is running, which is worth a line in
/// the log rather than a refusal to start.
pub(crate) fn record(path: &Path) {
    if let Some(within) = path.parent() {
        let _ = std::fs::create_dir_all(within);
    }
    if let Err(trouble) = std::fs::write(path, std::process::id().to_string()) {
        eprintln!("[colai] could not write {}: {trouble}", path.display());
    }
}

/// And that we have gone.
///
/// On the way out, so a file left behind means a crash rather than a running toolbar. The
/// plugin checks the pid is alive regardless — this only shortens the window in which a
/// stale file could name a pid the system has since given to somebody else.
pub(crate) fn forget(path: &Path) {
    let _ = std::fs::remove_file(path);
}
