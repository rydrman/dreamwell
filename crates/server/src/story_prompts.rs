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

fn chapter_snapshot(chapter: &StoryChapter, index: usize) -> String {
    let beat_count = chapter.beats.len();
    let prose_beats = chapter
        .beats
        .iter()
        .filter(|b| b.content.chars().count() > 80)
        .count();
    let title = if chapter.title.is_empty() {
        "(untitled)".to_string()
    } else {
        chapter.title.clone()
    };
    let synopsis = if chapter.synopsis.is_empty() {
        "(no synopsis)".to_string()
    } else {
        chapter.synopsis.clone()
    };
    let mut line = format!("Chapter {} — {}: {}", index + 1, title, synopsis);
    if beat_count > 0 {
        line.push_str(&format!(" [{beat_count} beats"));
        if prose_beats > 0 {
            line.push_str(&format!(", {prose_beats} with substantial prose"));
        }
        line.push(']');
    }
    line
}

fn beat_snapshot(beat: &StoryBeat, index: usize) -> String {
    let title = if beat.title.is_empty() {
        "(untitled)".to_string()
    } else {
        beat.title.clone()
    };
    let synopsis = if beat.synopsis.is_empty() {
        "(no synopsis)".to_string()
    } else {
        beat.synopsis.clone()
    };
    let prose_chars = beat.content.chars().count();
    let mut line = format!("Beat {} — {}: {}", index + 1, title, synopsis);
    if prose_chars > 80 {
        line.push_str(&format!(
            " [{prose_chars} chars of prose — preserve unless restructuring requires otherwise]"
        ));
    }
    line
}

pub fn build_propose_chapters_messages(
    story: &Story,
    chapters: &[StoryChapter],
    guidance: &str,
) -> Vec<Value> {
    let target = story.length_preset.ref_chapters();
    let mut user = format!(
        "Review this story and propose a complete chapter outline (~{target} chapters for this length preset, but use your judgment).\n\n\
         You may add, remove, merge, split, reorder, or rewrite chapters. \
         Prefer to keep chapters that already have substantial beat prose unless the author guidance says otherwise.\n\n\
         Respond with ONLY valid JSON: {{\"chapters\":[{{\"title\":\"...\",\"synopsis\":\"...\"}}, ...]}}\n\
         Each synopsis should be 2-4 sentences. Return the full proposed chapter list in reading order.",
    );
    if chapters.is_empty() {
        user.push_str("\n\nThere are no chapters yet — propose the full outline from the premise.");
    } else {
        user.push_str("\n\nCurrent chapters:\n");
        user.push_str(
            &chapters
                .iter()
                .enumerate()
                .map(|(i, c)| chapter_snapshot(c, i))
                .collect::<Vec<_>>()
                .join("\n"),
        );
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

pub fn build_propose_beats_messages(
    story: &Story,
    chapters: &[StoryChapter],
    chapter: &StoryChapter,
    guidance: &str,
) -> Vec<Value> {
    let prior_chapters = prior_chapter_synopses(chapters, chapter.sort_order);
    let mut user = format!(
        "Review chapter \"{}\" and propose a complete beat breakdown for it.\n\n\
         Chapter synopsis: {}\n\n\
         You may add, remove, reorder, or rewrite beats. \
         Prefer to keep beats that already have substantial prose unless the author guidance says otherwise.\n\n\
         Respond with ONLY valid JSON: {{\"beats\":[{{\"title\":\"...\",\"synopsis\":\"...\"}}, ...]}}\n\
         Each synopsis should be 1-3 sentences. Return the full proposed beat list in reading order.",
        if chapter.title.is_empty() {
            "Untitled chapter"
        } else {
            &chapter.title
        },
        if chapter.synopsis.is_empty() {
            "(no synopsis yet)"
        } else {
            &chapter.synopsis
        },
    );
    if !prior_chapters.is_empty() {
        user.push_str("\n\nPrior chapters (synopses only):\n");
        user.push_str(&prior_chapters);
    }
    if chapter.beats.is_empty() {
        user.push_str("\n\nThere are no beats yet — propose a beat breakdown for this chapter.");
    } else {
        user.push_str("\n\nCurrent beats:\n");
        user.push_str(
            &chapter
                .beats
                .iter()
                .enumerate()
                .map(|(i, b)| beat_snapshot(b, i))
                .collect::<Vec<_>>()
                .join("\n"),
        );
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

pub fn parse_chapters_proposal_json(text: &str) -> Option<Vec<(String, String)>> {
    parse_proposal_array_json(text, "chapters")
}

pub fn parse_beats_proposal_json(text: &str) -> Option<Vec<(String, String)>> {
    parse_proposal_array_json(text, "beats")
}

fn parse_proposal_array_json(text: &str, key: &str) -> Option<Vec<(String, String)>> {
    let trimmed = text.trim();
    let json_str = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let value: Value = serde_json::from_str(json_str).ok()?;
    let items = value.get(key)?.as_array()?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let title = item.get("title")?.as_str()?.trim().to_string();
        let synopsis = item.get("synopsis")?.as_str()?.trim().to_string();
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
