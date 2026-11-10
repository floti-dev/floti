//! The agents folder, watched while a client listens for `agents:changed`.
//!
//! What counts is an agent file coming, going or having other bytes: the
//! folder is measured by each agent file's hash ([`fingerprint`]), before
//! and after each burst of the OS's events, and only the names whose hash
//! moved are told. So the host's own write and an outside editor's are told
//! alike, a burst an editor makes (a write, a truncate, a write again, a
//! temporary file renamed over the file) is told once, and an event that
//! changed nothing, such as a file being read, is told never.
//!
//! The folder and the folder that holds it are each watched on their own,
//! not recursively: the folder may not exist yet, may be deleted and may be
//! made again, and the watch on the one that holds it sees that and watches
//! the folder anew. A burst is measured [`SETTLE`] after its first event, so
//! a stream of writes is told while it goes on.
//!
//! The folder is watched only while a client listens ([`Listening`]): the
//! first listener starts the watch and the last one to leave stops it.
//!
//! The OS's watch is `notify`'s, which is the one interface over each OS's
//! own; nothing here differs by OS.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::{DEFAULT_AGENT_NAME, is_agent_name, is_default_name};
use crate::agent_config::sha256_hex;

/// How long after a burst's first event the folder is measured.
pub const SETTLE: Duration = Duration::from_millis(200);

/// Each agent file in a folder, by name, with what its bytes hash to: what a
/// change is measured by.
pub type Fingerprint = BTreeMap<String, String>;

/// The agent files in `dir` now: each `<name>.md` file whose name is an
/// agent name. Empty when there is no folder.
pub fn fingerprint(dir: &Path) -> Fingerprint {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Fingerprint::new();
    };
    entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "md" {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_owned();
            if !is_agent_name(&name) || !path.is_file() {
                return None;
            }
            // A file that cannot be read is still there; which error it
            // gives says when that changes.
            let hash = match std::fs::read(&path) {
                Ok(bytes) => sha256_hex(&bytes),
                Err(err) => format!("unreadable: {:?}", err.kind()),
            };
            Some((name, hash))
        })
        .collect()
}

/// The agents whose file came, went or has other bytes from `before` to
/// `after`, sorted, each once and named as `agents:list` names it: an
/// `agent.md` from before agents were called agents is the default's.
pub fn changed(before: &Fingerprint, after: &Fingerprint) -> Vec<String> {
    let names: BTreeSet<&str> = before
        .iter()
        .filter(|(name, hash)| after.get(*name) != Some(*hash))
        .map(|(name, _)| name.as_str())
        .chain(
            after
                .keys()
                .filter(|name| !before.contains_key(*name))
                .map(String::as_str),
        )
        .map(|name| {
            if is_default_name(name) {
                DEFAULT_AGENT_NAME
            } else {
                name
            }
        })
        .collect();
    names.into_iter().map(str::to_owned).collect()
}

/// Who listens for changes, by key, and the watch kept while anyone does.
#[derive(Debug)]
pub struct Listening<K, W> {
    listeners: BTreeSet<K>,
    watch: Option<W>,
}

impl<K: Ord, W> Default for Listening<K, W> {
    fn default() -> Self {
        Self {
            listeners: BTreeSet::new(),
            watch: None,
        }
    }
}

impl<K: Ord, W> Listening<K, W> {
    /// `listener` listens now, or no longer. The first to listen starts the
    /// watch with `start`, which may find nothing to watch and is asked
    /// again by the next; the last to leave drops it. `start` is never
    /// called while nobody listens.
    pub fn set(&mut self, listener: K, listens: bool, start: impl FnOnce() -> Option<W>) {
        if listens {
            self.listeners.insert(listener);
        } else {
            self.listeners.remove(&listener);
        }
        if self.listeners.is_empty() {
            self.watch = None;
        } else if self.watch.is_none() {
            self.watch = start();
        }
    }

    /// The watch, while anyone listens and it could be started.
    pub fn watch(&self) -> Option<&W> {
        self.watch.as_ref()
    }
}

/// Told the names of the agents whose files changed.
pub type OnChange = Arc<dyn Fn(Vec<String>) + Send + Sync>;

/// One folder's watch. Dropping it stops the OS's watch and any measuring
/// already due.
pub struct AgentsWatch {
    /// Asks for the folder to be measured as if the OS had told a change.
    poke: mpsc::UnboundedSender<bool>,
    task: JoinHandle<()>,
}

