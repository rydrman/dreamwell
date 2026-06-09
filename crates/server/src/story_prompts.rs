use dreamwell_types::{Story, StoryBeat, StoryChapter};
use serde_json::Value;

pub fn story_basics(story: &Story) -> String {
    let preset = story.length_preset;
    format!(
        "Title: {}\nPremise: {}\nTone: {}\nGenre: {}\nPOV: {}\nLength: {} (target ~{} chapters)\nAuthor notes: {}",
        story.title,
        story.premise,
        story.tone,
        story.genre,
        story.pov,
        preset.label(),
        preset.ref_chapters(),
        story.notes,
    )
}

pub fn prior_chapter_synopses(chapters: &[StoryChapter], before_order: i64) -> String {
    chapters
        .iter()
        .filter(|c| c.sort_order < before_order)
        .map(|c| format!("Chapter {} — {}: {}", c.sort_order + 1, c.title, c.synopsis))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn prior_beat_synopses(beats: &[StoryBeat], before_order: i64) -> String {
    beats
        .iter()
        .filter(|b| b.sort_order < before_order)
        .map(|b| format!("Beat {} — {}: {}", b.sort_order + 1, b.title, b.synopsis))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn build_chapter_outline_messages(
    story: &Story,
    chapters: &[StoryChapter],
    chapter_order: i64,
    guidance: &str,
) -> Vec<Value> {
    let prior = prior_chapter_synopses(chapters, chapter_order);
    let mut user = format!(
        "Plan chapter {} of ~{} for this story.\n\nRespond with ONLY valid JSON: {{\"title\":\"...\",\"synopsis\":\"...\"}}\nThe synopsis should be 2-4 sentences covering the chapter arc.",
        chapter_order + 1,
        story.length_preset.ref_chapters(),
    );
    if !prior.is_empty() {
        user.push_str("\n\nPrior chapters (synopses only):\n");
        user.push_str(&prior);
    }
    if !guidance.trim().is_empty() {
        user.push_str("\n\nGuidance from the author:\n");
        user.push_str(guidance.trim());
    }
    vec![
        serde_json::json!({
            "role": "system",
            "content": format!(
                "You are a story structure assistant. Output strict JSON only — no markdown fences.\n\n{}",
                story_basics(story)
            ),
        }),
        serde_json::json!({ "role": "user", "content": user }),
    ]
}

pub fn build_beat_outline_messages(
    story: &Story,
    chapters: &[StoryChapter],
    chapter: &StoryChapter,
    beat_order: i64,
    guidance: &str,
) -> Vec<Value> {
    let prior_chapters = prior_chapter_synopses(chapters, chapter.sort_order);
    let prior_beats = prior_beat_synopses(&chapter.beats, beat_order);
    let mut user = format!(
        "Plan beat {} for chapter \"{}\".\n\nRespond with ONLY valid JSON: {{\"title\":\"...\",\"synopsis\":\"...\"}}\nThe synopsis should be 1-3 sentences describing what happens in this beat.",
        beat_order + 1,
        chapter.title,
    );
    user.push_str("\n\nChapter synopsis:\n");
    user.push_str(&chapter.synopsis);
    if !prior_chapters.is_empty() {
        user.push_str("\n\nPrior chapters (synopses only):\n");
        user.push_str(&prior_chapters);
    }
    if !prior_beats.is_empty() {
        user.push_str("\n\nPrior beats in this chapter:\n");
        user.push_str(&prior_beats);
    }
    if !guidance.trim().is_empty() {
        user.push_str("\n\nGuidance from the author:\n");
        user.push_str(guidance.trim());
    }
    vec![
        serde_json::json!({
            "role": "system",
            "content": format!(
                "You are a story structure assistant. Output strict JSON only — no markdown fences.\n\n{}",
                story_basics(story)
            ),
        }),
        serde_json::json!({ "role": "user", "content": user }),
    ]
}

pub fn build_beat_prose_messages(
    story: &Story,
    chapters: &[StoryChapter],
    chapter: &StoryChapter,
    beat: &StoryBeat,
    guidance: &str,
) -> Vec<Value> {
    let prior_chapters = prior_chapter_synopses(chapters, chapter.sort_order);
    let prior_beats = prior_beat_synopses(&chapter.beats, beat.sort_order);
    let mut user = format!(
        "Write the prose for this beat.\n\nBeat title: {}\nBeat synopsis: {}\n\nChapter synopsis: {}",
        beat.title, beat.synopsis, chapter.synopsis
    );
    if !prior_chapters.is_empty() {
        user.push_str("\n\nPrior chapters (synopses only):\n");
        user.push_str(&prior_chapters);
    }
    if !prior_beats.is_empty() {
        user.push_str("\n\nPrior beats in this chapter (synopses only):\n");
        user.push_str(&prior_beats);
    }
    if !guidance.trim().is_empty() {
        user.push_str("\n\nGuidance from the author:\n");
        user.push_str(guidance.trim());
    }
    user.push_str("\n\nWrite only the narrative prose for this beat. No headings, labels, or meta commentary.");
    vec![
        serde_json::json!({
            "role": "system",
            "content": format!(
                "You are a fiction writer. Match the story tone and POV.\n\n{}",
                story_basics(story)
            ),
        }),
        serde_json::json!({ "role": "user", "content": user }),
    ]
}

pub fn build_full_outline_messages(
    story: &Story,
    chapters: &[StoryChapter],
    chapter_indices: &[usize],
    guidance: &str,
) -> Vec<Value> {
    let filled: Vec<String> = chapters
        .iter()
        .enumerate()
        .filter(|(i, c)| !chapter_indices.contains(i) && !c.title.is_empty())
        .map(|(i, c)| format!("Chapter {} — {}: {}", i + 1, c.title, c.synopsis))
        .collect();
    let to_generate: Vec<String> = chapter_indices
        .iter()
        .map(|i| format!("Chapter {}", i + 1))
        .collect();
    let mut user = format!(
        "Plan a {}-chapter story outline. Generate outlines for: {}.\n\nRespond with ONLY valid JSON: {{\"chapters\":[{{\"title\":\"...\",\"synopsis\":\"...\"}}, ...]}}\nEach synopsis should be 2-4 sentences. Return exactly {} chapter object(s) in order.",
        story.length_preset.ref_chapters(),
        to_generate.join(", "),
        chapter_indices.len(),
    );
    if !filled.is_empty() {
        user.push_str("\n\nAlready planned chapters (keep consistent with these):\n");
        user.push_str(&filled.join("\n"));
    }
    if !guidance.trim().is_empty() {
        user.push_str("\n\nGuidance from the author:\n");
        user.push_str(guidance.trim());
    }
    vec![
        serde_json::json!({
            "role": "system",
            "content": format!(
                "You are a story structure assistant. Output strict JSON only — no markdown fences.\n\n{}",
                story_basics(story)
            ),
        }),
        serde_json::json!({ "role": "user", "content": user }),
    ]
}

pub fn parse_full_outline_json(text: &str) -> Option<Vec<(String, String)>> {
    let trimmed = text.trim();
    let json_str = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let value: Value = serde_json::from_str(json_str).ok()?;
    let chapters = value.get("chapters")?.as_array()?;
    let mut out = Vec::with_capacity(chapters.len());
    for ch in chapters {
        let title = ch.get("title")?.as_str()?.trim().to_string();
        let synopsis = ch.get("synopsis")?.as_str()?.trim().to_string();
        if title.is_empty() {
            return None;
        }
        out.push((title, synopsis));
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

pub fn parse_outline_json(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    let json_str = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let value: Value = serde_json::from_str(json_str).ok()?;
    let title = value.get("title")?.as_str()?.trim().to_string();
    let synopsis = value.get("synopsis")?.as_str()?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    Some((title, synopsis))
}
