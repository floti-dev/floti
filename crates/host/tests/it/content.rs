//! Every content channel through a client connection to a running host: a
//! note read and rendered, a draft rendered, a scope searched, a wikilink
//! resolved, an editor's source highlighted, and artifacts opened, a refused
//! load and a file larger than one answer included. The renderer, the tree,
//! search, the guard and the content service on their own are unit tests in
//! `workspace/render` and `workspace/content`.
//!
//! Every test makes its own folders, with a space in every path, and the
//! agent's config folder is the test's own fake home's, never the machine's.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Instant;

use floti_host::agent_config::{hash_content, normalize};
use floti_host::server::{HostConfig, HostRun, backends};
use floti_host::testkit::{PATIENCE, RunningHost, TestClient, TestDirs};
use floti_protocol::channel::{EventChannel, RequestChannel, from_payload, to_payload};
use floti_protocol::channels::host::HostStopReason;
use floti_protocol::channels::workspace::{
    ARTIFACT_PIECE_LEN, AppMode, ArtifactBytes, ArtifactBytesRequest, ArtifactHeader, ArtifactLink,
    ContentFileKind, ContentRenderRequest, ContentSearchRequest, DiscoveryResult, DiscoveryUpdated,
    HighlightSource, HostSettingsPatch, ListContentScopes, OpenArtifact, ReadArtifactBytes,
    ReadContentDocument, RenderContent, ResolveWikilink, ScanForProjects, ScanRequest, ScopedFile,
    SearchContent, SourceToRender, WikilinkRequest, WriteSettings,
};
use floti_protocol::{Bytes, Message};
use serde_json::{Value, json};