impl Drop for AgentsWatch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl AgentsWatch {
    /// Watches the agents folder `dir`, on `runtime`, telling `on_change`
    /// each burst that changed an agent file. The folder need not exist. A
    /// folder whose OS will not watch it is still measured when it is
    /// [poked](Self::poke).
    pub fn start(dir: &Path, runtime: &tokio::runtime::Handle, on_change: OnChange) -> Self {
        let baseline = fingerprint(dir);
        let (poke, events) = mpsc::unbounded_channel::<bool>();
        let watcher = watcher(dir, poke.clone()).map(|watcher| Arc::new(Mutex::new(watcher)));
        if let Some(watcher) = &watcher {
            let mut watching = watcher.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(parent) = dir.parent() {
                let _ = watching.watch(parent, RecursiveMode::NonRecursive);
            }
            let _ = watching.watch(dir, RecursiveMode::NonRecursive);
        }
        let folder = dir.to_path_buf();
        let task = runtime.spawn(settle(
            events,
            SETTLE,
            move |moved| {
                let folder = folder.clone();
                let watcher = watcher.clone();
                async move {
                    tokio::task::spawn_blocking(move || {
                        if moved && let Some(watcher) = &watcher {
                            rewatch(watcher, &folder);
                        }
                        fingerprint(&folder)
                    })
                    .await
                    .unwrap_or_default()
                }
            },
            baseline,
            on_change,
        ));
        Self { poke, task }
    }

    /// Measures the folder soon, as if the OS had told a change: after the
    /// host's own change, which a folder the OS does not watch would not
    /// tell.
    pub fn poke(&self) {
        let _ = self.poke.send(false);
    }
}

/// The OS's watch, which sends `true` for an event about the folder itself
/// (it came, went or was renamed), and `false` for one about an agent file
/// in it.
fn watcher(dir: &Path, events: mpsc::UnboundedSender<bool>) -> Option<RecommendedWatcher> {
    // The folder as the OS may spell it in its events: macOS reports a
    // temporary folder under /private/var, which is /var as written.
    let spellings: Vec<PathBuf> = std::iter::once(dir.to_path_buf())
        .chain(
            dir.parent()
                .and_then(|parent| std::fs::canonicalize(parent).ok())
                .zip(dir.file_name())
                .map(|(parent, name)| parent.join(name)),
        )
        .collect();
    notify::recommended_watcher(move |event: notify::Result<Event>| {
        let Ok(event) = event else {
            // The handle lost: anything may have changed.
            let _ = events.send(true);
            return;
        };
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        if event.need_rescan() {
            let _ = events.send(true);
            return;
        }
        for path in &event.paths {
            if spellings.iter().any(|folder| folder == path) {
                let _ = events.send(moves_the_folder(&event.kind));
            } else if path.extension().is_some_and(|extension| extension == "md") {
                let _ = events.send(false);
            }
        }
    })
    .ok()
}

/// Whether an event about the folder itself means it came, went or moved,
/// so that its watch has to be made again.
fn moves_the_folder(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Name(_))
            | EventKind::Any
            | EventKind::Other
    )
}

/// Watches `dir` anew: it came, went or was made again.
fn rewatch(watcher: &Mutex<RecommendedWatcher>, dir: &Path) {
    let mut watching = watcher.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = watching.unwatch(dir);
    if dir.is_dir()
        && let Err(err) = watching.watch(dir, RecursiveMode::NonRecursive)
    {
        log_line!(
            "floti host: the agents folder {} could not be watched: {err}",
            dir.display()
        );
    }
}

