//! Every `template:*` channel and `workspace:create`, driven as a client of
//! the host drives them, over its socket. A client lists, previews and reads
//! templates, creates, describes, renames, fills and deletes one, imports a
//! skill into one, saves a folder as one, and makes a workspace from one; it
//! scans over the socket for the workspace it made and reads its scan root from
//! the settings, which a `settings:changed` subscriber on another connection
//! hears. The rules behind each channel are unit tests in
//! `workspace/templates`, and the seeding at host start is tested through the
//! host's router in `server/features/templates.rs`.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use floti_host::agent::claude_code::ClaudeCodeSettings;
use floti_host::platform::links;
use floti_host::platform::socket::Endpoint;
use floti_host::server::{HostConfig, HostRun, backends};
use floti_host::testkit::{PATIENCE, RunningHost, TestClient, TestDirs};
use floti_protocol::Message;
use floti_protocol::channel::{EventChannel, RequestChannel, from_payload};
use floti_protocol::channels::host::HostStopReason;
use floti_protocol::channels::workspace::{
    AppMode, CreateTemplate, CreateTemplateRequest, CreateWorkspace, CreateWorkspaceOutcome,
    CreateWorkspaceRequest, DeleteTemplate, FolderTemplateKind, FolderTemplatePreview,
    FolderTemplateRequest, HostSettings, ImportIntoTemplate, ImportIntoTemplateRequest,
    ListTemplates, PreviewFolderTemplate, PreviewTemplate, ReadSettings, ReadTemplateDetail,
    RenameTemplate, RenameTemplateRequest, SaveFolderAsTemplate, SaveFolderAsTemplateRequest,
    SaveFolderAsTemplateResult, ScanForProjects, ScanRequest, SettingsChanged,
    SubstituteTemplateFile, TemplateDetail, TemplateFileRequest, TemplateListing,
    TemplateMetadataRequest, TemplatePreview, TemplatePreviewRequest, TemplateRef,
    TemplateWriteResult, WorkspaceMode, WriteTemplateMetadata,
};

/// Writes `content` at `path`, making its parents.
fn write(path: &Path, content: &str) -> PathBuf {
    write_bytes(path, content.as_bytes())
}

/// Paths here are joined with forward-slashed relative parts, which every OS
/// the host runs on reads as separators.
fn write_bytes(path: &Path, content: &[u8]) -> PathBuf {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
    path.to_path_buf()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

/// Every path under a folder, relative, forward-slashed and sorted. A link
/// is listed and never entered. Knows nothing of templates.
fn tree(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, prefix: &str, into: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let path = entry.path();
            into.push(rel.clone());
            if !links::is_link(&path) && path.is_dir() {
                walk(&path, &rel, into);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out);
    out.sort();
    out
}

fn line_matches(text: &str, wanted: &str) -> bool {
    text.lines().any(|line| line == wanted)
}

fn manifest_value<'a>(manifest: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}: \"");
    manifest
        .lines()
        .find_map(|line| line.strip_prefix(&prefix)?.strip_suffix('"'))
}

fn looks_like_iso(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() > 11
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
}

/// A workspace with the things somebody would want to leave behind, under
/// `parent`.
fn plant_workspace_in(parent: &Path) -> PathBuf {
    let dir = parent.join("my-workspace");
    write(&dir.join("workspace.yaml"), "name: my-workspace\n");
    write(&dir.join("CLAUDE.md"), "# my-workspace\n");
    write(&dir.join(".claude/skills/think/SKILL.md"), "skill\n");
    write(&dir.join("notes/journal.md"), "a private journal\n");
    write(&dir.join("repos/thing/README.md"), "a checkout\n");
    write(&dir.join(".git/HEAD"), "ref: refs/heads/main\n");
    write(
        &dir.join("tools/node_modules/left-pad/index.js"),
        "module.exports = 1\n",
    );
    write(&dir.join("tools/run.mjs"), "console.log(1)\n");
    dir
}