fn put(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn shown(path: &Path) -> String {
    path.display().to_string()
}

/// What a file one folder above an artifact holds, which no load may answer.
const SECRET: &str = "TOP SECRET BYTES";

// ---------------------------------------------------------------------------
// Over a client connection: an artifact opened, and every load its frame
// makes, answered by a running host through its socket.

const CAPABILITIES: &[&str] = &["files", "terminal"];
const DISCOVERY: u64 = 1;

/// A host whose one scan root is main's artifact fixture, with a client
/// connected to it that has made the folder a known scope.
struct Wire {
    host: Option<RunningHost>,
    client: TestClient,
    /// The scope as the scan found it.
    scope: String,
    notes: PathBuf,
    /// Last: fields drop in order.
    dirs: TestDirs,
}

impl Wire {
    async fn start() -> Self {
        Self::start_with(|_| {}).await
    }

    /// The same host, with `prepare` writing more into the folder before it
    /// is scanned.
    async fn start_with(prepare: impl FnOnce(&Path)) -> Self {
        let dirs = TestDirs::new();
        let root = dirs.folder("alpha project");
        let notes = root.join("notes");
        put(&notes.join("first.md"), "# First\n\nSee [[second]].\n");
        put(&notes.join("second.md"), "# Second\n");
        put(
            &notes.join("lesson.html"),
            "<!doctype html><html><head><title>Lesson</title></head><body><p>Read [[second]] and [[unwritten]].</p></body></html>\n",
        );
        put(&notes.join("chart.js"), "window.chart = 1\n");
        put(&root.join("secret.txt"), SECRET);
        prepare(&root);

        let endpoint = dirs.endpoint();
        let host = RunningHost::start(HostConfig {
            folders: dirs.folders(),
            endpoint: endpoint.clone(),
            backends: backends(&dirs.user_env(), None),
            inherited_env: std::env::vars_os()
                .filter(|(key, _)| key != "SHELL")
                .chain([(OsString::from("SHELL"), OsString::from("/bin/sh"))])
                .collect(),
            run: HostRun {
                mode: AppMode::Dev,
                home: Some(dirs.home.clone()),
                node: None,
                desktop: false,
                keeper: None,
                wsl: None,
                config_read_only: false,
                updates: None,
            },
        });
        let mut client = TestClient::connect(&endpoint, CAPABILITIES).await;
        settled(&mut client).await;

        // The folder becomes a known scope the way a user makes it one: a
        // scan root, scanned.
        client
            .request::<WriteSettings>(&HostSettingsPatch {
                scan_roots: Some(vec![shown(&root)]),
                ..HostSettingsPatch::default()
            })
            .await
            .expect("the scan root is kept");
        let found = client
            .request::<ScanForProjects>(&ScanRequest {
                include_git: Some(false),
            })
            .await
            .expect("a scan answers");
        let scope = found
            .projects
            .iter()
            .find(|project| normalize(Path::new(&project.path)) == normalize(&root))
            .unwrap_or_else(|| panic!("the scan did not find the folder: {found:?}"))
            .path
            .clone();
        Self {
            host: Some(host),
            client,
            scope,
            notes: normalize(&notes),
            dirs,
        }
    }

    async fn stop(mut self) {
        if let Some(host) = self.host.take() {
            host.stop(HostStopReason::Stop).await;
        }
    }

    async fn open(&mut self, scope: &str, file: &Path) -> Result<ArtifactLink, String> {
        self.client
            .request::<OpenArtifact>(&ScopedFile {
                scope_path: scope.to_owned(),
                path: shown(file),
            })
            .await
            .map_err(|error| error.message)
    }

    async fn load(&mut self, url: &str, offset: u64) -> ArtifactBytes {
        self.client
            .request::<ReadArtifactBytes>(&ArtifactBytesRequest {
                url: url.to_owned(),
                offset,
            })
            .await
            .unwrap_or_else(|error| panic!("{url} was not answered: {error:?}"))
    }

    /// The lesson's address and the folder part of it its neighbours share.
    async fn lesson(&mut self) -> (ArtifactLink, String) {
        let scope = self.scope.clone();
        let lesson = self.notes.join("lesson.html");
        let link = self.open(&scope, &lesson).await.expect("the lesson opens");
        let base = link.url[..link.url.rfind('/').unwrap()].to_owned();
        (link, base)
    }
}

/// Waits out the scan a host runs as it starts, so the test's own scan is
/// the one that answers.
async fn settled(client: &mut TestClient) {
    let start = client.subscribe_on(DISCOVERY, DiscoveryUpdated::NAME).await;
    if let Message::Snapshot(snapshot) = &start {
        let state: Option<DiscoveryResult> = from_payload(&snapshot.state).unwrap();
        if state.is_some() {
            return;
        }
    }
    let deadline = Instant::now() + PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match client.next(left).await {
            Some(Message::Event(event)) if event.subscription == DISCOVERY => return,
            Some(_) => continue,
            None => panic!("the host's first scan did not finish within {PATIENCE:?}"),
        }
    }
}

