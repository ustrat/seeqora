//! Time in questions and in item text (W5 query rewrite).
//!
//! Two jobs, both rule-based and deterministic (T7):
//!
//! - [`parse_question`]: "last Thursday", "next week", "in November",
//!   "yesterday", "on Monday afternoon" become a time window, and "next" /
//!   "upcoming" / "latest" become an ordering. Search uses the window to rank
//!   items in it first, and the ordering to sort the most relevant items by
//!   date. It never hides other items (a wrong parse must not lose answers).
//! - [`event_days`]: the days an item is *about*. A calendar event is about
//!   its start; a message sent on May 15 saying "your visit on 06/05" is
//!   about June 5. This is what makes "next appointment" work for reminders.
//!
//! All dates are local days: days since 1970-01-01 in the user's time zone,
//! given as a UTC offset in minutes.

use crate::record::SourceKind;

pub const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    /// Best match first (no time word in the question).
    #[default]
    Relevance,
    /// "next", "upcoming", "coming up": soonest item on or after today first.
    Soonest,
    /// "last", "latest", "most recent": newest item first.
    Latest,
}

/// What a question says about time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TimeQuery {
    /// Item time window, UTC milliseconds, inclusive.
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    /// Source named alongside a window ("texted me yesterday").
    pub source: Option<SourceKind>,
    pub order: Order,
}

impl TimeQuery {
    pub fn has_window(&self) -> bool {
        self.from_ms.is_some() || self.to_ms.is_some()
    }
}

/// Local day number of a UTC timestamp.
pub fn local_day(ms: i64, utc_offset_minutes: i32) -> i64 {
    (ms + utc_offset_minutes as i64 * 60_000).div_euclid(DAY_MS)
}

/// UTC milliseconds at local midnight starting `day`.
pub fn day_start_ms(day: i64, utc_offset_minutes: i32) -> i64 {
    day * DAY_MS - utc_offset_minutes as i64 * 60_000
}

/// 0 = Monday … 6 = Sunday. 1970-01-01 was a Thursday.
fn weekday(day: i64) -> i64 {
    (day + 3).rem_euclid(7)
}

/// (year, month, day) to days since 1970-01-01, proleptic Gregorian.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Days since 1970-01-01 to (year, month, day).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
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

fn days_in_month(y: i64, m: u32) -> u32 {
    let next = if m == 12 { days_from_civil(y + 1, 1, 1) } else { days_from_civil(y, m + 1, 1) };
    (next - days_from_civil(y, m, 1)) as u32
}

const MONTHS: [&str; 12] = [
    "january", "february", "march", "april", "may", "june", "july", "august", "september",
    "october", "november", "december",
];
const WEEKDAYS: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

/// Month number for a full name or an abbreviation ("sep", "sept.", "nov").
fn month_of(word: &str) -> Option<u32> {
    let w = word.trim_end_matches('.');
    if w.len() < 3 {
        return None;
    }
    MONTHS
        .iter()
        .position(|m| *m == w || (w.len() <= 4 && m.starts_with(w)))
        .map(|i| i as u32 + 1)
}

/// Weekday (0 = Monday) for a name, plural or unambiguous abbreviation.
/// "sun", "sat" and "wed" are left out: they are ordinary words too.
fn weekday_of(word: &str) -> Option<i64> {
    let w = word.trim_end_matches('.');
    let short = match w {
        "mon" => Some(0),
        "tue" | "tues" => Some(1),
        "thu" | "thur" | "thurs" => Some(3),
        "fri" => Some(4),
        _ => None,
    };
    short.or_else(|| {
        WEEKDAYS
            .iter()
            .position(|d| *d == w || format!("{d}s") == w)
            .map(|i| i as i64)
    })
}

/// "5", "5th", "22nd,", "20-23" -> 5, 5, 22, 20 (1–31).
fn day_number(word: &str) -> Option<u32> {
    let w = word.trim_end_matches([',', '.', ';', ':']);
    let w = w.split('-').next().unwrap_or(w);
    let w = ["st", "nd", "rd", "th"].iter().fold(w, |w, s| w.strip_suffix(s).unwrap_or(w));
    (!w.is_empty() && w.len() <= 2 && w.chars().all(|c| c.is_ascii_digit()))
        .then(|| w.parse().ok())
        .flatten()
        .filter(|d| (1..=31).contains(d))
}

