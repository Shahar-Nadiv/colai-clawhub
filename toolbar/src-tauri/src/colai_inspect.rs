//! What a window says it is showing, as the desktop itself reports it.
//!
//! Only ever asked one question: what is the address of the page or folder in front of
//! you. A browser knows its own URL exactly, and a mark that carries it saves an agent
//! the round trip of working out which page a screenshot came from — which is the whole
//! reason any of this is here.
//!
//! Over the D-Bus connection the app already has a client for, rather than the `atspi`
//! crate: the interfaces used here are two methods on a stable specification, and taking
//! a dependency to spell them differently would mean owning its version alignment with
//! zbus forever.
//!
//! It is honest about its reach. GTK applications answer; Chromium and Electron ones
//! expose nothing unless they believe a screen reader is present, which on a developer's
//! desk is most of the windows. Measured on this machine: holding a connection open does
//! not wake them — accessibility has to be switched on. So a window that says nothing
//! produces no address rather than an invented one, and the message says the URL was not
//! available rather than going quiet about it.
use serde::Serialize;
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, Proxy};

/// An AT-SPI object: which application answers for it, and where it lives there.
type Node = (String, OwnedObjectPath);

/// Screen coordinates rather than window-relative ones.
const ON_SCREEN: u32 = 0;
/// How long a reading taken for a mark's address may take.
///
/// Short, because nobody asked for it. Marking is instant today and has to stay that
/// way; an address is worth a quarter of a second and not a whole one.
const BRIEFLY: std::time::Duration = std::time::Duration::from_millis(250);

/// The address of the page or folder a window is showing, if it will say.
///
/// Asked only where there is an answer worth waiting for. A browser has a URL and a file
/// manager has a folder; an editor, a terminal and a game have nothing here that the
/// window's own process did not already say better, so they are not asked at all and
/// marking in them costs exactly nothing.
///
/// The kind is decided by what the window exposes rather than by a list of brand names:
/// a window with a `Document` in it is showing a document, whatever it is called.
#[tauri::command]
pub(crate) async fn colai_showing(x: i32, y: i32) -> Option<Showing> {
    #[cfg(target_os = "linux")]
    {
        match tokio::time::timeout(BRIEFLY, showing_at(x, y)).await {
            Ok(seen) => seen,
            // A desktop that will not answer in a quarter of a second has told us
            // nothing, which is the same outcome as one with nothing to tell.
            Err(_) => None,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (x, y);
        None
    }
}

/// What a window says it is showing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Showing {
    pub url: Option<String>,
}

#[cfg(target_os = "linux")]
async fn showing_at(x: i32, y: i32) -> Option<Showing> {
    // Twice at most: a kept connection can have died with the session's accessibility
    // helper, and the cheapest proof that it is alive is asking it something. A second
    // failure is the bus being genuinely absent, which is an answer.
    match look_from(&kept_bus().await.ok()?, x, y).await {
        Ok(seen) => seen,
        Err(()) => {
            forget_the_bus().await;
            look_from(&kept_bus().await.ok()?, x, y).await.unwrap_or(None)
        }
    }
}

/// The walk itself, over whichever connection it was handed.
///
/// `Err` means the connection would not answer at all, which is the one case worth
/// retrying. A walk that completes and finds nothing is `Ok(None)` — an answer.
#[cfg(target_os = "linux")]
async fn look_from(bus: &Connection, x: i32, y: i32) -> Result<Option<Showing>, ()> {
    let root: Node = (
        "org.a11y.atspi.Registry".to_string(),
        OwnedObjectPath::try_from("/org/a11y/atspi/accessible/root").map_err(|_| ())?,
    );
    for app in children(bus, &root).await.map_err(|_| ())? {
        // One application refusing to answer is that application's business, not a
        // broken bus — it is skipped, and the rest are still asked.
        for window in children(bus, &app).await.unwrap_or_default() {
            if !contains(bus, &window, x, y).await {
                continue;
            }
            let url = document_url(bus, &window).await;
            return Ok(Some(Showing { url }));
        }
    }
    Ok(None)
}

