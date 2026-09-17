// Marks left where a running Claude Code will find them.
//
// The toolbar cannot speak into a session somebody is sitting in. It runs its own `claude`,
// and pointing that at a live conversation puts two processes on one transcript — which is
// not a crash but something worse to debug: the mark is answered in the toolbar's Work
// panel while the chat the person is looking at says nothing at all. That is exactly how
// this was reported: "nothing was sent to this session".
//
// There is a way in, and it is the session's own front door rather than a side entrance:
// a `UserPromptSubmit` hook runs inside the session, on the person's next message, and what
// it prints goes to the model. So the toolbar writes the mark down here, and the hook hands
// it over from the inside.
//
// What that costs is immediacy. The mark does not appear the instant it is sent; it appears
// when the person next presses Enter in that chat. That is a real limitation and the rail
// says so rather than pretending the message has gone.
//
// Why not the peer socket, which is the mechanism built for this: it needs the session's
// `peerToken`, a credential, and `peerProtocol: 1` answers nothing at all to
// newline-delimited JSON — there is an undocumented framing layer under it. Both were
// tried. A private protocol with a credential in front of it is not a thing to build a
// product on; a documented hook is.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;

/// What was staged, so the rail can say where it went and how much of it.
pub(crate) struct Staged {
    pub at: PathBuf,
    pub pictures: usize,
}

/// Where marks wait for the session they belong to.
///
/// Beside the pidfile, under the toolbar's own config directory, because it is the toolbar's
/// state rather than the plugin's — and the same expression the launcher and the notice use,
/// so all three agree without being told.
pub(crate) fn outbox() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(said) if !said.is_empty() => PathBuf::from(said),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("ai.colai.toolbar").join("outbox"))
}

/// Leave a mark for a conversation that is open in somebody's terminal.
///
/// One directory per send, named for the moment it happened so they are handed over in the
/// order they were made. The directory is built under a `.part` name and renamed into place
/// once everything is in it: the hook may run at any instant, and a half-written mark read
/// halfway is a picture with no question or a question with no picture.
pub(crate) fn stage(
    session_key: &str,
    cwd: Option<&str>,
    message: &str,
    images: &[String],
) -> Result<Staged, String> {
    let outbox = outbox().ok_or("no home directory to write to")?;
    let waiting = outbox.join(session_key);
    let named = format!("{}-{}", now_ms(), std::process::id());
    let building = waiting.join(format!("{named}.part"));
    fs::create_dir_all(&building)
        .map_err(|trouble| format!("could not prepare {}: {trouble}", building.display()))?;

    /*
     * The pictures go in the project, not in here.
     *
     * Found by running it: with the pictures under `~/.config`, the session was handed
     * their paths and answered "the read was denied" — a mark delivered, described, and
     * unopenable. A session may read what is under the directory it is working in, and
     * that is the whole of the rule. So the pictures are written there and the mark that
     * points at them stays here.
     *
     * `.colai/` carries a `.gitignore` of its own saying `*`, so a directory the toolbar
     * put in somebody's repository cannot turn up in their next commit.
     */
    let pictures_at = match cwd.map(Path::new).filter(|cwd| cwd.is_dir()) {
        Some(cwd) => {
            let at = cwd.join(".colai").join("marks").join(&named);
            match fs::create_dir_all(&at) {
                Ok(()) => {
                    let _ = fs::write(cwd.join(".colai").join(".gitignore"), "*\n");
                    prune(&cwd.join(".colai").join("marks"));
                    at
                }
                // A read-only checkout, or somewhere we may not write. The mark still goes;
                // the pictures sit here and the session asks before opening them, which is
                // a prompt rather than a failure.
                Err(_) => building.clone(),
            }
        }
        None => building.clone(),
    };

    let mut wrote = Vec::new();
    for (at, data) in images.iter().enumerate() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|trouble| format!("a mark could not be read back: {trouble}"))?;
        let name = format!("mark-{}.png", at + 1);
        fs::write(pictures_at.join(&name), bytes)
            .map_err(|trouble| format!("could not write {name}: {trouble}"))?;
        wrote.push(pictures_at.join(&name));
    }

    // Rendered here rather than by the hook, so the hook is a `cat` and cannot get the
    // quoting wrong. Everything it needs to know is already known here.
    fs::write(building.join(SAY), &handing_over(message, &wrote))
        .map_err(|trouble| format!("could not write the message: {trouble}"))?;

    let at = waiting.join(named);
    fs::rename(&building, &at)
        .map_err(|trouble| format!("could not put the mark in place: {trouble}"))?;
    Ok(Staged { at, pictures: wrote.len() })
}

