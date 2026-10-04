//! Undoing an edit the toolbar only watched.
//!
//! A mark handed to a terminal is answered in that session's own process; colai sees the edit
//! afterwards, off the transcript, and cannot ask that process to put it back — `colai_undo`'s
//! rewind speaks to colai's own agent over its control pipe, not to someone else's terminal. So
//! "Undo" here reverses the edit on disk directly, from what the edit itself recorded: an `Edit`
//! swapped `old` for `new`, so undoing it swaps `new` back for `old`. A full-file `Write` keeps no
//! prior copy, so it cannot be reversed this way — it is reported rather than guessed at.
//!
//! The same walk answers "what would putting these back do?" without touching disk (`dry_run`),
//! which is how the Work panel previews "Rewind files to here" before anybody commits to it.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// One edit to put back, as the card recorded it from the agent's own tool call.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditToUndo {
    pub path: String,
    /// `"edit"` — reverse a single string swap. `"multiedit"` is named as such when it is turned
    /// down; anything else (a full-file write, a notebook edit) keeps no prior copy here and
    /// cannot be reversed from what was recorded.
    pub kind: String,
    #[serde(default)]
    pub old: String,
    #[serde(default)]
    pub new: String,
}

/// What putting a list of edits back did — or, in a dry run, would do.
///
/// Both halves always present, so the page never has to tell "nothing restored" from "no
/// answer". One entry per edit, newest first (the order they come off in), so a file edited
/// twice appears twice.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Reverted {
    pub restored: Vec<PutBack>,
    pub skipped: Vec<LeftAlone>,
}

/// An edit that was (or would be) put back.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PutBack {
    /// The file's own name, which is what a person reads in a sentence about it.
    pub file: String,
    /// The whole path, for telling two files of one name apart.
    pub path: String,
}

/// An edit that was (or would be) left as it is, and why, in words for the person.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LeftAlone {
    pub file: String,
    pub path: String,
    pub why: String,
}

/// Put back every edit in `edits`, newest first, reversing each on disk — or, with `dry_run`,
/// say what that would do and write nothing.
///
/// Never a whole-call error, so one unreversible write does not strand the edits that *can* be
/// undone: each edit lands in `restored` or `skipped`. `dry_run` is optional so the response
/// card's Undo, which predates it, keeps working.
#[tauri::command]
pub(crate) async fn colai_revert_edits(
    edits: Vec<EditToUndo>,
    dry_run: Option<bool>,
) -> Result<Reverted, String> {
    Ok(revert_all(&edits, dry_run.unwrap_or(false)))
}

/// The pure core: reverse each edit, last first, sorting each into restored or skipped.
///
/// Every file is read once and the edits are applied to that copy in turn, so a dry run of two
/// overlapping edits gives the same answer as really doing them — the second is judged against
/// the text the first left, not against the file as it sits on disk.
pub(crate) fn revert_all(edits: &[EditToUndo], dry_run: bool) -> Reverted {
    let mut out = Reverted::default();
    let mut texts: HashMap<&str, String> = HashMap::new();
    // Last edit first: overlapping edits to one file are laid down in order, so they come off in
    // reverse — otherwise the second edit's `new` would no longer be found once the first was undone.
    for edit in edits.iter().rev() {
        let file = short_name(&edit.path);
        match revert_one(edit, &mut texts, dry_run) {
            Ok(()) => out.restored.push(PutBack { file, path: edit.path.clone() }),
            Err(why) => out.skipped.push(LeftAlone { file, path: edit.path.clone(), why }),
        }
    }
    out
}

/// The last path segment, which is what a person reads in a message about a file.
fn short_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

