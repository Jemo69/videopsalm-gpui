# Race 1 (frame/caption wrap) — winner
Baseline: vp-core 251/252 (`long_file_caption_slide_wraps_without_overrun` FAILED: wrap_line drops the space at soft breaks, concat loses a char).
Attempts:
- A trailing-space in wrap_line: caption concat fixed, but verse `join(" ")` test broke (double spaces), then fix_orphan_word (re-joins with single spaces) stripped them → rejected (blast radius across verse pipeline).
- B change caption test to join(" "): broke on hard-split pieces (".mp4" gained a space) → rejected.
- C WINNER: new wrap_caption (char-preserving, break after last space// within cap, else hard cut) for file_slide + URL slides; wrap_line untouched. Grafted hard-cut fallback from A.
Result: vp-core 252/252.
