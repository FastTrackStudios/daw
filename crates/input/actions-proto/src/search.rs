//! Scored search for command palette functionality.
//!
//! Zero dependencies, works on WASM. Sufficient for hundreds of actions.
//!
//! ## Scoring Rules
//!
//! - Exact name prefix: 100
//! - Word boundary match in name: 60
//! - Substring match in name: 40
//! - Match in action ID: 20
//! - Match in description: 10

use crate::when::ActionContext;
use crate::{ActionCategory, ActionDefinition};

/// A scored search result with match metadata for highlighting.
pub struct SearchResult<'a> {
    pub definition: &'a ActionDefinition,
    pub score: u32,
    /// Character indices in the name that matched the query (for highlight rendering).
    pub name_matches: Vec<usize>,
}

/// Search actions by query string, returning scored results sorted by relevance.
///
/// If `ctx` is provided, only actions whose when-clause is satisfied are included.
pub fn search_actions<'a>(
    actions: &'a [ActionDefinition],
    query: &str,
    ctx: Option<&ActionContext>,
) -> Vec<SearchResult<'a>> {
    let query_lower = query.to_lowercase();
    if query_lower.is_empty() {
        // Empty query: return all (filtered by context), score 0
        return actions
            .iter()
            .filter(|a| ctx.is_none_or(|c| a.is_active(c)))
            .map(|a| SearchResult {
                definition: a,
                score: 0,
                name_matches: vec![],
            })
            .collect();
    }

    let mut results: Vec<SearchResult<'a>> = actions
        .iter()
        .filter(|a| ctx.is_none_or(|c| a.is_active(c)))
        .filter_map(|action| {
            let (score, name_matches) = score_action(action, &query_lower);
            if score > 0 {
                Some(SearchResult {
                    definition: action,
                    score,
                    name_matches,
                })
            } else {
                None
            }
        })
        .collect();

    results.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.definition.name.cmp(&b.definition.name))
    });
    results
}

/// Filter actions by category.
pub fn filter_by_category(
    actions: &[ActionDefinition],
    category: ActionCategory,
) -> Vec<&ActionDefinition> {
    actions.iter().filter(|a| a.category == category).collect()
}

fn score_action(action: &ActionDefinition, query_lower: &str) -> (u32, Vec<usize>) {
    let mut best_score = 0u32;
    let mut best_matches = vec![];

    // Check name
    let name_lower = action.name.to_lowercase();
    if let Some((score, matches)) = score_field(&name_lower, query_lower, true)
        && score > best_score
    {
        best_score = score;
        best_matches = matches;
    }

    // Check action ID
    let id_lower = action.id.as_str().to_lowercase();
    if let Some((score, _)) = score_field(&id_lower, query_lower, false) {
        let id_score = (score as f32 * 0.2) as u32; // ID matches score lower
        if id_score > best_score {
            best_score = id_score;
            best_matches = vec![]; // Don't highlight name for ID matches
        }
    }

    // Check description
    let desc_lower = action.description.to_lowercase();
    if let Some((score, _)) = score_field(&desc_lower, query_lower, false) {
        let desc_score = (score as f32 * 0.1) as u32; // Description matches score lowest
        if desc_score > best_score {
            best_score = desc_score;
            best_matches = vec![]; // Don't highlight name for description matches
        }
    }

    (best_score, best_matches)
}

fn score_field(field: &str, query: &str, is_name: bool) -> Option<(u32, Vec<usize>)> {
    // Exact prefix match (highest score for names)
    if field.starts_with(query) {
        let matches: Vec<usize> = (0..query.len()).collect();
        let score = if is_name { 100 } else { 80 };
        return Some((score, matches));
    }

    // Word boundary match: query matches the start of any word
    let word_starts: Vec<usize> = std::iter::once(0)
        .chain(
            field
                .char_indices()
                .filter(|(_, c)| *c == ' ' || *c == '_' || *c == '-' || *c == '.' || *c == '/')
                .map(|(i, _)| i + 1),
        )
        .collect();

    for &start in &word_starts {
        if start < field.len() && field[start..].starts_with(query) {
            let matches: Vec<usize> = (start..start + query.len()).collect();
            let score = if is_name { 60 } else { 50 };
            return Some((score, matches));
        }
    }

    // Substring match
    if let Some(pos) = field.find(query) {
        let matches: Vec<usize> = (pos..pos + query.len()).collect();
        let score = if is_name { 40 } else { 30 };
        return Some((score, matches));
    }

    // Fuzzy match: all query chars appear in order
    if let Some(matches) = fuzzy_match(field, query) {
        let score = if is_name { 20 } else { 10 };
        return Some((score, matches));
    }

    None
}

