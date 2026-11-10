//! Markdown and source rendered to sanitised, highlighted HTML.
//!
//! - [`markdown`]: a note to HTML. comrak parses GitHub-flavoured markdown,
//!   ammonia sanitises everything the note wrote, and only then are the
//!   frontmatter chips, heading anchors, wikilinks, tags, callouts and code
//!   highlighting applied, so nothing the host generates depends on the
//!   sanitiser letting it through.
//! - [`highlight`]: syntect, with each token carrying `--shiki-light` and
//!   `--shiki-dark` as today's shiki output does, so a client's stylesheet
//!   picks a side without re-highlighting.
//! - [`frontmatter`]: the YAML block at the top of a note.
//! - [`wikilinks`]: `[[wikilink]]` parsing and resolution.
//!
//! [`highlight::editor_highlight`] is `editor:highlight`, which the editors
//! share.

pub mod frontmatter;
pub mod highlight;
mod html;
pub mod markdown;
pub mod wikilinks;

pub use highlight::{HIGHLIGHT_MAX_BYTES, editor_highlight};
pub use markdown::render_markdown;