/// The file the hook reads and prints.
pub(crate) const SAY: &str = "say.txt";

/// What the session is told, in the words the model will see.
///
/// Addressed to the model and about the person, because that is what it is: the person did
/// this in another window a moment ago, and the session is being caught up. It says where
/// the pictures are rather than carrying them, since a hook's output is text — and Claude
/// Code reads an image file directly, so a path is enough.
fn handing_over(message: &str, pictures: &[PathBuf]) -> String {
    let mut said = String::from(
        "The person marked something on their screen with colai and sent it to this \
         conversation. It was made outside this chat, so it is arriving now rather than \
         when they sent it.\n\n",
    );
    if !pictures.is_empty() {
        said.push_str(
            "Read these first — they are screenshots of exactly what was pointed at, and \
             the message below refers to them:\n",
        );
        for one in pictures {
            said.push_str(&format!("  {}\n", one.display()));
        }
        said.push('\n');
    }
    said.push_str("What they asked:\n");
    said.push_str(message.trim());
    said.push('\n');
    said
}

/// Whether anything is waiting for this conversation.
///
/// Cheap, because the rail asks it on every reload to decide whether to say so.
pub(crate) fn waiting_for(session_key: &str) -> usize {
    let Some(outbox) = outbox() else {
        return 0;
    };
    let Ok(entries) = fs::read_dir(outbox.join(session_key)) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| is_a_finished_mark(&entry.path()))
        .count()
}

/// A staged mark that is all there — not one still being written.
fn is_a_finished_mark(path: &Path) -> bool {
    path.is_dir()
        && path.extension().and_then(|e| e.to_str()) != Some("part")
        && path.join(SAY).is_file()
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis())
        .unwrap_or(0)
}