fn header<'a>(answer: &'a ArtifactBytes, name: &str) -> Option<&'a str> {
    answer
        .headers
        .iter()
        .find(|header| header.name == name)
        .map(|header| header.value.as_str())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_opens_an_artifact_and_its_neighbour_over_the_wire_under_a_policy_that_names_no_network()
 {
    let mut wire = Wire::start().await;
    let (link, base) = wire.lesson().await;
    assert!(
        link.url
            .starts_with(&format!("floti-artifact://{}/", link.token))
    );
    assert!(link.url.ends_with("/lesson.html"));

    // A request that leaves the offset out is answered from the first byte.
    let bare = to_payload(&json!({ "url": link.url })).unwrap();
    let entry: ArtifactBytes = from_payload(
        &wire
            .client
            .request_on(ReadArtifactBytes::NAME, bare)
            .await
            .expect("the entry is answered"),
    )
    .unwrap();
    assert_eq!(entry.status, 200);
    assert_eq!(
        header(&entry, "content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(entry.total_bytes, entry.body.len() as u64);
    let html = String::from_utf8(entry.body.0.clone()).unwrap();
    let open = "<script type=\"application/json\" data-helm-wikilinks>";
    let start = html.find(open).unwrap() + open.len();
    let end = start + html[start..].find("</script>").unwrap();
    let island: Value = serde_json::from_str(&html[start..end]).unwrap();
    assert_eq!(island, json!({ "second": true, "unwritten": false }));
    let lower = html.to_lowercase();
    for path in [
        wire.dirs.root().to_path_buf(),
        PathBuf::from(&wire.scope),
        wire.notes.clone(),
    ] {
        let path = shown(&path);
        let escaped = serde_json::to_string(&path).unwrap();
        for spelling in [
            path.clone(),
            path.replace('\\', "/"),
            escaped[1..escaped.len() - 1].to_owned(),
        ] {
            assert!(
                !lower.contains(&spelling.to_lowercase()),
                "{spelling} leaked"
            );
        }
    }

    // The entry read from an offset is the same document, sliced.
    let rest = wire.load(&link.url, 10).await;
    assert_eq!(rest.status, 200);
    assert_eq!(rest.total_bytes, entry.total_bytes);
    assert_eq!(rest.body.as_slice(), &entry.body.as_slice()[10..]);

    let sibling = wire.load(&format!("{base}/chart.js"), 0).await;
    assert_eq!(sibling.status, 200);
    assert_eq!(
        sibling.body.as_slice(),
        std::fs::read(wire.notes.join("chart.js")).unwrap()
    );
    assert_eq!(
        header(&sibling, "content-type"),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(header(&sibling, "x-content-type-options"), Some("nosniff"));
    assert_eq!(header(&sibling, "cache-control"), Some("no-store"));
    let csp = header(&sibling, "content-security-policy").unwrap();
    let directives: Vec<&str> = csp.split(';').map(str::trim).collect();
    assert!(directives.contains(&"default-src 'none'"));
    assert!(directives.contains(&"connect-src 'none'"));
    assert!(!csp.contains("http:") && !csp.contains("https:"), "{csp}");

    wire.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_is_refused_an_artifact_load_with_the_status_and_text_the_host_serves() {
    let mut wire = Wire::start().await;
    let (_, base) = wire.lesson().await;

    for escape in [
        "..%2fsecret.txt",
        "../secret.txt",
        "..%5csecret.txt",
        "%2e%2e/secret.txt",
    ] {
        let answer = wire.load(&format!("{base}/{escape}"), 0).await;
        assert_ne!(answer.status, 200, "{escape}");
        assert!(
            !String::from_utf8_lossy(answer.body.as_slice()).contains(SECRET),
            "{escape} leaked"
        );
    }

    let refused = |status: u16, text: &str| ArtifactBytes {
        status,
        headers: vec![ArtifactHeader {
            name: "content-type".into(),
            value: "text/plain; charset=utf-8".into(),
        }],
        total_bytes: text.len() as u64,
        body: Bytes(text.as_bytes().to_vec()),
    };
    let forbidden = format!("{base}/..%2fsecret.txt");
    assert_eq!(wire.load(&forbidden, 0).await, refused(403, "Forbidden"));
    // A refusal is its whole text, whatever offset was asked for.
    assert_eq!(wire.load(&forbidden, 4).await, refused(403, "Forbidden"));
    assert_eq!(
        wire.load("floti-artifact://0123456789abcdef/lesson.html", 0)
            .await,
        refused(404, "Not found")
    );
    assert_eq!(
        wire.load(&format!("{base}/missing.css"), 0).await,
        refused(404, "Not found")
    );
    assert_eq!(
        wire.load("https://x.test/lesson.html", 0).await,
        refused(400, "Bad request")
    );
    assert_eq!(
        wire.load(&format!("{base}/%zz.js"), 0).await,
        refused(400, "Bad request")
    );

    // Only a folder the host knows can be pinned.
    let stranger = wire.dirs.folder("stranger");
    put(&stranger.join("page.html"), "<p>x</p>");
    let refused = wire
        .open(&shown(&stranger), &stranger.join("page.html"))
        .await
        .unwrap_err();
    assert!(refused.contains("is not a folder Floti knows"), "{refused}");

    wire.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_artifact_larger_than_a_piece_reaches_the_client_byte_for_byte_and_the_connection_stays_up()
 {
    let mut wire = Wire::start().await;
    // About two and a half pieces of bytes that are not text.
    let len = ARTIFACT_PIECE_LEN as usize * 5 / 2 + 17;
    let video: Vec<u8> = (0..len).map(|at| (at * 31 % 251) as u8 | 0x80).collect();
    std::fs::write(wire.notes.join("clip.bin"), &video).unwrap();
    let (_, base) = wire.lesson().await;
    let url = format!("{base}/clip.bin");

    let mut arrived = Vec::with_capacity(len);
    let mut pieces = 0;
    loop {
        let piece = wire.load(&url, arrived.len() as u64).await;
        assert_eq!(piece.status, 200);
        assert_eq!(piece.total_bytes, len as u64);
        assert_eq!(
            header(&piece, "content-type"),
            Some("application/octet-stream")
        );
        assert!(piece.body.len() as u64 <= ARTIFACT_PIECE_LEN);
        assert!(!piece.body.is_empty(), "a piece before the end is empty");
        arrived.extend_from_slice(piece.body.as_slice());
        pieces += 1;
        if arrived.len() as u64 == piece.total_bytes {
            break;
        }
    }
    assert_eq!(pieces, 3);
    assert!(arrived == video, "the pieces do not add up to the file");

    // Past the end there is nothing more, and that is not a refusal.
    let past = wire.load(&url, len as u64 + 5).await;
    assert_eq!((past.status, past.total_bytes), (200, len as u64));
    assert!(past.body.is_empty());

    // The same connection still answers.
    let scopes = wire
        .client
        .request::<ListContentScopes>(&())
        .await
        .expect("the connection is still up");
    assert!(
        scopes.iter().any(|scope| scope.path == wire.scope),
        "{scopes:?}"
    );

    wire.stop().await;
}

/// A note with everything the renderer reports on: frontmatter, two
/// headings, a live and a broken wikilink, a tag, markup that must not
/// survive, a fence with a grammar and one without.
const WIRE_NOTE: &str = "---\n\
type: journal\n\
date: 2026-08-10\n\
---\n\
\n\
# Alpha\n\
\n\
## Two Words\n\
\n\
Links to [[second]] and [[nowhere]], tagged #wire.\n\
\n\
<script>alert(1)</script>\n\
\n\
<img src=x onerror=alert(1)>\n\
\n\
<a href=\"javascript:alert(1)\">x</a> <iframe src=\"https://x.test\"></iframe>\n\
\n\
```ts\n\
const a: number = 1\n\
```\n\
\n\
```notalanguage\n\
hello\n\
```\n";

fn same_file(resolved: Option<&str>, expected: &Path) -> bool {
    resolved.is_some_and(|path| {
        shown(&normalize(Path::new(path))).to_lowercase() == shown(expected).to_lowercase()
    })
}

fn assert_no_script_survives(html: &str) {
    let lower = html.to_lowercase();
    for banned in ["<script", "onerror", "javascript:", "<iframe"] {
        assert!(!lower.contains(banned), "{banned} survived: {html}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_reads_and_renders_a_note_over_the_wire_sanitised_with_its_chips_headings_wikilinks_tags_and_counts()
 {
    let mut wire =
        Wire::start_with(|root| put(&root.join("notes").join("alpha.md"), WIRE_NOTE)).await;
    let scope = wire.scope.clone();
    let alpha = wire.notes.join("alpha.md");

    let document = wire
        .client
        .request::<ReadContentDocument>(&ScopedFile {
            scope_path: scope.clone(),
            path: shown(&alpha),
        })
        .await
        .expect("the note is read");
    assert_eq!(document.file.rel_path, "notes/alpha.md");
    assert_eq!(document.file.kind, ContentFileKind::Markdown);
    assert_eq!(document.file.note_type.as_deref(), Some("journal"));
    assert_eq!(document.content.content, WIRE_NOTE);
    assert_eq!(document.content.hash, hash_content(WIRE_NOTE));
    assert_eq!(document.error, None);

    let rendered = document.rendered.expect("a note is rendered");
    assert_no_script_survives(&rendered.html);
    assert!(
        !rendered.html.contains("type: journal"),
        "{}",
        rendered.html
    );
    assert!(rendered.frontmatter.present);
    let chips: Vec<(&str, &str)> = rendered
        .frontmatter
        .fields
        .iter()
        .map(|chip| (chip.key.as_str(), chip.value.as_str()))
        .collect();
    assert_eq!(chips, [("type", "journal"), ("date", "2026-08-10")]);
    let headings: Vec<(u32, &str, &str)> = rendered
        .headings
        .iter()
        .map(|heading| (heading.depth, heading.text.as_str(), heading.slug.as_str()))
        .collect();
    assert_eq!(
        headings,
        [(1, "Alpha", "alpha"), (2, "Two Words", "two-words")]
    );
    let second = rendered
        .links
        .iter()
        .find(|link| link.target == "second")
        .expect("the live link is listed");
    assert!(
        same_file(second.resolved.as_deref(), &wire.notes.join("second.md")),
        "{second:?}"
    );
    let nowhere = rendered
        .links
        .iter()
        .find(|link| link.target == "nowhere")
        .expect("the broken link is listed");
    assert_eq!(nowhere.resolved, None);
    assert!(rendered.html.contains("wikilink-broken"));
    assert_eq!(rendered.tags, ["wire"]);
    assert_eq!(rendered.counts.headings, 2);
    assert_eq!(rendered.counts.wikilinks, 2);
    assert_eq!(rendered.counts.broken_wikilinks, 1);
    assert_eq!(rendered.counts.tags, 1);
    assert_eq!(rendered.counts.code_blocks, 2);
    assert_eq!(rendered.counts.highlighted_blocks, 1);
    assert!(rendered.html.contains("data-language=\"typescript\""));
    assert!(rendered.html.contains("--shiki-light"));
    assert!(rendered.html.contains("--shiki-dark"));
    assert_eq!(rendered.unknown_languages, ["notalanguage"]);
    assert!(rendered.words > 0);

    // A draft renders against the same scope, before it is on disk.
    let draft = wire
        .client
        .request::<RenderContent>(&ContentRenderRequest {
            scope_path: scope.clone(),
            path: shown(&alpha),
            source: "# Draft\n\nNow [[first]] and [[gamma]].\n\n<script>alert(2)</script>\n"
                .to_owned(),
        })
        .await
        .expect("the draft is rendered");
    assert_no_script_survives(&draft.html);
    assert_eq!(draft.counts.wikilinks, 2);
    assert_eq!(draft.counts.broken_wikilinks, 1);
    assert!(
        draft.links.iter().any(|link| link.target == "first"
            && same_file(link.resolved.as_deref(), &wire.notes.join("first.md"))),
        "{:?}",
        draft.links
    );
    assert!(!draft.frontmatter.present);

    // A file that went away says so; one outside every scope is refused.
    let gone = wire
        .client
        .request::<ReadContentDocument>(&ScopedFile {
            scope_path: scope.clone(),
            path: shown(&wire.notes.join("gone.md")),
        })
        .await
        .expect("a missing file is answered");
    assert_eq!(
        gone.error.as_deref(),
        Some("That file is not there any more.")
    );
    assert!(!gone.content.exists);
    let stranger = wire.dirs.folder("stranger");
    put(&stranger.join("note.md"), "# Not here\n");
    assert!(
        wire.client
            .request::<ReadContentDocument>(&ScopedFile {
                scope_path: shown(&stranger),
                path: shown(&stranger.join("note.md")),
            })
            .await
            .is_err()
    );

    wire.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_searches_a_scope_and_resolves_a_wikilink_over_the_wire() {
    let mut wire = Wire::start_with(|root| {
        put(
            &root.join("notes").join("beta.md"),
            "# Beta\n\nAlpha mentions geofencing here.\n",
        );
    })
    .await;
    let scope = wire.scope.clone();

    let search = |query: &str| ContentSearchRequest {
        scope_path: scope.clone(),
        query: query.to_owned(),
    };
    let first = wire
        .client
        .request::<SearchContent>(&search("geofenc"))
        .await
        .expect("the search answers");
    assert_eq!(first.query, "geofenc");
    assert!(first.cold);
    assert_eq!(first.total_matches, 1);
    let hit = match first.hits.as_slice() {
        [hit] => hit,
        hits => panic!("one file holds the term: {hits:?}"),
    };
    assert_eq!(hit.rel_path, "notes/beta.md");
    assert_eq!(hit.matches, 1);
    assert!(!hit.name_match);
    let line = &hit.lines[0];
    assert_eq!(line.line, 3);
    assert_eq!(&line.text[line.from as usize..line.to as usize], "geofenc");
    assert!(first.body_kinds.contains(&ContentFileKind::Markdown));
    assert!(!first.body_kinds.contains(&ContentFileKind::Html));

    // The second search is served warm; an HTML file matches by its name.
    let warm = wire
        .client
        .request::<SearchContent>(&search("lesson"))
        .await
        .expect("the search answers");
    assert!(!warm.cold);
    assert!(
        warm.hits
            .iter()
            .any(|hit| hit.rel_path == "notes/lesson.html" && hit.name_match),
        "{:?}",
        warm.hits
    );
    let none = wire
        .client
        .request::<SearchContent>(&search("nothing matches this"))
        .await
        .expect("the search answers");
    assert_eq!((none.total_matches, none.hits.len()), (0, 0));

    // A link an artifact's frame clicks resolves beside the artifact.
    let lesson = shown(&wire.notes.join("lesson.html"));
    let resolve = |target: &str| WikilinkRequest {
        scope_path: scope.clone(),
        target: target.to_owned(),
        from: lesson.clone(),
    };
    let found = wire
        .client
        .request::<ResolveWikilink>(&resolve("second"))
        .await
        .expect("the link is resolved");
    assert!(
        same_file(found.path.as_deref(), &wire.notes.join("second.md")),
        "{found:?}"
    );
    let missing = wire
        .client
        .request::<ResolveWikilink>(&resolve("unwritten"))
        .await
        .expect("a link to nothing is answered");
    assert_eq!(missing.path, None);

    wire.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_highlights_an_editors_source_over_the_wire() {
    let mut wire = Wire::start().await;

    let small = wire
        .client
        .request::<HighlightSource>(&SourceToRender {
            path: "notes/small.ts".to_owned(),
            source: "const value = 1\nconst other = 2\n".to_owned(),
        })
        .await
        .expect("a small source is highlighted");
    assert!(small.highlighted);
    assert!(!small.too_large);
    assert_eq!(small.language, "typescript");
    assert!(small.lines.len() >= 2, "{:?}", small.lines);
    assert!(
        small.lines[0].contains("--shiki-dark"),
        "{}",
        small.lines[0]
    );

    // Past the ceiling the editor is told so, and gets no lines.
    let big = "const value = \"x\"\n".repeat((512 * 1024 + 1024) / 18 + 1);
    let too_large = wire
        .client
        .request::<HighlightSource>(&SourceToRender {
            path: "notes/big.ts".to_owned(),
            source: big,
        })
        .await
        .expect("a large source is answered");
    assert!(too_large.too_large);
    assert!(!too_large.highlighted);
    assert!(too_large.lines.is_empty());

    wire.stop().await;
}