fn year_number(word: &str) -> Option<i64> {
    let w = word.trim_end_matches([',', '.', ';', ':']);
    (w.len() == 4 && w.chars().all(|c| c.is_ascii_digit()))
        .then(|| w.parse().ok())
        .flatten()
        .filter(|y| (1990..=2100).contains(y))
}

/// Lowercased words, keeping '/', '-' and ':' inside tokens for dates and times.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '"' | '!' | '?'))
        .map(|w| w.trim_matches(|c: char| matches!(c, ',' | ';' | '\'' | '’')).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// How to place a date written without a year.
#[derive(Clone, Copy)]
enum Resolve {
    /// The first such date on or after this day (reminders look forward).
    OnOrAfter(i64),
    /// The such date closest to this day (questions).
    Nearest(i64),
}

fn place(m: u32, d: u32, year: Option<i64>, how: Resolve) -> Option<i64> {
    let pick = |y: i64| (d <= days_in_month(y, m)).then(|| days_from_civil(y, m, d));
    if let Some(y) = year {
        return pick(y);
    }
    let around = match how {
        Resolve::OnOrAfter(day) | Resolve::Nearest(day) => day,
    };
    let (y0, _, _) = civil_from_days(around);
    let options: Vec<i64> = [y0 - 1, y0, y0 + 1].iter().filter_map(|&y| pick(y)).collect();
    match how {
        Resolve::OnOrAfter(day) => options.into_iter().filter(|&x| x >= day).min(),
        Resolve::Nearest(day) => options.into_iter().min_by_key(|&x| (x - day).abs()),
    }
}

/// Day-of-month only ("the 24th"): the first one on or after `from`.
fn place_day_of_month(d: u32, from: i64) -> Option<i64> {
    let (y, m, _) = civil_from_days(from);
    (0..3)
        .map(|k| {
            let mm = (m - 1 + k) % 12 + 1;
            let yy = y + ((m - 1 + k) / 12) as i64;
            (yy, mm)
        })
        .filter_map(|(yy, mm)| (d <= days_in_month(yy, mm)).then(|| days_from_civil(yy, mm, d)))
        .find(|&x| x >= from)
}

/// Dates written in `text`, as local days.
fn mentioned(text: &str, sent_day: i64, how: Resolve) -> Vec<i64> {
    let w = words(text);
    let mut out = Vec::new();
    for (i, t) in w.iter().enumerate() {
        let t = t.trim_end_matches(['.', ':']);
        // 2026-11-20
        let iso: Vec<&str> = t.split('-').collect();
        if iso.len() == 3 && iso[0].len() == 4 {
            if let (Ok(y), Ok(m), Ok(d)) = (iso[0].parse::<i64>(), iso[1].parse::<u32>(), iso[2].parse::<u32>()) {
                if (1..=12).contains(&m) && (1..=31).contains(&d) {
                    out.extend(place(m, d, Some(y), how));
                }
            }
            continue;
        }
        // 06/05, 6/5/25, 08/29/2024
        let parts: Vec<&str> = t.split('/').collect();
        if (2..=3).contains(&parts.len()) && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
            let m = parts[0].parse::<u32>().unwrap_or(0);
            let d = parts[1].parse::<u32>().unwrap_or(0);
            let y = parts.get(2).and_then(|y| y.parse::<i64>().ok()).map(|y| if y < 100 { 2000 + y } else { y });
            if (1..=12).contains(&m) && (1..=31).contains(&d) && parts[0].len() <= 2 && parts[1].len() <= 2 {
                out.extend(place(m, d, y, how));
            }
            continue;
        }
        // Nov 5, November 5th 2026, Thursday February 26 2026
        if let Some(m) = month_of(t) {
            // "may" and "march" are also ordinary words: only with a day after them.
            if let Some(d) = w.get(i + 1).and_then(|x| day_number(x)) {
                let y = w.get(i + 2).and_then(|x| year_number(x));
                out.extend(place(m, d, y, how));
            }
            continue;
        }
        // the 24th, Friday the 16th
        if t == "the" {
            if let Some(next) = w.get(i + 1) {
                let ordinal = ["st", "nd", "rd", "th"].iter().any(|s| next.trim_end_matches([',', '.']).ends_with(s));
                // "the 24th of October" is read at "october 24"-style tokens instead.
                let of_month = w.get(i + 2).is_some_and(|x| x == "of");
                if ordinal && !of_month {
                    if let Some(d) = day_number(next) {
                        out.extend(place_day_of_month(d, sent_day));
                    }
                }
            }
            continue;
        }
        match t {
            "today" | "tonight" => out.push(sent_day),
            "tomorrow" => out.push(sent_day + 1),
            _ => {}
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The local days an item is about: a calendar event's start, or the dates a
/// message, note or file mentions (placed on or after a date shortly before
/// it was written). Empty if it mentions none.
pub fn event_days(source: SourceKind, timestamp_ms: i64, text: &str, utc_offset_minutes: i32) -> Vec<i64> {
    let sent = local_day(timestamp_ms, utc_offset_minutes);
    match source {
        SourceKind::Calendar => vec![sent],
        SourceKind::Contacts => vec![],
        _ => mentioned(text, sent, Resolve::OnOrAfter(sent - 60)),
    }
}

/// Read the time words in a question asked at `now_ms`.
pub fn parse_question(question: &str, now_ms: i64, utc_offset_minutes: i32) -> TimeQuery {
    let off = utc_offset_minutes;
    let w = words(question);
    let today = local_day(now_ms, off);
    let has = |s: &str| w.iter().any(|x| x == s);
    let after = |s: &str| -> Option<&str> {
        w.iter().position(|x| x == s).and_then(|i| w.get(i + 1)).map(|x| x.as_str())
    };
    let span = |d0: i64, d1: i64| (Some(day_start_ms(d0, off)), Some(day_start_ms(d1 + 1, off) - 1));
    let monday = today - weekday(today);
    let (y, m, _) = civil_from_days(today);
    let month_span = |y: i64, m: u32| {
        let first = days_from_civil(y, m, 1);
        span(first, first + days_in_month(y, m) as i64 - 1)
    };
    let shift_month = |k: i64| {
        let idx = y * 12 + (m as i64 - 1) + k;
        (idx.div_euclid(12), (idx.rem_euclid(12) + 1) as u32)
    };
    // A bare weekday means the past one only when the question is in the past
    // tense ("What did Dana text me on Monday?"); otherwise the coming one
    // ("the deck due Friday").
    let past_tense = ["did", "was", "were", "went", "had", "sent", "texted", "happened", "got"]
        .iter()
        .any(|s| has(s));

    let mut window: Option<(Option<i64>, Option<i64>)> = None;
    if has("today") || has("tonight") {
        window = Some(span(today, today));
    } else if has("tomorrow") {
        window = Some(span(today + 1, today + 1));
    } else if has("yesterday") {
        window = Some(span(today - 1, today - 1));
    } else if after("last") == Some("week") || after("past") == Some("week") {
        window = Some(span(monday - 7, monday - 1));
    } else if after("this") == Some("week") {
        window = Some(span(monday, monday + 6));
    } else if after("next") == Some("week") {
        window = Some(span(monday + 7, monday + 13));
    } else if after("last") == Some("month") {
        let (yy, mm) = shift_month(-1);
        window = Some(month_span(yy, mm));
    } else if after("this") == Some("month") {
        let first = days_from_civil(y, m, 1);
        // "earlier this month" ends today; "this month" runs to its end.
        let end = if has("earlier") { today } else { first + days_in_month(y, m) as i64 - 1 };
        window = Some(span(first, end));
    } else if after("next") == Some("month") {
        let (yy, mm) = shift_month(1);
        window = Some(month_span(yy, mm));
    }

    // A weekday, optionally with last/next/this and a part of the day.
    if window.is_none() {
        if let Some((i, wd)) = w.iter().enumerate().find_map(|(i, x)| weekday_of(x).map(|d| (i, d))) {
            let prev = if i > 0 { w[i - 1].as_str() } else { "" };
            let back = (weekday(today) - wd).rem_euclid(7);
            let fwd = (wd - weekday(today)).rem_euclid(7);
            let day = match prev {
                "last" | "past" => today - if back == 0 { 7 } else { back },
                "next" => today + if fwd == 0 { 7 } else { fwd },
                "this" | "coming" => today + fwd,
                _ if past_tense => today - back,
                _ => today + fwd,
            };
            let (mut from, mut to) = span(day, day);
            let start = day_start_ms(day, off);
            if has("morning") {
                to = Some(start + DAY_MS / 2 - 1);
            } else if has("afternoon") {
                from = Some(start + DAY_MS / 2);
            } else if has("evening") || has("night") {
                from = Some(start + 17 * 3_600_000);
            }
            window = Some((from, to));
        }
    }

    // A date ("October 3rd", "10/3") or a month on its own ("in November").
    if window.is_none() {
        if let Some(&d) = mentioned(question, today, Resolve::Nearest(today)).first() {
            window = Some(span(d, d));
        } else if let Some((i, mm)) = w.iter().enumerate().find_map(|(i, x)| month_of(x).map(|m| (i, m))) {
            // Only "in November" / "during May": "bill Jordan for September"
            // is about September's work, not something that happened then.
            let after_in = i > 0 && matches!(w[i - 1].as_str(), "in" | "during");
            if after_in {
                let yy = [y - 1, y, y + 1]
                    .into_iter()
                    .min_by_key(|&yy| (days_from_civil(yy, mm, 15) - today).abs())
                    .unwrap_or(y);
                window = Some(month_span(yy, mm));
            }
        }
    }

    let mut tq = TimeQuery::default();
    if let Some((from, to)) = window {
        tq.from_ms = from;
        tq.to_ms = to;
        tq.source = if has("calendar") || has("schedule") {
            Some(SourceKind::Calendar)
        } else if ["text", "texted", "texts", "message", "messages", "messaged", "sms"].iter().any(|s| has(s)) {
            Some(SourceKind::Messages)
        } else if has("note") || has("notes") || ((has("wrote") || has("write")) && has("down")) {
            Some(SourceKind::Notes)
        } else {
            None
        };
        return tq;
    }

    // No window: "next"/"upcoming" or "last"/"latest" set the order.
    let unit = |s: &str| matches!(s, "year" | "years" | "time" | "one" | "few");
    let next_unit = after("next").is_some_and(unit);
    let last_unit = after("last").is_some_and(unit) || after("last") == Some("night");
    if (has("next") && !next_unit)
        || has("upcoming")
        || (has("coming") && after("coming") == Some("up"))
        || has("soon")
    {
        tq.order = Order::Soonest;
        // "What's next on my calendar?": the calendar in time order.
        if has("calendar") || has("schedule") {
            tq.source = Some(SourceKind::Calendar);
        }
    } else if (has("last") && !last_unit)
        || has("latest")
        || has("previous")
        || (has("recent") && (has("most") || has("the")))
    {
        tq.order = Order::Latest;
    }
    tq
}

#[cfg(test)]
mod tests {
    use super::*;

    // Golden-set "now": Thursday 2026-10-15 18:00 UTC.
    const NOW: i64 = 1_792_087_200_000;

    fn day(y: i64, m: u32, d: u32) -> i64 {
        days_from_civil(y, m, d)
    }

    fn window_days(q: &str) -> (i64, i64) {
        let t = parse_question(q, NOW, 0);
        (local_day(t.from_ms.unwrap(), 0), local_day(t.to_ms.unwrap(), 0))
    }

    #[test]
    fn calendar_math_round_trips() {
        assert_eq!(civil_from_days(day(2026, 10, 15)), (2026, 10, 15));
        assert_eq!(local_day(NOW, 0), day(2026, 10, 15));
        assert_eq!(weekday(day(2026, 10, 15)), 3); // Thursday
        assert_eq!(days_in_month(2028, 2), 29);
    }

    #[test]
    fn question_windows_match_the_golden_set() {
        assert_eq!(window_days("What did I do last Thursday?"), (day(2026, 10, 8), day(2026, 10, 8)));
        assert_eq!(window_days("What's on my calendar next week?"), (day(2026, 10, 19), day(2026, 10, 25)));
        assert_eq!(window_days("What did Dana text me on Monday?"), (day(2026, 10, 12), day(2026, 10, 12)));
        assert_eq!(window_days("What appointments do I have in November?"), (day(2026, 11, 1), day(2026, 11, 30)));
        assert_eq!(window_days("Who texted me yesterday?"), (day(2026, 10, 14), day(2026, 10, 14)));
        assert_eq!(window_days("What was my card charged for on Sunday?"), (day(2026, 10, 11), day(2026, 10, 11)));
        assert_eq!(window_days("What did I write down earlier this month about the apartment?"), (day(2026, 10, 1), day(2026, 10, 15)));
        assert_eq!(window_days("How much was the Uber on October 3rd?"), (day(2026, 10, 3), day(2026, 10, 3)));
        assert_eq!(window_days("What did Priya and I plan last week?"), (day(2026, 10, 5), day(2026, 10, 11)));
        assert_eq!(window_days("Where was Maya's soccer game last Sunday?"), (day(2026, 10, 11), day(2026, 10, 11)));
        // No past tense: the coming Friday.
        assert_eq!(window_days("Which template should I use for the deck due Friday?"), (day(2026, 10, 16), day(2026, 10, 16)));
        // "for September" names a subject, not a window.
        assert!(!parse_question("How much did I bill Jordan for September?", NOW, 0).has_window());
        let t = parse_question("What was booked on Tuesday afternoon?", NOW, 0);
        assert_eq!(t.from_ms, Some(day_start_ms(day(2026, 10, 13), 0) + DAY_MS / 2));
    }

    #[test]
    fn sources_named_with_a_window_are_kept() {
        assert_eq!(parse_question("Who texted me yesterday?", NOW, 0).source, Some(SourceKind::Messages));
        assert_eq!(parse_question("What's on my calendar next week?", NOW, 0).source, Some(SourceKind::Calendar));
        assert_eq!(parse_question("What's the wifi password Dana sent?", NOW, 0), TimeQuery::default());
    }

    #[test]
    fn next_and_last_set_the_order() {
        assert_eq!(parse_question("when is camir next appointment", NOW, 0).order, Order::Soonest);
        let cal = parse_question("What is coming up next on my calendar?", NOW, 0);
        assert_eq!((cal.order, cal.source), (Order::Soonest, Some(SourceKind::Calendar)));
        assert_eq!(parse_question("When did I last get the oil changed?", NOW, 0).order, Order::Latest);
        assert_eq!(parse_question("How much is my rent going to be next year?", NOW, 0).order, Order::Relevance);
        assert!(!parse_question("when is my next appointment", NOW, 0).has_window());
    }

    #[test]
    fn local_offset_moves_today() {
        // 18:00 UTC is already Friday in Tokyo (+9h).
        assert_eq!(local_day(NOW, 9 * 60), day(2026, 10, 16));
        let t = parse_question("what's on today", NOW, -7 * 60);
        assert_eq!(t.from_ms, Some(day_start_ms(day(2026, 10, 15), -7 * 60)));
    }

    #[test]
    fn messages_are_about_the_dates_they_mention() {
        let sent = |y, m, d| day_start_ms(day(y, m, d), 0) + 10 * 3_600_000;
        let ev = |text: &str, at: i64| event_days(SourceKind::Messages, at, text, 0);
        assert_eq!(
            ev("EMORY: Camir, your visit with Leleiko, Rebecca M, MD on 06/05 at 2:00 PM is confirmed.", sent(2025, 5, 15)),
            vec![day(2025, 6, 5)]
        );
        assert_eq!(ev("Reminder: your cleaning with Dr. Lin is on Thursday Oct 8 at 8:30 AM", sent(2026, 10, 2)), vec![day(2026, 10, 8)]);
        assert_eq!(ev("Don't forget Grandpa's birthday dinner on the 24th", sent(2026, 10, 4)), vec![day(2026, 10, 24)]);
        assert_eq!(ev("Game moved to Field 3, 9am kickoff tomorrow", sent(2026, 10, 10)), vec![day(2026, 10, 11)]);
        assert_eq!(ev("Camir has multiple appointments on 08/29/2024 at 8:30 AM", sent(2024, 8, 8)), vec![day(2024, 8, 29)]);
        assert_eq!(ev("the cabin is booked for Nov 20-23", sent(2026, 10, 12)), vec![day(2026, 11, 20)]);
        assert_eq!(ev("A late December reminder for Jan 3", sent(2026, 12, 20)), vec![day(2027, 1, 3)]);
        assert!(ev("You may want to march on", sent(2026, 10, 1)).is_empty());
        assert!(ev("Payment sent for INV-2026-014, thanks!", sent(2026, 10, 14)).is_empty());
        assert_eq!(event_days(SourceKind::Calendar, sent(2026, 11, 5), "Fillings", 0), vec![day(2026, 11, 5)]);
    }
}