/// How long a mark's pictures stay readable after they have been handed over.
///
/// Not removed when the hook consumes them, which was the first thought and is wrong: a
/// conversation goes on after the mark arrives, and "look at that screenshot again" is an
/// ordinary next sentence. They are cleared on a later send instead, by which time the
/// conversation has moved on.
const KEEP_PICTURES_FOR: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Clear out marks old enough that nothing is still talking about them.
fn prune(marks: &Path) {
    let Ok(entries) = fs::read_dir(marks) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|since| since > KEEP_PICTURES_FOR);
        if old {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod leaving_a_mark {
    use super::*;

    /// A home and a project of our own. `HOME` is process-wide, so these run one at a time.
    struct Bench {
        home: PathBuf,
        project: PathBuf,
        was: Option<std::ffi::OsString>,
        _held: std::sync::MutexGuard<'static, ()>,
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            match &self.was {
                Some(was) => std::env::set_var("HOME", was),
                None => std::env::remove_var("HOME"),
            }
            let _ = fs::remove_dir_all(&self.home);
        }
    }

    fn bench() -> Bench {
        let held = crate::hold_the_environment();
        let home = std::env::temp_dir().join(format!("colai-outbox-{}", now_ms()));
        let project = home.join("a-project");
        fs::create_dir_all(&project).expect("a project");
        let was = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);
        std::env::remove_var("XDG_CONFIG_HOME");
        Bench { home, project, was, _held: held }
    }

    /// One red pixel, which is a real PNG and small enough to write inline.
    fn a_picture() -> String {
        base64::engine::general_purpose::STANDARD.encode(
            [
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
                0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
                0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
                0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D,
                0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
            ],
        )
    }

    #[test]
    fn the_pictures_land_where_the_session_may_read_them() {
        /*
         * The bug this is about was found by running it: with the pictures under
         * `~/.config`, the session was handed their paths and said "the read was denied".
         * A session may read what is under the directory it works in, so that is where a
         * picture has to be — anywhere else is a mark that arrives and cannot be opened.
         */
        let bench = bench();
        let project = bench.project.to_string_lossy().to_string();
        let staged = stage("a-chat", Some(&project), "what is this", &[a_picture()])
            .expect("a staged mark");
        assert_eq!(staged.pictures, 1);

        let said = fs::read_to_string(staged.at.join(SAY)).expect("the message");
        let named = said
            .lines()
            .find(|line| line.trim().ends_with(".png"))
            .expect("a picture named in the message")
            .trim()
            .to_string();
        assert!(
            Path::new(&named).starts_with(&bench.project),
            "the picture must be under the project the conversation is had in, was {named}"
        );
        assert!(Path::new(&named).is_file(), "and must actually be there");
    }

    #[test]
    fn a_directory_put_in_somebodys_repository_ignores_itself() {
        // The toolbar writing into a checkout is a liberty. Turning up in their next commit
        // would be a second one.
        let bench = bench();
        let project = bench.project.to_string_lossy().to_string();
        stage("a-chat", Some(&project), "what is this", &[a_picture()]).expect("a staged mark");
        let ignore = fs::read_to_string(bench.project.join(".colai").join(".gitignore"))
            .expect("a .gitignore");
        assert_eq!(ignore.trim(), "*");
    }

    #[test]
    fn a_conversation_with_no_directory_still_gets_its_mark() {
        // A new conversation has no recorded cwd. The mark still goes; the session asks
        // before opening the picture, which is a prompt rather than a failure.
        let _bench = bench();
        let staged = stage("a-chat", None, "what is this", &[a_picture()]).expect("a mark");
        assert_eq!(staged.pictures, 1);
        assert!(staged.at.join("mark-1.png").is_file());
    }

    #[test]
    fn a_half_written_mark_is_not_handed_over() {
        /*
         * The hook may run at any instant, including between the picture being written and
         * the question being written. A mark is built under `.part` and renamed into place
         * whole, and `waiting_for` only counts what has arrived.
         */
        let bench = bench();
        let half = outbox().expect("an outbox").join("a-chat").join("1-1.part");
        fs::create_dir_all(&half).expect("a half-written mark");
        fs::write(half.join(SAY), "half").expect("a message");
        assert_eq!(waiting_for("a-chat"), 0);

        let project = bench.project.to_string_lossy().to_string();
        stage("a-chat", Some(&project), "what is this", &[a_picture()]).expect("a mark");
        assert_eq!(waiting_for("a-chat"), 1, "and a finished one does count");
    }

    #[test]
    fn the_message_reaches_the_model_as_the_person_wrote_it() {
        let bench = bench();
        let project = bench.project.to_string_lossy().to_string();
        let staged = stage("a-chat", Some(&project), "  why is this button grey  ", &[])
            .expect("a mark");
        let said = fs::read_to_string(staged.at.join(SAY)).expect("the message");
        assert!(said.contains("why is this button grey"));
        // And with nothing to look at, it does not tell the model to read nothing.
        assert!(!said.contains("Read these first"));
    }

    #[test]
    fn marks_for_one_conversation_do_not_count_for_another() {
        let bench = bench();
        let project = bench.project.to_string_lossy().to_string();
        stage("a-chat", Some(&project), "what is this", &[]).expect("a mark");
        assert_eq!(waiting_for("a-chat"), 1);
        assert_eq!(waiting_for("another-chat"), 0);
    }
}
