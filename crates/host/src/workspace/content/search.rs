//! Full-text search over a scope's content.
//!
//! A plain case-insensitive substring scan, deliberately: a filter box has to
//! match `geofenc` inside `geofencing`, which a tokenising index cannot, and
//! a scope's few megabytes of notes are crossed in milliseconds.
//!
//! The corpus is held in memory and kept while every file's path, size and
//! mtime are the ones it was read at. The first search after a scope opens
//! pays the disk read, every one after it does not, and the result says
//! which of the two it was.
//!
//! Offsets a client highlights with (`from`, `to`) count UTF-16 units, as the
//! client's strings do.

use std::time::Instant;

use floti_protocol::channels::workspace::{
    ContentFile, ContentFileKind, ContentSearchHit, ContentSearchLine, ContentSearchResult, Millis,
};

use super::tree::name_order;

/// How many files a result carries.
pub const MAX_HITS: usize = 200;

/// How many lines each hit shows.
const MAX_LINES_PER_FILE: usize = 4;

/// A file large enough to be a database dump is not prose: it is matched by
/// its name only.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// How much of a long line an excerpt shows, in UTF-16 units.
const WINDOW: usize = 160;

/// The kinds whose bytes are searched. Source, data and text are in: "where
/// did the agent define X" is a question about a workspace's scripts as much as
/// its notes. HTML is out, because a generated artifact is markup with its
/// stylesheet inside it, and binary is never text. Every file is matched on
/// its name whatever its kind, and the result carries this list so a client
/// can say what was searched.
pub const SEARCHED_BODY_KINDS: [ContentFileKind; 4] = [
    ContentFileKind::Markdown,
    ContentFileKind::Data,
    ContentFileKind::Text,
    ContentFileKind::Source,
];

struct CorpusEntry {
    file: ContentFile,
    text: String,
    /// `text` folded once, at build time, with every byte offset unchanged.
    lower: String,
}

/// Every file in a scope, with the text of the ones whose bodies are searched.
pub struct ContentCorpus {
    scope_key: String,
    entries: Vec<CorpusEntry>,
    bytes: u64,
    with_text: u32,
    signature: String,
}

/// `path`, size and mtime of every file, so a stale corpus is detectable
/// without reading anything.
fn signature_of(files: &[ContentFile]) -> String {
    files
        .iter()
        .map(|file| format!("{}\u{0}{}\u{0}{}", file.path, file.size, file.mtime_ms.0))
        .collect::<Vec<_>>()
        .join("\u{1}")
}

/// Folds case one character at a time, keeping a character whose folded form
/// would change its length, so an offset into the folded text is the same
/// offset into the original.
fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        let mut lower = ch.to_lowercase();
        match (lower.next(), lower.next()) {
            (Some(single), None) if single.len_utf8() == ch.len_utf8() => out.push(single),
            _ => out.push(ch),
        }
    }
    out
}

/// Builds the corpus for `files`. A file too large to be prose is carried
/// with an empty body, so it stays findable by name.
pub fn build_corpus(scope_key: &str, files: &[ContentFile]) -> ContentCorpus {
    let mut entries = Vec::with_capacity(files.len());
    let mut bytes = 0u64;
    let mut with_text = 0u32;
    for file in files {
        let mut text = String::new();
        if SEARCHED_BODY_KINDS.contains(&file.kind)
            && file.size <= MAX_FILE_BYTES
            && let Ok(raw) = std::fs::read(&file.path)
        {
            text = String::from_utf8_lossy(&raw).into_owned();
            bytes += text.encode_utf16().count() as u64;
            with_text += 1;
        }
        let lower = fold(&text);
        entries.push(CorpusEntry {
            file: file.clone(),
            text,
            lower,
        });
    }
    ContentCorpus {
        scope_key: scope_key.to_owned(),
        entries,
        bytes,
        with_text,
        signature: signature_of(files),
    }
}