/// How far into a window to look for the thing that knows its own address.
///
/// A browser puts its document two or three levels below the window. Beyond that is
/// somebody else's document — a frame, an embedded view — and answering with the address
/// of a thing nobody pointed at is worse than answering with nothing.
#[cfg(target_os = "linux")]
const DOCUMENT_WITHIN: usize = 4;

#[cfg(target_os = "linux")]
async fn document_url(bus: &Connection, window: &Node) -> Option<String> {
    let mut looking = vec![(window.clone(), 0usize)];
    while let Some((node, depth)) = looking.pop() {
        if let Some(paper) = on(bus, &node, "org.a11y.atspi.Document").await {
            if let Ok(url) = paper
                .call::<_, _, String>("GetAttributeValue", &("DocURL",))
                .await
            {
                let url = url.trim().to_string();
                // An empty attribute is the interface saying "I have no document",
                // which is not the same as a document at the empty address.
                if !url.is_empty() {
                    return Some(url);
                }
            }
        }
        if depth + 1 >= DOCUMENT_WITHIN {
            continue;
        }
        for child in children(bus, &node).await.unwrap_or_default() {
            looking.push((child, depth + 1));
        }
    }
    None
}

/// The accessibility connection, kept rather than rebuilt.
///
/// Every reading used to open two connections — the session bus, to be told the a11y
/// bus's address, then the a11y bus itself — and drop them both. That is two handshakes
/// for a question asked once per window, in front of somebody who is marking something.
///
/// Safe to hold by this module's own measurement, recorded in the header above: keeping
/// a connection open does not wake applications that are not exposing themselves. It
/// changes what the reading costs, not what it can see.
#[cfg(target_os = "linux")]
static A11Y: tokio::sync::Mutex<Option<Connection>> = tokio::sync::Mutex::const_new(None);

/// The connection, opening one if there is not one already.
///
/// A dropped connection is ordinary — the accessibility bus goes away with the session's
/// helper and comes back — so a dead one is replaced rather than being an error to
/// report. Liveness is proven by asking it something, in `showing_at`, rather than by a
/// separate question that would be one more round trip and could still race.
#[cfg(target_os = "linux")]
async fn kept_bus() -> Result<Connection, String> {
    let mut held = A11Y.lock().await;
    if let Some(bus) = held.as_ref() {
        return Ok(bus.clone());
    }
    let fresh = accessibility_bus().await?;
    *held = Some(fresh.clone());
    Ok(fresh)
}

/// Drop the kept connection, so the next reading opens a new one.
#[cfg(target_os = "linux")]
async fn forget_the_bus() {
    *A11Y.lock().await = None;
}

#[cfg(target_os = "linux")]
async fn accessibility_bus() -> Result<Connection, String> {
    let session = Connection::session()
        .await
        .map_err(|error| format!("No session bus: {error}"))?;
    let door = Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus")
        .await
        .map_err(|error| format!("No accessibility bus: {error}"))?;
    let address: String = door
        .call("GetAddress", &())
        .await
        .map_err(|error| format!("The accessibility bus has no address: {error}"))?;
    zbus::connection::Builder::address(address.as_str())
        .map_err(|error| format!("Could not read that address: {error}"))?
        .build()
        .await
        .map_err(|error| format!("Could not reach the accessibility bus: {error}"))
}

#[cfg(target_os = "linux")]
async fn on<'a>(bus: &'a Connection, node: &Node, face: &'static str) -> Option<Proxy<'a>> {
    Proxy::new(bus, node.0.clone(), node.1.clone(), face)
        .await
        .ok()
}

#[cfg(target_os = "linux")]
async fn children(bus: &Connection, node: &Node) -> Result<Vec<Node>, ()> {
    let seat = on(bus, node, "org.a11y.atspi.Accessible").await.ok_or(())?;
    seat.call("GetChildren", &()).await.map_err(|_| ())
}

#[cfg(target_os = "linux")]
async fn contains(bus: &Connection, node: &Node, x: i32, y: i32) -> bool {
    let Some(shape) = on(bus, node, "org.a11y.atspi.Component").await else {
        return false;
    };
    shape
        .call("Contains", &(x, y, ON_SCREEN))
        .await
        .unwrap_or(false)
}
