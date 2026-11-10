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
    /// Local day the item is about (see `when::event_days`): the soonest
    /// upcoming one, else the latest mentioned. Lets the prompt mark it
    /// past, today or upcoming.
    pub about_day: Option<i64>,
    /// A calendar event: its own date is the day it is about.
    pub is_event: bool,
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
- Each source says whether what it is about is past, today or upcoming. \"Next\" or \"upcoming\" means today or later, never past. \
If the question asks for the next one and no source is today or upcoming, say there is none coming up in the sources, then give the most recent past one with its date.\n\
- If the sources do not answer the question, reply exactly: {NOT_FOUND}\n\
\n\
Example of the format:\n\
Sources:\n\
[1] Text message from Sam (sent Tue 2025-03-04; about Fri 2025-03-07, upcoming): Booked us a table at Lucia for 7:30 Friday.\n\
[2] Calendar event \"Gym\" (Wed 2025-03-05, upcoming): Gym, Wed 6 pm.\n\
Question: where is dinner on Friday?\n\
Answer: Dinner is at Lucia at 7:30 on Friday [1].",
        today = day_label(now_ms + offset_ms),
    );
    let today = (now_ms + offset_ms).div_euclid(86_400_000);
    let status = |day: i64| match day.cmp(&today) {
        std::cmp::Ordering::Less => "past",
        std::cmp::Ordering::Equal => "today",
        std::cmp::Ordering::Greater => "upcoming",
    };
    let mut user = String::from("Sources:\n");
    for (i, p) in passages.iter().enumerate() {
        let text: String = p.text.trim().chars().take(PASSAGE_CHARS).collect();
        let text = text.replace('\n', " ");
        let sent_day = (p.timestamp_ms + offset_ms).div_euclid(86_400_000);
        let when = match (p.is_event, p.about_day) {
            (true, _) => format!("{}, {}", day_label(p.timestamp_ms + offset_ms), status(sent_day)),
            (false, Some(d)) => format!(
                "sent {}; about {}, {}",
                day_label(p.timestamp_ms + offset_ms),
                day_label(d * 86_400_000),
                status(d)
            ),
            (false, None) => day_label(p.timestamp_ms + offset_ms),
        };
        user.push_str(&format!("[{}] {} ({}): {}\n", i + 1, p.label, when, text));
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

/// Words too common to show that an answer came from a passage.
const STOP_WORDS: &[&str] = &[
    "the", "an", "is", "are", "was", "were", "be", "been", "on", "at", "in", "of", "to", "and",
    "or", "for", "your", "you", "my", "me", "it", "its", "this", "that", "with", "from", "by",
    "has", "have", "had", "will", "next", "there", "their", "they", "he", "she", "his", "her",
    "st", "nd", "rd", "th", "am", "pm", "not", "no", "yes", "as", "so", "do", "did",
];

/// Lowercase words and numbers, split where letters meet digits ("26th" ->
/// "26", "3pm" -> "3"), without stop words.
fn content_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_digit = false;
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.is_empty() {
            let digit = cur.chars().all(|c| c.is_ascii_digit());
            if (digit || cur.chars().count() >= 2) && !STOP_WORDS.contains(&cur.as_str()) {
                out.push(std::mem::take(cur));
            }
            cur.clear();
        }
    };
    for c in text.chars() {
        if c.is_alphanumeric() {
            let d = c.is_ascii_digit();
            if !cur.is_empty() && d != cur_digit {
                flush(&mut cur, &mut out);
            }
            cur_digit = d;
            cur.extend(c.to_lowercase());
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// When the model answered without a citation, find the one passage the
/// answer clearly came from: at least half of the answer's content words and
/// numbers, and at least two of them, appear in it. Returns its number (1-based).
/// The shell labels such a citation as matched by Seeqora, not by the model.
pub fn attribute(answer: &str, passages: &[&str]) -> Option<u32> {
    let mut words = content_tokens(answer);
    words.sort();
    words.dedup();
    if words.len() < 2 {
        return None;
    }
    let mut best: Option<(usize, usize)> = None; // (shared, passage index)
    for (i, p) in passages.iter().enumerate() {
        let have: std::collections::HashSet<String> = content_tokens(p).into_iter().collect();
        let shared = words.iter().filter(|w| have.contains(*w)).count();
        if best.is_none_or(|(s, _)| shared > s) {
            best = Some((shared, i));
        }
    }
    let (shared, i) = best?;
    (shared >= 2 && shared * 2 >= words.len()).then_some(i as u32 + 1)
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

/// The reasoning block a Qwen3-family model may still write; never shown.
const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";

/// The part of `text` that is the answer: everything after a leading
/// `<think>…</think>` block, if there is one. `None` while that block is
/// still open, or while the text so far could still become `<think>`.
pub fn visible(text: &str) -> Option<&str> {
    let t = text.trim_start();
    if t.starts_with(THINK_OPEN) {
        return t.find(THINK_CLOSE).map(|i| t[i + THINK_CLOSE.len()..].trim_start());
    }
    let undecided = !t.is_empty() && THINK_OPEN.starts_with(t);
    (!undecided).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_blocks_are_hidden() {
        assert_eq!(visible("Dinner is at 7 [1]."), Some("Dinner is at 7 [1]."));
        assert_eq!(visible("<thi"), None);
        assert_eq!(visible("<think>\nlet me see"), None);
        assert_eq!(visible("<think>\nhmm\n</think>\n\nDinner [1]."), Some("Dinner [1]."));
        assert_eq!(visible("<b>bold</b>"), Some("<b>bold</b>"));
        assert_eq!(visible(""), Some(""));
    }

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
        let p = |label: &str, text: String| PromptPassage {
            label: label.into(),
            timestamp_ms: 0,
            text,
            about_day: None,
            is_event: false,
        };
        let ps = vec![p("Message from Dana", "wifi is Tamarack".into()), p("Note", long)];
        let (system, user) = build_prompt("what is the wifi?", &ps, 0, 0);
        assert!(system.contains(NOT_FOUND));
        assert!(system.contains("Today is Thu 1970-01-01"));
        assert!(user.contains("[1] Message from Dana (Thu 1970-01-01): wifi is Tamarack"));
        assert!(user.contains(&format!("[2] Note (Thu 1970-01-01): {}\n", "x".repeat(PASSAGE_CHARS))));
        assert!(user.ends_with("Question: what is the wifi?"));
    }

    #[test]
    fn prompt_marks_what_is_past_and_upcoming() {
        let day = 86_400_000_i64;
        let now = 10 * day + 3_600_000; // Sun 1970-01-11, 01:00 UTC
        let ps = vec![
            PromptPassage {
                label: "Text message from Clinic".into(),
                timestamp_ms: 2 * day,
                text: "Your visit on 01/05 is confirmed".into(),
                about_day: Some(4),
                is_event: false,
            },
            PromptPassage {
                label: "Calendar event \"Dentist\"".into(),
                timestamp_ms: 12 * day,
                text: "Dentist".into(),
                about_day: Some(12),
                is_event: true,
            },
        ];
        let (_, user) = build_prompt("when is my next appointment?", &ps, now, 0);
        assert!(user.contains("[1] Text message from Clinic (sent Sat 1970-01-03; about Mon 1970-01-05, past):"));
        assert!(user.contains("[2] Calendar event \"Dentist\" (Tue 1970-01-13, upcoming):"));
    }

    #[test]
    fn attribution_finds_the_clear_source_only() {
        let ps = [
            "When is your next break sir?",
            "Reminder: your appointment with Dr. Lee is Thursday, February 26th at 3pm.",
        ];
        assert_eq!(
            attribute("Camir's next appointment is on Thursday, February 26th at 3", &ps),
            Some(2)
        );
        // One shared word is not enough; neither is a minority of the answer.
        assert_eq!(attribute("It is blue.", &ps), None);
        assert_eq!(attribute("The meeting is with Dr. Patel in Boston on Monday", &ps), None);
    }

    #[test]
    fn tokens_split_numbers_from_letters() {
        assert_eq!(content_tokens("Feb 26th at 3pm, Camir's"), vec!["feb", "26", "3", "camir"]);
    }

    #[test]
    fn not_found_is_recognised() {
        assert!(says_not_found(NOT_FOUND));
        assert!(!says_not_found("The code is 1234 [1]."));
    }
}