fn ticked(preview: &floti_protocol::channels::workspace::FolderTemplatePreview) -> Vec<String> {
    preview
        .entries
        .iter()
        .filter(|entry| entry.included)
        .map(|entry| entry.name.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Over a client connection
// ---------------------------------------------------------------------------

/// A host over a test's folders, with a client connected to it. The
/// templates folder is the host's own, `templates` in its config folder,
/// seeded as the host started. Claude Code (the fake claude, never run) is
/// the agent whose config a template imports from.
struct Wire {
    host: RunningHost,
    client: TestClient,
    endpoint: Endpoint,
    templates: PathBuf,
    /// Where a test's workspaces, folders and link targets go, outside the
    /// templates folder.
    root: PathBuf,
    /// Last: fields drop in order.
    dirs: TestDirs,
}

/// The fake `claude` (`test/fake-agents/claude-code.mjs`), run by Node.
fn fake_claude() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("test")
        .join("fake-agents")
        .join("claude-code.mjs")
}

fn wire_config(dirs: &TestDirs, endpoint: &Endpoint) -> HostConfig {
    let claude = ClaudeCodeSettings {
        search_first: Vec::new(),
        program: floti_host::testkit::node().into(),
        leading_args: vec![fake_claude().into_os_string()],
        config_dir: dirs.home.join(".claude"),
        pass_config_dir: true,
        api_key_set: false,
    };
    HostConfig {
        folders: dirs.folders(),
        endpoint: endpoint.clone(),
        backends: backends(&dirs.user_env(), Some(claude)),
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
    }
}

impl Wire {
    async fn start() -> Self {
        Self::after(|_| {}).await
    }

    /// A host started once `prepare` has had the folders.
    async fn after(prepare: impl FnOnce(&TestDirs)) -> Self {
        let dirs = TestDirs::new();
        prepare(&dirs);
        let root = dirs.folder("work area");
        let endpoint = dirs.endpoint();
        let host = RunningHost::start(wire_config(&dirs, &endpoint));
        let client = TestClient::connect(&endpoint, &[]).await;
        Self {
            host,
            client,
            endpoint,
            templates: dirs.config.join("templates"),
            root,
            dirs,
        }
    }

    async fn stop(self) {
        self.host.stop(HostStopReason::Stop).await;
    }

    /// Asks `C` and expects an answer rather than an error: every refusal on
    /// these channels is a result naming its problems.
    async fn ask<C: RequestChannel>(&mut self, request: &C::Request) -> C::Response {
        self.client
            .request::<C>(request)
            .await
            .unwrap_or_else(|error| panic!("`{}` failed: {}", C::NAME, error.message))
    }

    async fn listing(&mut self) -> TemplateListing {
        self.ask::<ListTemplates>(&()).await
    }

    async fn listed_ids(&mut self) -> Vec<String> {
        self.listing()
            .await
            .templates
            .into_iter()
            .map(|choice| choice.id)
            .collect()
    }

    async fn create(&mut self, name: &str) -> TemplateWriteResult {
        self.ask::<CreateTemplate>(&CreateTemplateRequest {
            name: name.into(),
            label: None,
            description: None,
        })
        .await
    }

    async fn preview(&mut self, template: &str) -> TemplatePreview {
        self.ask::<PreviewTemplate>(&TemplatePreviewRequest {
            template: template.into(),
            mode: Some(WorkspaceMode::New),
        })
        .await
    }

    async fn detail(&mut self, template: &str) -> TemplateDetail {
        self.ask::<ReadTemplateDetail>(&TemplateRef {
            template: template.into(),
        })
        .await
    }

    async fn delete(&mut self, template: &str) -> bool {
        let result = self
            .ask::<DeleteTemplate>(&TemplateRef {
                template: template.into(),
            })
            .await;
        result.ok
    }

    async fn workspace(
        &mut self,
        dir: &Path,
        name: &str,
        template: &str,
    ) -> CreateWorkspaceOutcome {
        self.ask::<CreateWorkspace>(&CreateWorkspaceRequest {
            mode: WorkspaceMode::New,
            dir: dir.display().to_string(),
            name: Some(name.into()),
            template: Some(template.into()),
        })
        .await
    }

    async fn folder_preview(&mut self, dir: &Path) -> FolderTemplatePreview {
        self.ask::<PreviewFolderTemplate>(&FolderTemplateRequest {
            dir: dir.display().to_string(),
            kind: FolderTemplateKind::Workspace,
        })
        .await
    }

    async fn save_folder(
        &mut self,
        dir: &Path,
        name: &str,
        include: Vec<String>,
    ) -> SaveFolderAsTemplateResult {
        self.ask::<SaveFolderAsTemplate>(&SaveFolderAsTemplateRequest {
            dir: dir.display().to_string(),
            kind: FolderTemplateKind::Workspace,
            name: name.into(),
            label: "Frozen".into(),
            description: "A workspace, frozen.".into(),
            include,
        })
        .await
    }

    async fn roots(&mut self) -> Vec<String> {
        self.ask::<ReadSettings>(&()).await.scan_roots
    }

    /// Another client, subscribed to `settings:changed`, and the scan roots
    /// it started with.
    async fn settings_watcher(&self) -> (TestClient, Vec<String>) {
        let mut watcher = TestClient::connect(&self.endpoint, &[]).await;
        match watcher.subscribe_on(SETTINGS, SettingsChanged::NAME).await {
            Message::Snapshot(snapshot) => {
                let settings: HostSettings = from_payload(&snapshot.state).unwrap();
                (watcher, settings.scan_roots)
            }
            other => panic!("`settings:changed` did not start with a snapshot: {other:?}"),
        }
    }
}

/// The subscription a settings watcher holds.
const SETTINGS: u64 = 1;

/// The scan roots the next `settings:changed` event carries, or `None` when
/// none comes within `wait`.
async fn heard_roots(watcher: &mut TestClient, wait: Duration) -> Option<Vec<String>> {
    let deadline = Instant::now() + wait;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match watcher.next(left).await? {
            Message::Event(event) if event.subscription == SETTINGS => {
                let settings: HostSettings = from_payload(&event.payload).unwrap();
                return Some(settings.scan_roots);
            }
            _ => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_makes_a_workspace_from_a_template_with_its_values_filled_in_and_a_scan_lists_it()
{
    let mut wire = Wire::start().await;
    let literal = "runs-on: ${{ matrix.os }}\nname: {{NAME}}\n";
    for (relative, content) in [
        ("template.yaml", "label: \"Demo\"\n"),
        (
            "CLAUDE.md.tpl",
            "# {{NAME}}\n\nfrom {{TEMPLATE}} at {{CREATED_AT}}\n",
        ),
        ("workflow.yml", literal),
        ("dot-claude/settings.json", "{\"model\":\"opus\"}"),
        ("notes/.gitkeep", ""),
    ] {
        write(&wire.templates.join("demo").join(relative), content);
    }
    let parent = wire.root.join("workspaces");
    fs::create_dir_all(&parent).unwrap();
    let (mut watcher, roots_before) = wire.settings_watcher().await;

    let outcome = wire.harness(&parent, "work", "demo").await;

    let workspace = parent.join("work");
    let path = workspace.display().to_string();
    assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
    assert_eq!(outcome.path.as_deref(), Some(path.as_str()));
    assert!(outcome.roots.contains(&path), "{:?}", outcome.roots);
    assert!(!roots_before.contains(&path), "{roots_before:?}");
    // The new workspace joined the scan roots: a subscriber hears the whole
    // settings with it, and a read finds it there.
    let heard = heard_roots(&mut watcher, PATIENCE).await;
    assert_eq!(heard.as_ref(), Some(&outcome.roots));
    assert_eq!(wire.roots().await, outcome.roots);
    assert_eq!(
        tree(&workspace),
        [
            ".claude",
            ".claude/settings.json",
            "CLAUDE.md",
            "workspace.yaml",
            "notes",
            "workflow.yml"
        ]
    );
    let claude_md = read(&workspace.join("CLAUDE.md"));
    assert!(line_matches(&claude_md, "# work"), "{claude_md}");
    let from = claude_md
        .lines()
        .find_map(|line| line.strip_prefix("from demo at "))
        .unwrap();
    assert!(looks_like_iso(from), "{from}");
    assert_eq!(read(&workspace.join("workflow.yml")), literal);
    let manifest = read(&workspace.join("workspace.yaml"));
    assert!(line_matches(&manifest, "name: \"work\""), "{manifest}");
    assert!(line_matches(&manifest, "template: \"demo\""), "{manifest}");
    assert!(claude_md.contains(manifest_value(&manifest, "created").unwrap()));

    // The manifest is one a scan recognises: a scan over the wire lists it.
    let scanned = wire
        .ask::<ScanForProjects>(&ScanRequest {
            include_git: Some(false),
        })
        .await;
    let found: Vec<(&str, &str)> = scanned
        .harnesses
        .iter()
        .map(|workspace| (workspace.name.as_str(), workspace.path.as_str()))
        .collect();
    assert!(found.contains(&("work", path.as_str())), "{found:?}");
    let made = scanned
        .harnesses
        .iter()
        .find(|workspace| workspace.name == "work")
        .unwrap();
    assert_eq!(made.template.as_deref(), Some("demo"));

    wire.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_lists_previews_and_reads_templates_and_a_broken_manifest_does_not_fail_the_listing()
 {
    let mut wire = Wire::start().await;
    let templates = wire.templates.clone();
    write(
        &templates.join("good").join("template.yaml"),
        "label: \"Good\"\n",
    );
    write(
        &templates.join("broken").join("template.yaml"),
        "label: \"unterminated\n  - [\n",
    );
    for (relative, content) in [
        ("template.yaml", "label: \"Demo\"\n"),
        ("CLAUDE.md.tpl", "# {{NAME}}\n"),
        ("workflow.yml", "on: push\n"),
        ("dot-claude/settings.json", "{}"),
        ("notes/.gitkeep", ""),
    ] {
        write(&templates.join("demo").join(relative), content);
    }

    let listing = wire.listing().await;
    let ids: Vec<&str> = listing.templates.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids[0], "minimal");
    for id in ["example", "good", "demo"] {
        assert!(ids.contains(&id), "{ids:?}");
    }
    assert!(!ids.contains(&"broken"), "{ids:?}");
    assert_eq!(listing.problems.len(), 1, "{:?}", listing.problems);
    assert!(
        listing.problems[0].contains("broken"),
        "{:?}",
        listing.problems
    );
    assert_eq!(listing.dir, templates.display().to_string());

    let preview = wire.preview("demo").await;
    assert_eq!(
        preview.entries,
        [
            "workspace.yaml",
            ".claude/settings.json",
            "CLAUDE.md",
            "notes/",
            "workflow.yml"
        ]
    );
    assert!(preview.note.contains("Demo"), "{}", preview.note);
    assert!(preview.problems.is_empty(), "{:?}", preview.problems);
    let absent = wire.preview("absent").await;
    assert!(absent.entries.is_empty());
    assert_eq!(absent.problems, ["There is no template called \"absent\"."]);

    let detail = wire.detail("demo").await;
    assert_eq!(detail.label, "Demo");
    assert!(detail.has_manifest);
    assert_eq!(detail.file_count, 4);
    let mapped: Vec<String> = detail
        .files
        .iter()
        .map(|file| format!("{} -> {}", file.rel_path, file.target))
        .collect();
    assert_eq!(
        mapped,
        [
            "CLAUDE.md.tpl -> CLAUDE.md",
            "dot-claude/settings.json -> .claude/settings.json",
            "notes/.gitkeep -> notes/",
            "workflow.yml -> workflow.yml"
        ]
    );

    // A name that reaches outside the folder reads nothing there.
    write(
        &wire.dirs.config.join("outside").join("template.yaml"),
        "label: \"Out\"\n",
    );
    for template in ["../outside", "..\\outside"] {
        let preview = wire.preview(template).await;
        assert!(
            preview.entries.is_empty(),
            "{template}: {:?}",
            preview.entries
        );
        assert!(!preview.problems.is_empty(), "{template}");
        let detail = wire.detail(template).await;
        assert!(detail.files.is_empty(), "{template}: {:?}", detail.files);
        assert!(!detail.problems.is_empty(), "{template}");
    }

    wire.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_creates_describes_renames_fills_and_deletes_a_template_in_the_templates_folder() {
    let mut wire = Wire::start().await;
    let templates = wire.templates.clone();

    let created = wire.create("draft").await;
    assert_eq!(
        created,
        TemplateWriteResult {
            ok: true,
            template: Some("draft".into()),
            problems: Vec::new()
        }
    );
    assert_eq!(tree(&templates.join("draft")), ["template.yaml"]);

    let described = wire
        .ask::<WriteTemplateMetadata>(&TemplateMetadataRequest {
            template: "draft".into(),
            label: "Draft kit".into(),
            description: "For drafts.".into(),
        })
        .await;
    assert!(described.ok, "{:?}", described.problems);
    let listing = wire.listing().await;
    let draft = listing.templates.iter().find(|t| t.id == "draft").unwrap();
    assert_eq!(
        (draft.label.as_str(), draft.description.as_deref()),
        ("Draft kit", Some("For drafts."))
    );

    let renamed = wire
        .ask::<RenameTemplate>(&RenameTemplateRequest {
            template: "draft".into(),
            name: "final".into(),
        })
        .await;
    assert_eq!(
        renamed.template.as_deref(),
        Some("final"),
        "{:?}",
        renamed.problems
    );
    assert!(!templates.join("draft").exists());
    let ids = wire.listed_ids().await;
    assert!(ids.contains(&"final".to_owned()) && !ids.contains(&"draft".to_owned()));
    assert_eq!(wire.detail("final").await.label, "Draft kit");

    write(&templates.join("final").join("CLAUDE.md"), "# {{NAME}}\n");
    let filled = wire
        .ask::<SubstituteTemplateFile>(&TemplateFileRequest {
            template: "final".into(),
            path: "CLAUDE.md".into(),
        })
        .await;
    assert!(filled.ok, "{:?}", filled.problems);
    assert!(!templates.join("final").join("CLAUDE.md").exists());
    assert!(templates.join("final").join("CLAUDE.md.tpl").exists());
    let parent = wire.root.clone();
    let made = wire.harness(&parent, "made", "final").await;
    assert!(made.problems.is_empty(), "{:?}", made.problems);
    assert_eq!(read(&parent.join("made").join("CLAUDE.md")), "# made\n");

    assert!(wire.delete("final").await);
    assert!(!templates.join("final").exists());
    assert!(!wire.listed_ids().await.contains(&"final".to_owned()));

    wire.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_imports_a_skill_from_the_user_scope_as_its_whole_folder() {
    let mut wire = Wire::after(|dirs| {
        let skill = dirs.home.join(".claude").join("skills").join("think");
        write(
            &skill.join("SKILL.md"),
            "---\nname: think\ndescription: Think it through.\n---\nUse {{NAME}} as written.\n",
        );
        write(&skill.join("reference.md"), "# reference\n");
    })
    .await;
    let claude = wire.dirs.home.join(".claude");
    let skill = claude.join("skills").join("think");
    assert!(wire.create("kit").await.ok);

    let result = wire
        .ask::<ImportIntoTemplate>(&ImportIntoTemplateRequest {
            template: "kit".into(),
            scope_path: claude.display().to_string(),
            paths: vec![skill.join("SKILL.md").display().to_string()],
        })
        .await;

    assert!(result.problems.is_empty(), "{:?}", result.problems);
    let mut created = result.created.clone();
    created.sort();
    assert_eq!(
        created,
        [
            ".claude/skills/think/SKILL.md",
            ".claude/skills/think/reference.md"
        ]
    );
    let copied = wire.templates.join("kit/.claude/skills/think");
    assert_eq!(
        fs::read(copied.join("SKILL.md")).unwrap(),
        fs::read(skill.join("SKILL.md")).unwrap()
    );
    assert_eq!(read(&copied.join("reference.md")), "# reference\n");

    let gone = claude.join("skills").join("gone").join("SKILL.md");
    let refused = wire
        .ask::<ImportIntoTemplate>(&ImportIntoTemplateRequest {
            template: "kit".into(),
            scope_path: claude.display().to_string(),
            paths: vec![gone.display().to_string()],
        })
        .await;
    assert!(!refused.ok);
    assert_eq!(
        refused.problems,
        [format!(
            "{} is not in User any more. Re-read the scope and try again.",
            gone.display()
        )]
    );

    wire.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_previews_a_folder_as_a_template_and_saves_what_was_ticked() {
    let mut wire = Wire::start().await;
    let dir = plant_workspace_in(&wire.root);

    let preview = wire.folder_preview(&dir).await;
    let mut names: Vec<&str> = preview.entries.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            ".claude",
            ".git",
            "CLAUDE.md",
            "workspace.yaml",
            "notes",
            "repos",
            "tools"
        ]
    );
    let include = ticked(&preview);
    let mut sorted_include = include.clone();
    sorted_include.sort();
    assert_eq!(sorted_include, [".claude", "CLAUDE.md", "notes", "tools"]);
    assert_eq!(preview.file_count, 4);

    let saved = wire.save_folder(&dir, "frozen", include).await;

    assert!(saved.ok, "{:?}", saved.problems);
    assert_eq!(saved.file_count, 4);
    assert_eq!(
        tree(&wire.templates.join("frozen")),
        [
            ".claude",
            ".claude/skills",
            ".claude/skills/think",
            ".claude/skills/think/SKILL.md",
            "CLAUDE.md",
            "notes",
            "notes/journal.md",
            "template.yaml",
            "tools",
            "tools/run.mjs"
        ]
    );
    let listing = wire.listing().await;
    let frozen = listing.templates.iter().find(|t| t.id == "frozen").unwrap();
    assert_eq!(frozen.label, "Frozen");

    // A name already taken leaves that template alone.
    let again = wire
        .save_folder(&dir, "frozen", vec!["CLAUDE.md".into()])
        .await;
    assert!(!again.ok);
    assert_eq!(tree(&wire.templates.join("frozen")).len(), 10);

    wire.stop().await;
}
