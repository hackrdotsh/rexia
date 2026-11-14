//! Fuzzy matching for palettes: the query's characters must appear in order; matches at word
//! starts and in runs rank higher.

/// A score (higher is better) and the matched character indices, or None.
pub fn score(query: &str, candidate: &str) -> Option<(i64, Vec<usize>)> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let chars: Vec<char> = candidate.chars().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let mut positions = Vec::with_capacity(query.len());
    let mut score = 0i64;
    let mut at = 0;
    let mut previous: Option<usize> = None;
    for wanted in &query {
        // Prefer an occurrence at a word start, as initials ("spr" for Split Pane Right).
        let rest = at..lower.len();
        let word_start = rest
            .clone()
            .find(|&i| lower[i] == *wanted && (i == 0 || !chars[i - 1].is_alphanumeric()));
        let index = match word_start {
            Some(start) => start,
            None => rest.clone().find(|&i| lower[i] == *wanted)?,
        };
        score += 1;
        if index == 0 || !chars[index - 1].is_alphanumeric() {
            score += 8;
        }
        if previous.is_some_and(|p| index == p + 1) {
            score += 5;
        }
        positions.push(index);
        previous = Some(index);
        at = index + 1;
    }
    // Shorter candidates and earlier matches win ties.
    score -= (chars.len() as i64) / 8 + positions[0] as i64 / 4;
    Some((score, positions))
}

/// Indices of `items` that match, best first; items keep their order when the query is empty.
pub fn rank<'a>(query: &str, items: impl Iterator<Item = &'a str>) -> Vec<usize> {
    let mut scored: Vec<(i64, usize)> = items
        .enumerate()
        .filter_map(|(i, text)| score(query, text).map(|(s, _)| (s, i)))
        .collect();
    if !query.trim().is_empty() {
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    scored.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequences_match_and_word_starts_rank_first() {
        assert!(score("spr", "Split Pane Right").is_some());
        assert!(score("xyz", "Split Pane Right").is_none());
        let (_, positions) = score("spr", "Split Pane Right").unwrap();
        assert_eq!(positions, vec![0, 6, 11]);
        let ranked = rank("nt", ["Send Key", "New Tab", "Toggle Sidebar"].into_iter());
        assert_eq!(ranked[0], 1);
        assert_eq!(rank("", ["b", "a"].into_iter()), vec![0, 1]);
    }

    #[test]
    fn runs_beat_scattered_letters() {
        let run = score("zoom", "Zoom Pane").unwrap().0;
        let scattered = score("zoom", "Z o o m").unwrap().0;
        assert!(run > scattered - 30);
        let ranked = rank(
            "close",
            ["Close Other Tabs", "Close", "Toggle Close"].into_iter(),
        );
        assert_eq!(ranked[0], 1);
    }
}
