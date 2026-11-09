# Golden query set

Tenet 10 says to measure retrieval before polishing chat. This set gates every phase.

## What it is

The fixtures are a synthetic phone for one person (Alex Rivera, Portland) holding messages, notes, files, contacts, calendar events and photo/OCR text. The questions file holds questions with known correct source items. All questions use `now = 2026-10-15T18:00Z`, a Thursday.

| Kind | Example | In v0 |
|---|---|---|
| `lookup` | "What's the wifi password Dana sent?" | 26 |
| `temporal` | "What did I do last Thursday?" | 11 |
| `multi_source` | "The PDF from the person I met at the dentist" | 8 |
| `negative` | the answer is not on the phone | 5 |

The fixtures include deliberate distractors: an office wifi outage next to the cabin wifi password, a dentist appointment message, a dentist treatment plan and a dentist notes entry, and an airline verification code next to the flight confirmation. Some questions are paraphrased so that keyword overlap alone can't answer them.

**Target: 200 questions by W5.** The set grows with every bug that reaches production.

## Layout

```text
eval/golden/
  fixtures/*.jsonl   # one item per line: {source, item_id, time (RFC 3339), text, metadata, uri?}
  questions.jsonl    # one question per line, see schema.json
  schema.json        # question schema
  baseline*.json     # last accepted metrics; CI fails on a > 2 point drop
```

## Running

```sh
cargo run -p seeqora-eval --release -- --golden eval/golden                   # no filters
cargo run -p seeqora-eval --release -- --golden eval/golden --oracle-filters  # as if query rewrite were perfect
```

Flags: `--json out.json` writes the metrics. `--baseline file.json` fails on a drop of more than 2 points. `--gate` fails below the doctrine bars.

## Metrics

| Metric | Definition | Bar |
|---|---|---|
| Recall@10 | Answerable questions with ≥ 1 expected item among the items of the top 10 chunks | ≥ 0.90 |
| Recall@3 | Same, top 3 | ≥ 0.75 |
| Coverage@10 | Share of *all* expected items in the top 10 (multi-source completeness) | tracked |
| Citation correctness | Every `[n]` resolves to a supporting chunk | ≥ 0.95, from Phase 1 (needs the generator) |
| Unanswerable handled | Says "not found" on negatives | ≥ 0.95, from Phase 1 (needs the generator) |

Until the embedding model is chosen (W4), the dense side uses the test-only `HashEmbedder`, which matches on shared words and does not understand meaning. Early numbers mostly measure keyword matching; their main use is as a regression baseline.
