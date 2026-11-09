//! Cited answers from an on-device language model (T1, T2, T8).
//!
//! The core assembles the prompt from retrieved passages, runs the model and
//! checks the citations, so both shells behave the same (T6). Any model sits
//! behind [`Generator`]; v1 ships one implementation, [`crate::qwen`]. The user
//! imports the model file; the app never downloads it (T1).
//!
//! Citation rule (T2): passages are numbered `[1]..[n]` in the prompt. Numbers
//! the model writes that do not refer to a passage are removed. An answer with
//! no valid citation is reported as not found, and the shell shows the ranked
//! passages instead.

use crate::error::Result;

/// Passages given to the model. Six short passages keep the prompt near 1,200
/// tokens, which bounds prefill time on a phone CPU (T5).
pub const ANSWER_PASSAGES: usize = 6;
/// Per-passage cap, in characters (~175 tokens).
pub const PASSAGE_CHARS: usize = 700;
pub const MAX_NEW_TOKENS: usize = 200;
/// What the model is told to say when the passages do not answer the question.
pub const NOT_FOUND: &str = "I couldn't find that in your data.";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GenStats {
    pub prompt_tokens: usize,
    pub generated_tokens: usize,
    pub prefill_ms: u64,
    pub decode_ms: u64,
    /// The shell asked to stop before the model finished.
    pub stopped: bool,
}

/// An on-device chat model (T8). Implementations must not touch the network.
pub trait Generator: Send {
    fn model_id(&self) -> &str;

    /// Run one system + user turn. `on_text` receives text as it is decoded and
    /// returns `false` to stop early. Returns the full answer text.
    fn generate(
        &mut self,
        system: &str,
        user: &str,
        max_new_tokens: usize,
        on_text: &mut dyn FnMut(&str) -> bool,
    ) -> Result<(String, GenStats)>;
}

/// One numbered source in the prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct PromptPassage {
    /// Who or what it came from, e.g. "Message from Dana".
    pub label: String,
    pub timestamp_ms: i64,
    pub text: String,
}

/// System and user messages for a question over numbered passages.
/// `utc_offset_minutes` puts dates in the user's local time.
pub fn build_prompt(
    question: &str,
    passages: &[PromptPassage],
    now_ms: i64,
    utc_offset_minutes: i32,
) -> (String, String) {
    let offset_ms = utc_offset_minutes as i64 * 60_000;
    let system = format!(
        "You answer questions using only the numbered sources, which come from the user's own phone. \
Today is {today}.\n\
Rules:\n\
- Use only facts stated in the sources. Never guess or add outside knowledge.\n\
- After each fact, cite its source number in square brackets, like [2].\n\
- Answer in one to three short sentences.\n\
- If the sources do not answer the question, reply exactly: {NOT_FOUND}",
        today = day_label(now_ms + offset_ms),
    );
    let mut user = String::from("Sources:\n");
    for (i, p) in passages.iter().enumerate() {
        let text: String = p.text.trim().chars().take(PASSAGE_CHARS).collect();
        let text = text.replace('\n', " ");
        user.push_str(&format!(
            "[{}] {} ({}): {}\n",
            i + 1,
            p.label,
            day_label(p.timestamp_ms + offset_ms),
            text
        ));
    }
    user.push_str(&format!("\nQuestion: {}", question.trim()));
    (system, user)
}

/// The answer with invalid citations removed, and the valid passage numbers
/// in order of first use.
pub fn check_citations(text: &str, passages: usize) -> (String, Vec<u32>) {
    let mut out = String::with_capacity(text.len());
    let mut cited: Vec<u32> = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after.find(']');
        let inner = close.map(|c| &after[..c]);
        let is_ref = inner.is_some_and(|s| {
            !s.trim().is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == ',' || c == ' ')
        });
        if !is_ref {
            // Not a citation: keep the bracket as written.
            out.push('[');
            rest = after;
            continue;
        }
        let inner = inner.unwrap_or_default();
        let valid: Vec<u32> = inner
            .split(',')
            .filter_map(|n| n.trim().parse::<u32>().ok())
            .filter(|&n| n >= 1 && n as usize <= passages)
            .collect();
        if valid.is_empty() {
            // Cites a passage that does not exist: drop it, and the space before it.
            if out.ends_with(' ') {
                out.pop();
            }
        } else {
            for &n in &valid {
                if !cited.contains(&n) {
                    cited.push(n);
                }
            }
            let list: Vec<String> = valid.iter().map(|n| n.to_string()).collect();
            out.push_str(&format!("[{}]", list.join(", ")));
        }
        rest = &after[close.unwrap_or(0) + 1..];
    }
    out.push_str(rest);
    (out.trim().to_string(), cited)
}

/// Whether the model said the passages do not answer the question.
pub fn says_not_found(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("couldn't find that") || t.contains("could not find that") || t.contains("couldn’t find that")
}

/// "Mon 2026-11-09" for a timestamp already shifted to local time.
pub fn day_label(local_ms: i64) -> String {
    let days = local_ms.div_euclid(86_400_000);
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let wd = WEEKDAYS[(days + 4).rem_euclid(7) as usize]; // 1970-01-01 was a Thursday
    let (y, m, d) = civil_from_days(days);
    format!("{wd} {y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_local_and_named() {
        assert_eq!(day_label(0), "Thu 1970-01-01");
        // 2026-11-09 12:00 UTC is a Monday.
        assert_eq!(day_label(1_794_225_600_000), "Mon 2026-11-09");
        assert_eq!(day_label(-1), "Wed 1969-12-31");
    }

    #[test]
    fn invalid_citations_are_removed() {
        let (t, c) = check_citations("The password is Tamarack [2] [9]. Dinner is at 8 [1, 7].", 3);
        assert_eq!(t, "The password is Tamarack [2]. Dinner is at 8 [1].");
        assert_eq!(c, vec![2, 1]);
    }

    #[test]
    fn adjacent_and_repeated_citations() {
        let (t, c) = check_citations("Yes [1][3], and again [1].", 3);
        assert_eq!(t, "Yes [1][3], and again [1].");
        assert_eq!(c, vec![1, 3]);
    }

    #[test]
    fn non_citation_brackets_are_kept() {
        let (t, c) = check_citations("See [the note] and [ ] [", 2);
        assert_eq!(t, "See [the note] and [ ] [");
        assert!(c.is_empty());
    }

    #[test]
    fn prompt_numbers_passages_and_caps_them() {
        let long = "x".repeat(PASSAGE_CHARS + 50);
        let ps = vec![
            PromptPassage { label: "Message from Dana".into(), timestamp_ms: 0, text: "wifi is Tamarack".into() },
            PromptPassage { label: "Note".into(), timestamp_ms: 0, text: long },
        ];
        let (system, user) = build_prompt("what is the wifi?", &ps, 0, 0);
        assert!(system.contains(NOT_FOUND));
        assert!(system.contains("Today is Thu 1970-01-01"));
        assert!(user.contains("[1] Message from Dana (Thu 1970-01-01): wifi is Tamarack"));
        assert!(user.contains(&format!("[2] Note (Thu 1970-01-01): {}\n", "x".repeat(PASSAGE_CHARS))));
        assert!(user.ends_with("Question: what is the wifi?"));
    }

    #[test]
    fn not_found_is_recognised() {
        assert!(says_not_found(NOT_FOUND));
        assert!(!says_not_found("The code is 1234 [1]."));
    }
}