fn revert_one<'a>(
    edit: &'a EditToUndo,
    texts: &mut HashMap<&'a str, String>,
    dry_run: bool,
) -> Result<(), String> {
    match edit.kind.to_ascii_lowercase().replace('_', "").as_str() {
        "edit" => {}
        "multiedit" => return Err("MultiEdit".into()),
        _ => return Err("it was a Write".into()),
    }
    // Text that was only removed leaves nothing behind to find, so there is no telling where to
    // put it back.
    if edit.new.is_empty() {
        return Err("a pure deletion".into());
    }
    let text = match texts.get(edit.path.as_str()) {
        Some(text) => text.clone(),
        None => fs::read_to_string(Path::new(&edit.path))
            .map_err(|trouble| format!("could not read it ({trouble})"))?,
    };
    let hits = text.matches(&edit.new).count();
    if hits == 0 {
        return Err("the text has changed since".into());
    }
    if hits > 1 {
        return Err("the text appears more than once".into());
    }
    let put_back = text.replacen(&edit.new, &edit.old, 1);
    if !dry_run {
        fs::write(Path::new(&edit.path), &put_back)
            .map_err(|trouble| format!("could not write it ({trouble})"))?;
    }
    texts.insert(edit.path.as_str(), put_back);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(path: &str, old: &str, new: &str) -> EditToUndo {
        EditToUndo { path: path.into(), kind: "edit".into(), old: old.into(), new: new.into() }
    }
    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("colai-revert-{}-{}", std::process::id(), name))
    }
    fn whys(out: &Reverted) -> Vec<&str> {
        out.skipped.iter().map(|one| one.why.as_str()).collect()
    }

    #[test]
    fn an_edit_is_reversed_by_swapping_the_new_text_back_for_the_old() {
        let p = tmp("one.css");
        fs::write(&p, "a { color: #f87171; } b { x: 1 }").unwrap();
        let out = revert_all(&[edit(p.to_str().unwrap(), "color: #fff", "color: #f87171")], false);
        assert!(out.skipped.is_empty(), "nothing should be skipped: {out:?}");
        assert_eq!(out.restored.len(), 1);
        assert_eq!(out.restored[0].file, p.file_name().unwrap().to_str().unwrap());
        assert_eq!(fs::read_to_string(&p).unwrap(), "a { color: #fff; } b { x: 1 }");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn overlapping_edits_to_one_file_come_off_newest_first() {
        // Two edits applied in order v1→v2→v3; undoing both must land back on v1.
        let p = tmp("two.txt");
        fs::write(&p, "v3").unwrap();
        let path = p.to_str().unwrap();
        let out = revert_all(&[edit(path, "v1", "v2"), edit(path, "v2", "v3")], false);
        assert!(out.skipped.is_empty(), "both reverse cleanly: {out:?}");
        assert_eq!(out.restored.len(), 2);
        assert_eq!(fs::read_to_string(&p).unwrap(), "v1");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn a_dry_run_writes_nothing_and_answers_as_the_real_run_would() {
        let p = tmp("dry.txt");
        fs::write(&p, "v3").unwrap();
        let path = p.to_str().unwrap();
        let edits = [edit(path, "v1", "v2"), edit(path, "v2", "v3"), edit(path, "x", "MISSING")];
        let looked = revert_all(&edits, true);
        assert_eq!(fs::read_to_string(&p).unwrap(), "v3", "a dry run must not touch the file");
        // The second edit is judged against what the first would have left, not against disk.
        assert_eq!(looked.restored.len(), 2, "{looked:?}");
        assert_eq!(whys(&looked), ["the text has changed since"]);

        let done = revert_all(&edits, false);
        assert_eq!(done, looked, "the preview must say exactly what the real run then does");
        assert_eq!(fs::read_to_string(&p).unwrap(), "v1");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn a_change_that_is_gone_or_ambiguous_is_reported_not_forced() {
        let p = tmp("gone.txt");
        fs::write(&p, "nothing to see").unwrap();
        let out = revert_all(&[edit(p.to_str().unwrap(), "old", "MISSING")], false);
        assert!(out.restored.is_empty());
        assert_eq!(whys(&out), ["the text has changed since"]);

        fs::write(&p, "dup dup").unwrap();
        let out = revert_all(&[edit(p.to_str().unwrap(), "x", "dup")], false);
        assert_eq!(whys(&out), ["the text appears more than once"]);
        assert_eq!(fs::read_to_string(&p).unwrap(), "dup dup", "an ambiguous edit is left untouched");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn what_cannot_be_reversed_says_which_kind_of_change_it_was() {
        let of = |kind: &str, new: &str| EditToUndo {
            path: "dir/whatever.html".into(), kind: kind.into(), old: "gone".into(), new: new.into(),
        };
        let out = revert_all(&[of("write", "x"), of("MultiEdit", "x"), of("edit", "")], true);
        assert!(out.restored.is_empty());
        // Newest first: the order they would come off in.
        assert_eq!(whys(&out), ["a pure deletion", "MultiEdit", "it was a Write"]);
        assert!(out.skipped.iter().all(|one| one.file == "whatever.html"));
    }
}
