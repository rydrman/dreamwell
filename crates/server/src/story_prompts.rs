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

/// Maximum characters of prior prose to include when generating a beat.
const PRIOR_PROSE_CHAR_LIMIT: usize = 16_000;

pub fn prior_beat_prose(beats: &[StoryBeat], before_order: i64) -> String {
    let sections: Vec<String> = beats
        .iter()
        .filter(|b| b.sort_order < before_order && !b.content.trim().is_empty())
        .map(|b| {
            format!(
                "Beat {} — {}\n{}",
                b.sort_order + 1,
                b.title,
                b.content.trim()
            )
        })
        .collect();
    cap_prior_prose(sections, PRIOR_PROSE_CHAR_LIMIT)
}

pub fn prior_chapter_closing_prose(chapters: &[StoryChapter], chapter_order: i64) -> String {
    let Some(prior_chapter) = chapters
        .iter()
        .filter(|c| c.sort_order < chapter_order)
        .max_by_key(|c| c.sort_order)
    else {
        return String::new();
    };
    prior_chapter
        .beats
        .iter()
        .max_by_key(|b| b.sort_order)
        .map(|b| b.content.trim())
        .filter(|content| !content.is_empty())
        .map(str::to_string)
        .unwrap_or_default()
}

fn cap_prior_prose(mut sections: Vec<String>, max_chars: usize) -> String {
    if sections.is_empty() {
        return String::new();
    }
    let mut combined = sections.join("\n\n");
    while combined.chars().count() > max_chars && sections.len() > 1 {
        sections.remove(0);
        combined = sections.join("\n\n");
    }
    if combined.chars().count() <= max_chars {
        return combined;
    }
    truncate_prose_from_start(&combined, max_chars)
}

fn truncate_prose_from_start(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }
    let skip = char_count - max_chars;
    format!(
        "[…earlier prose truncated…]\n\n{}",
        text.chars().skip(skip).collect::<String>()
    )
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
    let prior_prose = prior_beat_prose(&chapter.beats, beat.sort_order);
    let chapter_opening = if beat.sort_order == 0 {
        prior_chapter_closing_prose(chapters, chapter.sort_order)
    } else {
        String::new()
    };
    let mut user = format!(
        "Write the prose for beat {} only.\n\n\
         Beat title: {}\n\
         Beat synopsis: {}\n\n\
         Scope: Cover ONLY what this beat's synopsis describes. \
         Stop when this beat ends — do not advance into later beats or resolve plot points reserved for them. \
         The chapter synopsis below describes the whole chapter arc; use it for tone and direction only, not as a checklist for this beat.\n\n\
         Chapter synopsis (background — may describe later beats): {}",
        beat.sort_order + 1,
        beat.title,
        beat.synopsis,
        chapter.synopsis
    );
    if !chapter_opening.is_empty() {
        user.push_str("\n\nEnd of previous chapter (continue from here):\n");
        user.push_str(&chapter_opening);
    }
    if !prior_prose.is_empty() {
        user.push_str(
            "\n\nPrior beats in this chapter (written prose — canonical; match names, facts, and events):\n",
        );
        user.push_str(&prior_prose);
    }
    if !prior_chapters.is_empty() {
        user.push_str("\n\nPrior chapters (synopses only):\n");
        user.push_str(&prior_chapters);
    }
    if !prior_beats.is_empty() {
        user.push_str(
            "\n\nPrior beats in this chapter (synopses — written prose above takes precedence):\n",
        );
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
                "You are a fiction writer. Match the story tone and POV. \
                 Respect established details in prior prose — do not contradict names, facts, or events already written.\n\n{}",
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use dreamwell_types::LengthPreset;

    fn sample_story() -> Story {
        Story {
            id: 1,
            title: "Test Story".to_string(),
            premise: "A hero finds a map.".to_string(),
            tone: "Adventurous".to_string(),
            genre: "Fantasy".to_string(),
            pov: "Third person".to_string(),
            length_preset: LengthPreset::Short,
            notes: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            active_job: None,
            queued_jobs: 0,
        }
    }

    fn sample_beat(order: i64, title: &str, synopsis: &str, content: &str) -> StoryBeat {
        StoryBeat {
            id: order + 1,
            chapter_id: 1,
            title: title.to_string(),
            synopsis: synopsis.to_string(),
            content: content.to_string(),
            sort_order: order,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            job_status: None,
        }
    }

    fn sample_chapter(order: i64, beats: Vec<StoryBeat>) -> StoryChapter {
        StoryChapter {
            id: order + 1,
            story_id: 1,
            title: format!("Chapter {}", order + 1),
            synopsis: format!("Chapter {} synopsis", order + 1),
            sort_order: order,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            beats,
        }
    }

    #[test]
    fn prior_beat_prose_includes_only_earlier_beats() {
        let beats = vec![
            sample_beat(0, "Arrival", "They arrive.", "The wagon rolled in."),
            sample_beat(1, "Market", "They shop.", "She bought bread."),
            sample_beat(2, "Fight", "A brawl.", ""),
        ];
        let prose = prior_beat_prose(&beats, 2);
        assert!(prose.contains("The wagon rolled in."));
        assert!(prose.contains("She bought bread."));
        assert!(!prose.contains("brawl"));
    }

    #[test]
    fn prior_chapter_closing_prose_uses_last_beat() {
        let chapters = vec![
            sample_chapter(
                0,
                vec![
                    sample_beat(0, "Start", "Begin.", "Opening line."),
                    sample_beat(1, "End", "Finish.", "Closing line."),
                ],
            ),
            sample_chapter(1, vec![sample_beat(0, "Next", "Continue.", "")]),
        ];
        assert_eq!(prior_chapter_closing_prose(&chapters, 1), "Closing line.");
    }

    #[test]
    fn beat_prose_prompt_includes_prior_prose_and_scope_guard() {
        let chapter = sample_chapter(
            0,
            vec![
                sample_beat(
                    0,
                    "Arrival",
                    "They arrive at the inn.",
                    "The wagon rolled to a stop.",
                ),
                sample_beat(1, "Market", "She haggles for bread.", ""),
            ],
        );
        let beat = chapter.beats[1].clone();
        let messages =
            build_beat_prose_messages(&sample_story(), &[chapter.clone()], &chapter, &beat, "");
        let user = messages[1]["content"].as_str().unwrap();
        let system = messages[0]["content"].as_str().unwrap();

        assert!(user.contains("Write the prose for beat 2 only"));
        assert!(user.contains("do not advance into later beats"));
        assert!(user.contains("The wagon rolled to a stop."));
        assert!(user.contains("written prose — canonical"));
        assert!(system.contains("Respect established details in prior prose"));
    }
}
