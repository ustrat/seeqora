# Golden query set

Tenet 10 says to measure retrieval before polishing chat. This set gates every phase.

## What it is

A synthetic phone (seeded messages, photos, notes, files, contacts and events) plus **200 questions**, each with known correct source items. The questions cover four kinds:

| Kind | Example |
|---|---|
| `lookup` | "what's the Wi-Fi password Dana sent" |
| `temporal` | "what did I do last Thursday" |
| `multi_source` | "the PDF from the person I met at the dentist" |
| `negative` | the answer is not on the phone; the expected output is "not found" |

The set grows with every bug that reaches production.

## Layout

```
eval/golden/
  fixtures/        # synthetic phone: one JSONL of core Records per source
  questions.jsonl  # one question per line, see schema.json
  schema.json      # question schema
```

## Bars (release-blocking)

| Metric | Bar |
|---|---|
| Recall@10 | ≥ 0.90 |
| Recall@3 | ≥ 0.75 |
| Citation correctness | ≥ 0.95 |
| Unanswerable handled | ≥ 0.95 |

The nightly device-farm run fails if any bar drops by more than 2 points. Fixtures land in Phase 0 W2 and reach 200 questions by W5.