/// Whether `corpus` was built from exactly `files`.
pub fn corpus_is_current(corpus: &ContentCorpus, scope_key: &str, files: &[ContentFile]) -> bool {
    corpus.scope_key == scope_key && corpus.signature == signature_of(files)
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// The line holding byte `offset`, trimmed to what a narrow column shows,
/// with the match's place inside it.
fn excerpt(text: &str, offset: usize, needle_units: usize) -> ContentSearchLine {
    let before = &text[..offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |at| at + 1);
    let end = text[start..].find('\n').map_or(text.len(), |at| start + at);
    let raw = &text[start..end];
    let raw = raw.strip_suffix('\r').unwrap_or(raw);
    let in_line = utf16_len(&text[start..offset]);

    // Centre the match when the line is longer than the column: cutting from
    // the left only would show a nested list item's indentation and none of
    // the words that matched.
    let units: Vec<u16> = raw.encode_utf16().collect();
    let mut from = 0usize;
    let mut shown = raw.to_owned();
    if units.len() > WINDOW {
        from = in_line.saturating_sub(WINDOW / 3);
        let to = (from + WINDOW).min(units.len());
        shown = String::from_utf16_lossy(&units[from.min(units.len())..to]);
    }
    let trimmed_lead = utf16_len(&shown) - utf16_len(shown.trim_start());
    let shown = shown.trim().to_owned();
    let at = in_line.saturating_sub(from + trimmed_lead);
    ContentSearchLine {
        line: u32::try_from(line).unwrap_or(u32::MAX),
        text: shown,
        from: u32::try_from(at).unwrap_or(u32::MAX),
        to: u32::try_from(at + needle_units).unwrap_or(u32::MAX),
    }
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Every file `query` matches, by name or by text, name matches first, then
/// the most matches, then by path; at most `limit` of them, with every match
/// counted.
pub fn search_corpus(
    corpus: &ContentCorpus,
    query: &str,
    cold: bool,
    limit: usize,
) -> ContentSearchResult {
    let started = Instant::now();
    let needle = fold(query.trim());
    let body_kinds = SEARCHED_BODY_KINDS.to_vec();

    if needle.is_empty() {
        return ContentSearchResult {
            query: query.to_owned(),
            hits: Vec::new(),
            files_searched: 0,
            body_kinds,
            files_with_text: 0,
            bytes_searched: 0,
            total_matches: 0,
            took_ms: Millis(0.0),
            cold,
            truncated: false,
        };
    }
    let needle_units = utf16_len(&needle);

    let mut hits: Vec<ContentSearchHit> = Vec::new();
    let mut total_matches = 0u32;
    for entry in &corpus.entries {
        // The name counts too: "find the note called journal-2026-08-10" is
        // the same gesture as "find the note that mentions the report".
        let name_match = fold(&entry.file.rel_path).contains(&needle)
            || fold(&entry.file.title).contains(&needle);

        let mut matches = 0usize;
        let mut lines: Vec<ContentSearchLine> = Vec::new();
        let mut at = 0usize;
        while let Some(found) = entry.lower[at..].find(&needle) {
            let offset = at + found;
            matches += 1;
            if lines.len() < MAX_LINES_PER_FILE {
                let line = excerpt(&entry.text, offset, needle_units);
                // One line can hold several matches; showing it twice is noise.
                if !lines.iter().any(|existing| existing.line == line.line) {
                    lines.push(line);
                }
            }
            at = offset + needle.len();
        }
        if matches == 0 && !name_match {
            continue;
        }

        total_matches = total_matches.saturating_add(count(matches));
        hits.push(ContentSearchHit {
            path: entry.file.path.clone(),
            rel_path: entry.file.rel_path.clone(),
            root: entry.file.root.clone(),
            title: entry.file.title.clone(),
            matches: count(matches),
            name_match,
            lines,
        });
    }

    hits.sort_by(|a, b| {
        b.name_match
            .cmp(&a.name_match)
            .then_with(|| b.matches.cmp(&a.matches))
            .then_with(|| name_order(&a.rel_path, &b.rel_path))
    });
    let truncated = hits.len() > limit;
    hits.truncate(limit);

    ContentSearchResult {
        query: query.to_owned(),
        hits,
        files_searched: count(corpus.entries.len()),
        body_kinds,
        files_with_text: corpus.with_text,
        bytes_searched: corpus.bytes,
        total_matches,
        took_ms: Millis(started.elapsed().as_secs_f64() * 1000.0),
        cold,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use floti_protocol::channels::workspace::ConfigScopeKind;

    use crate::testkit::TestDirs;
    use crate::workspace::content::testing::{fixture, put, tree_of};
    use crate::workspace::content::tree::{content_scope, read_content_tree};

    #[test]
    fn an_excerpt_counts_its_offsets_in_utf16_units_after_the_trimmed_indent() {
        let text = "first\n    caf\u{e9} \u{1f600} quokka here\n";
        let offset = text.find("quokka").unwrap();
        let line = excerpt(text, offset, 6);
        assert_eq!(line.line, 2);
        assert_eq!(line.text, "caf\u{e9} \u{1f600} quokka here");
        let from = line.from as usize;
        let units: Vec<u16> = line.text.encode_utf16().collect();
        assert_eq!(String::from_utf16_lossy(&units[from..from + 6]), "quokka");
    }

    #[test]
    fn folding_keeps_every_offset_where_it_was() {
        let text = "\u{130}stanbul QUOKKA";
        let folded = fold(text);
        assert_eq!(folded.len(), text.len());
        assert_eq!(folded.find("quokka"), text.find("QUOKKA"));
    }

    #[test]
    fn search_matches_inside_a_word_and_reports_the_line() {
        let dirs = TestDirs::new();
        let root = dirs.folder("my workspace");
        fixture(&root);
        let tree = tree_of(&root);
        let result = search_corpus(
            &build_corpus("scope", &tree.files),
            "geofenc",
            true,
            MAX_HITS,
        );
        assert_eq!(result.total_matches, 1);
        assert!(result.hits[0].lines[0].text.contains("geofencing"));
    }

    #[test]
    fn search_finds_an_html_file_by_name_without_reading_its_contents() {
        let dirs = TestDirs::new();
        let root = dirs.folder("my workspace");
        fixture(&root);
        put(
            &root.join("notes").join("dashboard.html"),
            "<style>notes { color: red }</style>",
        );
        let tree = tree_of(&root);
        let corpus = build_corpus("scope", &tree.files);
        let by_name = search_corpus(&corpus, "dashboard", true, MAX_HITS);
        let hit = by_name
            .hits
            .iter()
            .find(|hit| hit.rel_path == "notes/dashboard.html")
            .unwrap();
        assert!(hit.name_match);
        let by_body = search_corpus(&corpus, "color: red", true, MAX_HITS);
        assert!(
            !by_body
                .hits
                .iter()
                .any(|hit| hit.rel_path == "notes/dashboard.html")
        );
    }

    /// One file per kind, each saying "quokka" in its body, so a kind whose body
    /// is skipped shows as a count that comes up short.
    const BODIES: [(&str, &str); 6] = [
        (
            "notes/zebra.md",
            "# Zebra\n\nQuokka quokka.\nA QUOKKA again.\n",
        ),
        ("notes/data.json", "{ \"animal\": \"quokka\" }\n"),
        ("notes/plain.txt", "quokka, then quokka\n"),
        ("notes/tool.py", "print(\"quokka\")\n"),
        ("notes/page.html", "<p>quokka quokka quokka</p>\n"),
        ("notes/blob.png", "quokka in bytes"),
    ];
    const READ: [&str; 4] = [
        "notes/zebra.md",
        "notes/data.json",
        "notes/plain.txt",
        "notes/tool.py",
    ];

    fn search_fixture(root: &Path) -> crate::workspace::content::search::ContentCorpus {
        for (rel, body) in BODIES {
            put(&root.join(rel), body);
        }
        // Past the 4 MB ceiling and full of the word, so reading it would show.
        let huge = "quokka filler line\n".repeat((4 * 1024 * 1024 + 4096) / 19 + 1);
        put(&root.join("notes").join("huge.md"), &huge);
        let tree = read_content_tree(&content_scope(root, ConfigScopeKind::Project, None));
        let mut listed: Vec<&str> = tree
            .files
            .iter()
            .map(|file| file.rel_path.as_str())
            .collect();
        listed.sort_unstable();
        let mut expected: Vec<&str> = BODIES.iter().map(|(rel, _)| *rel).collect();
        expected.push("notes/huge.md");
        expected.sort_unstable();
        assert_eq!(listed, expected);
        build_corpus("scope", &tree.files)
    }

    fn occurrences(term: &str) -> u32 {
        let term = term.to_lowercase();
        READ.iter()
            .map(|rel| {
                let body = BODIES.iter().find(|(path, _)| path == rel).unwrap().1;
                u32::try_from(body.to_lowercase().matches(&term).count()).unwrap()
            })
            .sum()
    }

    #[test]
    fn search_reads_bodies_for_markdown_data_text_and_source_only_and_says_so() {
        let dirs = TestDirs::new();
        let corpus = search_fixture(&dirs.folder("search scope"));
        let result = search_corpus(&corpus, "quokka", true, MAX_HITS);
        let mut kinds = result.body_kinds.clone();
        kinds.sort_by_key(|kind| format!("{kind:?}"));
        assert_eq!(
            kinds,
            [
                ContentFileKind::Data,
                ContentFileKind::Markdown,
                ContentFileKind::Source,
                ContentFileKind::Text
            ]
        );
        assert_eq!(result.files_searched, 7);
        assert_eq!(result.files_with_text, 4);
        let mut matched: Vec<(&str, u32)> = result
            .hits
            .iter()
            .map(|hit| (hit.rel_path.as_str(), hit.matches))
            .collect();
        matched.sort_unstable();
        assert_eq!(
            matched,
            [
                ("notes/data.json", 1),
                ("notes/plain.txt", 2),
                ("notes/tool.py", 1),
                ("notes/zebra.md", 3)
            ]
        );
    }

    #[test]
    fn search_matches_a_file_over_4_mb_by_its_name_only() {
        let dirs = TestDirs::new();
        let corpus = search_fixture(&dirs.folder("search scope"));
        let result = search_corpus(&corpus, "huge", true, MAX_HITS);
        let hits: Vec<(&str, bool, u32)> = result
            .hits
            .iter()
            .map(|hit| (hit.rel_path.as_str(), hit.name_match, hit.matches))
            .collect();
        assert_eq!(hits, [("notes/huge.md", true, 0)]);
        assert_eq!(
            search_corpus(&corpus, "filler", true, MAX_HITS).total_matches,
            0
        );
    }

    #[test]
    fn search_totals_every_occurrence_for_terms_that_match_many_part_of_a_word_or_nothing() {
        let dirs = TestDirs::new();
        let corpus = search_fixture(&dirs.folder("search scope"));
        for term in ["quokka", "QUOKKA", "okk", "quokka.", "wombat"] {
            let result = search_corpus(&corpus, term, false, MAX_HITS);
            assert_eq!(result.total_matches, occurrences(term), "{term}");
        }
        assert_eq!(occurrences("wombat"), 0);
        assert!(
            search_corpus(&corpus, "wombat", false, MAX_HITS)
                .hits
                .is_empty()
        );
    }

    #[test]
    fn search_caps_the_hit_list_at_200_files_and_keeps_counting_past_it() {
        let dirs = TestDirs::new();
        let root = dirs.folder("search cap");
        for i in 0..205 {
            put(
                &root.join("notes").join(format!("n{i:03}.md")),
                "needle and needle\n",
            );
        }
        let tree = read_content_tree(&content_scope(&root, ConfigScopeKind::Project, None));
        assert_eq!(tree.files.len(), 205);
        let result = search_corpus(
            &build_corpus("scope", &tree.files),
            "needle",
            true,
            MAX_HITS,
        );
        assert_eq!(result.hits.len(), 200);
        assert!(result.truncated);
        assert_eq!(result.total_matches, 205 * 2);
    }
}
