//! What a session's agent may call it: the title `session_title` and
//! `floti session title` set, shown beside the session's name. One line of at
//! most [`TITLE_MAX`] characters, saying what is being done.
//!
//! A title that breaks these rules is refused with a sentence the agent can
//! act on, never cut or joined: a title cut to fit can stop mid-word and say
//! something else, and lines joined into one say what nobody wrote.

use crate::history::index::clean_name;

/// The longest title, in characters.
pub const TITLE_MAX: usize = 60;

/// `text` as the title it sets: trimmed, with control and format characters
/// made spaces and runs of spaces made one, as a name given by hand is.
///
/// # Errors
///
/// A sentence saying what is wrong with it: empty, more than one line, or
/// longer than [`TITLE_MAX`] characters.
pub fn session_title(text: &str) -> Result<String, String> {
    if text.trim().chars().any(is_line_break) {
        return Err(format!(
            "A title is one line. Say what this session is doing in at most {TITLE_MAX} characters, such as \"Agents page: edit and delete agents\"."
        ));
    }
    let title = clean_name(text);
    if title.is_empty() {
        return Err(
            "Give the title as text: what this session is doing, such as \"Agents page: edit and delete agents\"."
                .to_owned(),
        );
    }
    let length = title.chars().count();
    if length > TITLE_MAX {
        return Err(format!(
            "That title is {length} characters; a title is at most {TITLE_MAX}. Say it shorter."
        ));
    }
    Ok(title)
}

/// A character that ends a line.
fn is_line_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_is_kept_as_written_with_its_ends_trimmed_and_its_spaces_made_one() {
        assert_eq!(
            session_title("  Agents page:   edit and delete agents \t"),
            Ok("Agents page: edit and delete agents".to_owned())
        );
    }

    #[test]
    fn control_and_format_characters_become_spaces() {
        assert_eq!(
            session_title("Fix\u{200B}the\u{1b}login"),
            Ok("Fix the login".to_owned())
        );
    }

    #[test]
    fn a_title_of_more_than_one_line_is_refused() {
        for text in ["first\nsecond", "first\r\nsecond", "a\u{2028}b"] {
            let refused = session_title(text).unwrap_err();
            assert!(refused.contains("one line"), "{refused}");
        }
        // A line break at either end is trimmed, not a second line.
        assert_eq!(session_title("one line\n"), Ok("one line".to_owned()));
    }

    #[test]
    fn an_empty_title_is_refused() {
        for text in ["", "   ", "\u{200B}"] {
            assert!(session_title(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn sixty_characters_are_a_title_and_sixty_one_are_refused_saying_so() {
        let sixty = "é".repeat(TITLE_MAX);
        assert_eq!(session_title(&sixty), Ok(sixty.clone()));
        let refused = session_title(&format!("{sixty}x")).unwrap_err();
        assert!(refused.contains("61 characters"), "{refused}");
        assert!(refused.contains("at most 60"), "{refused}");
    }
}