/// Gathers each burst of `events` for `settle` from its first, then measures
/// the folder with `scan` (told whether an event of the burst moved the
/// folder itself) and tells `on_change` the names whose files moved from the
/// last measure. A burst that moved none is let go.
pub async fn settle<F, Fut>(
    mut events: mpsc::UnboundedReceiver<bool>,
    settle: Duration,
    mut scan: F,
    mut baseline: Fingerprint,
    on_change: OnChange,
) where
    F: FnMut(bool) -> Fut,
    Fut: Future<Output = Fingerprint>,
{
    while let Some(first) = events.recv().await {
        tokio::time::sleep(settle).await;
        let mut moved = first;
        while let Ok(more) = events.try_recv() {
            moved |= more;
        }
        let fresh = scan(moved).await;
        let names = changed(&baseline, &fresh);
        baseline = fresh;
        if !names.is_empty() {
            on_change(names);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print(entries: &[(&str, &str)]) -> Fingerprint {
        entries
            .iter()
            .map(|(name, hash)| ((*name).to_owned(), (*hash).to_owned()))
            .collect()
    }

    #[test]
    fn a_file_that_came_went_or_has_other_bytes_is_changed_and_no_other_is() {
        let before = print(&[("lead", "a"), ("reviewer", "b"), ("worker", "c")]);
        let after = print(&[("lead", "a"), ("reviewer", "B"), ("scout", "d")]);
        assert_eq!(changed(&before, &after), ["reviewer", "scout", "worker"]);
        assert!(changed(&before, &before).is_empty());
    }

    #[test]
    fn an_agent_md_from_before_is_told_as_the_default_and_the_default_once() {
        let none = print(&[]);
        assert_eq!(changed(&none, &print(&[("agent", "a")])), ["default"]);
        assert_eq!(
            changed(&none, &print(&[("agent", "a"), ("default", "d")])),
            ["default"]
        );
    }

    /// A watch that counts its drops.
    struct Counted(Arc<std::sync::atomic::AtomicUsize>);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[test]
    fn the_watch_starts_with_the_first_listener_and_stops_when_the_last_leaves() {
        use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
        let dropped = Arc::new(AtomicUsize::new(0));
        let started = AtomicUsize::new(0);
        let start = || {
            started.fetch_add(1, Relaxed);
            Some(Counted(Arc::clone(&dropped)))
        };
        let mut listening: Listening<u32, Counted> = Listening::default();

        listening.set(1, false, start);
        assert_eq!(
            started.load(Relaxed),
            0,
            "nobody listens, so nothing starts"
        );
        assert!(listening.watch().is_none());

        listening.set(1, true, start);
        listening.set(2, true, start);
        listening.set(1, true, start);
        assert_eq!(started.load(Relaxed), 1, "one watch for every listener");
        listening.set(1, false, start);
        assert!(listening.watch().is_some(), "one listener is left");
        assert_eq!(dropped.load(Relaxed), 0);

        listening.set(2, false, start);
        assert!(listening.watch().is_none());
        assert_eq!(dropped.load(Relaxed), 1, "the last to leave stops it");
        listening.set(2, false, start);
        assert_eq!(started.load(Relaxed), 1, "leaving again starts nothing");

        listening.set(3, true, start);
        assert_eq!(started.load(Relaxed), 2, "a new listener starts it again");
    }

    #[test]
    fn a_folder_that_cannot_be_watched_is_asked_again_by_the_next_listener() {
        let mut listening: Listening<u32, ()> = Listening::default();
        listening.set(1, true, || None);
        assert!(listening.watch().is_none());
        listening.set(2, true, || Some(()));
        assert!(listening.watch().is_some());
    }

    #[test]
    fn the_fingerprint_is_each_agent_files_hash_and_nothing_else_in_the_folder() {
        let dirs = crate::testkit::TestDirs::new();
        let dir = dirs.folder("agents watched");
        std::fs::write(dir.join("worker.md"), "one").unwrap();
        std::fs::write(dir.join("Not An Agent.md"), "x").unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        std::fs::write(dir.join(".worker.md.floti-write"), "x").unwrap();
        std::fs::create_dir(dir.join("folder.md")).unwrap();
        assert_eq!(fingerprint(&dir), print(&[("worker", &sha256_hex(b"one"))]));
        assert!(fingerprint(&dir.join("absent")).is_empty());
    }

    /// A settling watch fed by hand, with what it told and what the folder
    /// measures as now.
    struct Settling {
        events: mpsc::UnboundedSender<bool>,
        told: Arc<Mutex<Vec<Vec<String>>>>,
        folder: Arc<Mutex<Fingerprint>>,
        moved: Arc<Mutex<Vec<bool>>>,
    }

    impl Settling {
        fn start() -> Self {
            let (events, received) = mpsc::unbounded_channel();
            let told: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
            let folder = Arc::new(Mutex::new(print(&[("worker", "one")])));
            let moved: Arc<Mutex<Vec<bool>>> = Arc::default();
            let (into, now, scans) = (Arc::clone(&told), Arc::clone(&folder), Arc::clone(&moved));
            tokio::spawn(settle(
                received,
                SETTLE,
                move |was_moved| {
                    scans.lock().unwrap().push(was_moved);
                    let now = now.lock().unwrap().clone();
                    async move { now }
                },
                print(&[("worker", "one")]),
                Arc::new(move |names| into.lock().unwrap().push(names)),
            ));
            Self {
                events,
                told,
                folder,
                moved,
            }
        }

        fn told(&self) -> Vec<Vec<String>> {
            self.told.lock().unwrap().clone()
        }

        /// Lets the watch take what was sent, then moves the clock `by`.
        async fn step(&self, by: Duration) {
            Self::settle().await;
            tokio::time::advance(by).await;
            Self::settle().await;
        }

        async fn settle() {
            for _ in 0..8 {
                tokio::task::yield_now().await;
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_burst_is_measured_once_its_settle_after_its_first_event() {
        let watch = Settling::start();
        *watch.folder.lock().unwrap() = print(&[("worker", "two")]);
        watch.events.send(false).unwrap();
        watch.step(Duration::from_millis(150)).await;
        watch.events.send(true).unwrap();
        watch.step(Duration::from_millis(49)).await;
        assert!(watch.told().is_empty(), "the burst is still settling");
        watch.step(Duration::from_millis(2)).await;
        assert_eq!(watch.told(), [vec!["worker".to_owned()]]);
        assert_eq!(
            *watch.moved.lock().unwrap(),
            [true],
            "one measure for the burst, told that the folder moved"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_burst_that_changed_no_agent_file_tells_nothing_and_the_next_change_is_told_against_the_last_measure()
     {
        let watch = Settling::start();
        watch.events.send(false).unwrap();
        watch.step(SETTLE + Duration::from_millis(1)).await;
        assert!(watch.told().is_empty(), "a file read is not a change");

        *watch.folder.lock().unwrap() = print(&[("worker", "one"), ("scout", "s")]);
        watch.events.send(false).unwrap();
        watch.step(SETTLE + Duration::from_millis(1)).await;
        *watch.folder.lock().unwrap() = print(&[("scout", "s")]);
        watch.events.send(false).unwrap();
        watch.step(SETTLE + Duration::from_millis(1)).await;
        assert_eq!(
            watch.told(),
            [vec!["scout".to_owned()], vec!["worker".to_owned()]]
        );
    }
}