/// Simple fuzzy matching: each character of query appears in field in order.
fn fuzzy_match(field: &str, query: &str) -> Option<Vec<usize>> {
    let field_chars: Vec<char> = field.chars().collect();
    let query_chars: Vec<char> = query.chars().collect();
    let mut matches = Vec::with_capacity(query_chars.len());
    let mut fi = 0;

    for &qc in &query_chars {
        let mut found = false;
        while fi < field_chars.len() {
            if field_chars[fi] == qc {
                matches.push(fi);
                fi += 1;
                found = true;
                break;
            }
            fi += 1;
        }
        if !found {
            return None;
        }
    }

    Some(matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionCategory, ActionDefinition};

    fn test_actions() -> Vec<ActionDefinition> {
        vec![
            ActionDefinition::new(
                "fts.session.toggle_playback",
                "Toggle Playback",
                "Toggle play/pause",
            )
            .with_category(ActionCategory::Transport),
            ActionDefinition::new("fts.session.next_song", "Next Song", "Go to next song")
                .with_category(ActionCategory::Session),
            ActionDefinition::new(
                "fts.standalone.open_settings",
                "Open Settings",
                "Opens settings",
            )
            .with_category(ActionCategory::Settings),
            ActionDefinition::new(
                "fts.standalone.toggle_dark_mode",
                "Toggle Dark Mode",
                "Toggle dark/light",
            )
            .with_category(ActionCategory::View),
        ]
    }

    #[test]
    fn search_empty_returns_all() {
        let actions = test_actions();
        let results = search_actions(&actions, "", None);
        assert_eq!(results.len(), 4);
    }

    #[test]
    fn search_prefix_match_highest() {
        let actions = test_actions();
        let results = search_actions(&actions, "toggle", None);
        assert!(results.len() >= 2);
        // "Toggle Playback" and "Toggle Dark Mode" should rank highest
        assert!(results[0].score >= 100);
    }

    #[test]
    fn search_substring_match() {
        let actions = test_actions();
        let results = search_actions(&actions, "play", None);
        assert!(!results.is_empty());
        assert_eq!(
            results[0].definition.id.as_str(),
            "fts.session.toggle_playback"
        );
    }

    #[test]
    fn search_no_match() {
        let actions = test_actions();
        let results = search_actions(&actions, "zzzzz", None);
        assert!(results.is_empty());
    }

    #[test]
    fn search_id_match() {
        let actions = test_actions();
        let results = search_actions(&actions, "fts.session", None);
        assert!(results.len() >= 2);
    }

    #[test]
    fn filter_by_category_works() {
        let actions = test_actions();
        let transport = filter_by_category(&actions, ActionCategory::Transport);
        assert_eq!(transport.len(), 1);
        assert_eq!(transport[0].id.as_str(), "fts.session.toggle_playback");
    }

    #[test]
    fn search_with_context_filters() {
        let actions = vec![
            ActionDefinition::new("a", "Always Active", "desc"),
            ActionDefinition::new("b", "Performance Only", "desc").with_when("tab:performance"),
        ];

        let mut ctx = ActionContext::new();
        ctx.set_tab("settings");

        let results = search_actions(&actions, "", Some(&ctx));
        // Only "Always Active" should appear — "Performance Only" when-clause fails
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].definition.name, "Always Active");
    }

    #[test]
    fn fuzzy_match_works() {
        let matches = fuzzy_match("toggle playback", "tp");
        assert!(matches.is_some());
        let m = matches.unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0], 0); // 't' at index 0
        assert_eq!(m[1], 7); // 'p' at index 7
    }
}
