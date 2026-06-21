use dreamwell_state::{plan_schema, PLAN_BEAT_RULES, STATE_CHANGE_PROMPT};
use dreamwell_types::{Story, StoryActor, StoryBeat, StoryChapter};
use serde_json::{json, Value};

const STORY_PROSE_SYSTEM: &str = r#"You write story beat prose as natural narrative.

Rules:
- Cover every plan beat in order — each beat should be clearly reflected in the prose
- Beats are mandatory staging notes for THIS beat; do not substitute generic filler
- Match the story tone, genre, and POV from the story context
- Use the named characters consistently; do not contradict established typed state or prior canon
- No JSON, no XML tags, no meta commentary — prose only"#;

fn format_story_actors(actors: &[StoryActor]) -> String {
    actors
        .iter()
        .filter_map(|actor| {
            let name = actor.name.trim();
            if name.is_empty() {
                return None;
            }
            let mut part = format!("{name} ({})", actor.role.trim());
            if !actor.description.trim().is_empty() {
                part.push_str(&format!("\n{}", actor.description.trim()));
            }
            Some(part)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

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

pub fn prior_chapter_context(chapters: &[StoryChapter], before_order: i64) -> String {
    chapters
        .iter()
        .filter(|c| c.sort_order < before_order)
        .map(|c| {
            let label = format!("Chapter {} — {}", c.sort_order + 1, c.title);
            if c.prose_summary_valid && !c.prose_summary.trim().is_empty() {
                format!(
                    "{label} (compressed from prose):\n{}",
                    c.prose_summary.trim()
                )
            } else {
                format!("{label}: {}", c.synopsis)
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn prior_beat_synopses(beats: &[StoryBeat], before_order: i64) -> String {
    beats
        .iter()
        .filter(|b| b.sort_order < before_order)
        .map(|b| format!("Beat {} — {}: {}", b.sort_order + 1, b.title, b.synopsis))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn subsequent_beat_synopses(beats: &[StoryBeat], after_order: i64) -> String {
    beats
        .iter()
        .filter(|b| b.sort_order > after_order)
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

pub fn build_beat_mechanical_messages(
    story: &Story,
    chapters: &[StoryChapter],
    chapter: &StoryChapter,
    beat: &StoryBeat,
    guidance: &str,
) -> Vec<Value> {
    let prior_chapters = prior_chapter_synopses(chapters, chapter.sort_order);
    let prior_beats = prior_beat_synopses(&chapter.beats, beat.sort_order);
    let later_beats = subsequent_beat_synopses(&chapter.beats, beat.sort_order);
    let mut user = format!(
        "Expand beat {} (\"{}\") into a mechanical beat plan — concrete staging of THIS beat only.\n\n\
         Scope: Cover only what the beat synopsis describes. \
         Add staging detail (who, where, what happens step by step) but do not invent new plot events, characters, resolutions, or consequences beyond what the synopsis implies. \
         Do not borrow events from the chapter synopsis that belong to later beats. \
         Stop at this beat's natural endpoint — do not advance into later beats or resolve plot points reserved for them.\n\n\
         Beat synopsis (source of truth — elaborate this, do not replace or expand the plot):\n{}\n\n\
         Chapter synopsis (background for tone and chapter arc only — not a checklist; may describe later beats):\n{}",
        beat.sort_order + 1,
        beat.title,
        beat.synopsis.trim(),
        chapter.synopsis,
    );
    if !prior_chapters.is_empty() {
        user.push_str("\n\nPrior chapters (synopses only):\n");
        user.push_str(&prior_chapters);
    }
    if !prior_beats.is_empty() {
        user.push_str("\n\nPrior beats in this chapter:\n");
        user.push_str(&prior_beats);
    }
    if !later_beats.is_empty() {
        user.push_str(
            "\n\nLater beats in this chapter (reserved — do NOT include these events in this mechanical plan):\n",
        );
        user.push_str(&later_beats);
    }
    if !guidance.trim().is_empty() {
        user.push_str("\n\nGuidance from the author:\n");
        user.push_str(guidance.trim());
    }
    user.push_str(
        "\n\nOutput only the mechanical plan: short bullet lines or terse clauses, one per event or action within THIS beat. \
         Include character names, objects, locations, and cause/effect for events in the synopsis. \
         Do not plan events from later beats. \
         No scene narration, dialogue, or literary prose. No headings or meta commentary.",
    );

    vec![
        serde_json::json!({
            "role": "system",
            "content": format!(
                "You are a story structure assistant. Produce a bounded mechanical plan from a beat synopsis. \
                 Stay faithful to the synopsis — add staging detail, not new plot. \
                 Do not extrapolate beyond what the beat synopsis implies or include events planned for later beats.\n\n{}",
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
    variables: &[(String, String)],
    variables_enabled: bool,
) -> Vec<Value> {
    let prior_chapters = prior_chapter_context(chapters, chapter.sort_order);
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
         Mechanical beat plan:\n{}\n\n\
         Scope: Cover ONLY what the mechanical plan lists, in order. \
         Stop when this beat ends — do not advance into later beats or resolve plot points reserved for them. \
         The beat synopsis and chapter synopsis below are background context for tone and direction only — not a checklist.\n\n\
         Beat synopsis (background): {}\n\
         Chapter synopsis (background — may describe later beats): {}",
        beat.sort_order + 1,
        beat.title,
        beat.mechanical.trim(),
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
        user.push_str("\n\nPrior chapters (compressed summaries when available):\n");
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

    let variables_text = crate::story_variables::format_story_variables(variables);
    let mut system = format!(
        "You are a fiction writer. Match the story tone and POV. \
         Respect established details in prior prose — do not contradict names, facts, or events already written.\n\n{}",
        story_basics(story)
    );
    if !variables_text.is_empty() {
        system.push_str("\n\n");
        system.push_str(&variables_text);
    }
    let tracked_text = crate::story_variables::format_tracked_details(&story.tracked_details);
    if !tracked_text.is_empty() {
        system.push_str("\n\n");
        system.push_str(&tracked_text);
    }
    if variables_enabled {
        system.push_str("\n\n");
        system.push_str(crate::story_variables::story_variables_instruction());
    }

    vec![
        serde_json::json!({
            "role": "system",
            "content": system,
        }),
        serde_json::json!({ "role": "user", "content": user }),
    ]
}

pub fn build_beat_prose_continue_messages(
    story: &Story,
    chapters: &[StoryChapter],
    chapter: &StoryChapter,
    beat: &StoryBeat,
    guidance: &str,
    variables: &[(String, String)],
    variables_enabled: bool,
) -> Vec<Value> {
    let prior_chapters = prior_chapter_context(chapters, chapter.sort_order);
    let prior_beats = prior_beat_synopses(&chapter.beats, beat.sort_order);
    let prior_prose = prior_beat_prose(&chapter.beats, beat.sort_order);
    let mut user = format!(
        "Continue the prose for beat {} from where it left off.\n\n\
         Beat title: {}\n\
         Mechanical beat plan:\n{}\n\n\
         Scope: Continue covering what the mechanical plan lists, in order. \
         Pick up after the existing prose below — do not repeat or rewrite text already written. \
         Stop when this beat ends — do not advance into later beats or resolve plot points reserved for them. \
         The beat synopsis and chapter synopsis below are background context for tone and direction only — not a checklist.\n\n\
         Beat synopsis (background): {}\n\
         Chapter synopsis (background — may describe later beats): {}",
        beat.sort_order + 1,
        beat.title,
        beat.mechanical.trim(),
        beat.synopsis,
        chapter.synopsis
    );
    if !prior_prose.is_empty() {
        user.push_str(
            "\n\nPrior beats in this chapter (written prose — canonical; match names, facts, and events):\n",
        );
        user.push_str(&prior_prose);
    }
    if !prior_chapters.is_empty() {
        user.push_str("\n\nPrior chapters (compressed summaries when available):\n");
        user.push_str(&prior_chapters);
    }
    if !prior_beats.is_empty() {
        user.push_str(
            "\n\nPrior beats in this chapter (synopses — written prose above takes precedence):\n",
        );
        user.push_str(&prior_beats);
    }
    user.push_str("\n\nExisting prose for this beat (continue directly from the end):\n");
    user.push_str(beat.content.trim());
    if !guidance.trim().is_empty() {
        user.push_str("\n\nGuidance from the author:\n");
        user.push_str(guidance.trim());
    }
    user.push_str(
        "\n\nWrite only the new narrative prose to append. No headings, labels, meta commentary, or repetition of existing text.",
    );

    let variables_text = crate::story_variables::format_story_variables(variables);
    let mut system = format!(
        "You are a fiction writer. Match the story tone and POV. \
         Respect established details in prior prose — do not contradict names, facts, or events already written.\n\n{}",
        story_basics(story)
    );
    if !variables_text.is_empty() {
        system.push_str("\n\n");
        system.push_str(&variables_text);
    }
    let tracked_text = crate::story_variables::format_tracked_details(&story.tracked_details);
    if !tracked_text.is_empty() {
        system.push_str("\n\n");
        system.push_str(&tracked_text);
    }
    if variables_enabled {
        system.push_str("\n\n");
        system.push_str(crate::story_variables::story_variables_instruction());
    }

    vec![
        serde_json::json!({
            "role": "system",
            "content": system,
        }),
        serde_json::json!({ "role": "user", "content": user }),
    ]
}

pub fn build_beat_prose_continue_typed_messages(
    story: &Story,
    chapters: &[StoryChapter],
    chapter: &StoryChapter,
    beat: &StoryBeat,
    guidance: &str,
    state_block: &str,
) -> Vec<Value> {
    let prior_chapters = prior_chapter_context(chapters, chapter.sort_order);
    let prior_beats = prior_beat_synopses(&chapter.beats, beat.sort_order);
    let prior_prose = prior_beat_prose(&chapter.beats, beat.sort_order);
    let mut user = format!(
        "Continue the prose for beat {} from where it left off.\n\n\
         Beat title: {}\n\
         Mechanical beat plan:\n{}\n\n\
         Scope: Continue covering what the mechanical plan lists, in order. \
         Pick up after the existing prose below — do not repeat or rewrite text already written. \
         Stop when this beat ends — do not advance into later beats or resolve plot points reserved for them. \
         The beat synopsis and chapter synopsis below are background context for tone and direction only — not a checklist.\n\n\
         Beat synopsis (background): {}\n\
         Chapter synopsis (background — may describe later beats): {}",
        beat.sort_order + 1,
        beat.title,
        beat.mechanical.trim(),
        beat.synopsis,
        chapter.synopsis
    );
    if !prior_prose.is_empty() {
        user.push_str(
            "\n\nPrior beats in this chapter (written prose — canonical; match names, facts, and events):\n",
        );
        user.push_str(&prior_prose);
    }
    if !prior_chapters.is_empty() {
        user.push_str("\n\nPrior chapters (compressed summaries when available):\n");
        user.push_str(&prior_chapters);
    }
    if !prior_beats.is_empty() {
        user.push_str(
            "\n\nPrior beats in this chapter (synopses — written prose above takes precedence):\n",
        );
        user.push_str(&prior_beats);
    }
    user.push_str("\n\nExisting prose for this beat (continue directly from the end):\n");
    user.push_str(beat.content.trim());
    if !guidance.trim().is_empty() {
        user.push_str("\n\nGuidance from the author:\n");
        user.push_str(guidance.trim());
    }
    user.push_str(
        "\n\nWrite only the new narrative prose to append. No headings, labels, meta commentary, or repetition of existing text.",
    );
    if !state_block.is_empty() {
        user.push_str(&format!("\n\nCurrent typed state:\n{state_block}"));
    }

    let mut system = format!(
        "You are a fiction writer. Match the story tone and POV. \
         Respect established details in prior prose — do not contradict names, facts, or events already written.\n\n{}",
        story_basics(story)
    );
    let tracked_text = crate::story_variables::format_tracked_details(&story.tracked_details);
    if !tracked_text.is_empty() {
        system.push_str("\n\n");
        system.push_str(&tracked_text);
    }

    vec![
        serde_json::json!({
            "role": "system",
            "content": system,
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

const STORY_PLAN_SYSTEM: &str = r#"You plan story beat prose before it is written.

Given the beat synopsis and story context, output JSON with:
- plan_beats: short, specific bullet points this beat's prose must cover (in order)
- state_changes: typed durable state updates that should persist after this beat

Plan ONLY this beat — concrete staging from the synopsis and prior prose, not generic story beats.

Do not write final prose in this step — beats and state only."#;

pub fn story_plan_schema() -> serde_json::Value {
    plan_schema("plan_beats")
}

pub fn build_story_plan_messages(
    story: &Story,
    _chapters: &[StoryChapter],
    chapter: &StoryChapter,
    beat: &StoryBeat,
    state_block: &str,
    guidance: &str,
) -> Vec<serde_json::Value> {
    let mut user = format!(
        "{}\n\nChapter {} — {}:\n{}\n\nBeat {} — {}:\n{}\n\nBeat synopsis:\n{}",
        story_basics(story),
        chapter.sort_order + 1,
        chapter.title,
        chapter.synopsis,
        beat.sort_order + 1,
        beat.title,
        beat.synopsis,
        beat.synopsis
    );
    if !state_block.is_empty() {
        user.push_str(&format!("\n\nCurrent typed state:\n{state_block}"));
    }
    if !guidance.trim().is_empty() {
        user.push_str(&format!("\n\nAuthor guidance:\n{}", guidance.trim()));
    }
    if !beat.mechanical.trim().is_empty() {
        user.push_str("\n\nMechanical beat plan (use as staging — plan_beats should be specific clauses drawn from this):\n");
        user.push_str(beat.mechanical.trim());
    }
    user.push_str(
        "\n\nOutput plan_beats as concrete staging for THIS beat only — avoid generic beats that could fit any story moment.",
    );
    vec![
        json!({
            "role": "system",
            "content": format!("{STORY_PLAN_SYSTEM}\n\n{PLAN_BEAT_RULES}\n\n{STATE_CHANGE_PROMPT}"),
        }),
        json!({
            "role": "user",
            "content": user,
        }),
    ]
}

pub fn build_story_prose_from_plan_messages(
    story: &Story,
    chapter: &StoryChapter,
    beat: &StoryBeat,
    plan_beats: &[String],
    state_block: &str,
    guidance: &str,
    actors: &[StoryActor],
) -> Vec<serde_json::Value> {
    let beats_text = plan_beats
        .iter()
        .map(|b| format!("- {b}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut user = format!("Plan beats to cover:\n{beats_text}");
    if !state_block.is_empty() {
        user.push_str(&format!("\n\nCurrent typed state:\n{state_block}"));
    }
    user.push_str(&format!("\n\n{}", story_basics(story)));
    let actors_text = format_story_actors(actors);
    if !actors_text.is_empty() {
        user.push_str(&format!("\n\nCharacters:\n{actors_text}"));
    }
    let tracked_text = crate::story_variables::format_tracked_details(&story.tracked_details);
    if !tracked_text.is_empty() {
        user.push_str(&format!("\n\n{tracked_text}"));
    }
    user.push_str(&format!(
        "\n\nChapter {} — {}: {}\nBeat {} — {}: {}",
        chapter.sort_order + 1,
        chapter.title,
        chapter.synopsis,
        beat.sort_order + 1,
        beat.title,
        beat.synopsis,
    ));
    if !guidance.trim().is_empty() {
        user.push_str(&format!("\n\nAuthor guidance:\n{}", guidance.trim()));
    }
    vec![
        json!({
            "role": "system",
            "content": STORY_PROSE_SYSTEM,
        }),
        json!({
            "role": "user",
            "content": user,
        }),
    ]
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
            tracked_details: String::new(),
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
            mechanical: String::new(),
            content: content.to_string(),
            variable_updates: Vec::new(),
            plan_beats: Vec::new(),
            state_changes: Vec::new(),
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
            prose_summary: String::new(),
            prose_summary_valid: false,
            prose_summary_at: None,
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
    fn subsequent_beat_synopses_excludes_current_and_prior() {
        let beats = vec![
            sample_beat(0, "Arrival", "They arrive.", ""),
            sample_beat(1, "Market", "They shop.", ""),
            sample_beat(2, "Fight", "A brawl.", ""),
        ];
        let later = subsequent_beat_synopses(&beats, 0);
        assert!(later.contains("Market"));
        assert!(later.contains("Fight"));
        assert!(!later.contains("Arrival"));

        let later_from_middle = subsequent_beat_synopses(&beats, 1);
        assert!(later_from_middle.contains("Fight"));
        assert!(!later_from_middle.contains("Market"));
        assert!(!later_from_middle.contains("Arrival"));
    }

    #[test]
    fn beat_mechanical_prompt_includes_scope_guard_and_later_beats() {
        let chapter = sample_chapter(
            0,
            vec![
                sample_beat(0, "Arrival", "They arrive at the inn.", ""),
                sample_beat(1, "Market", "She haggles for bread.", ""),
                sample_beat(2, "Fight", "A brawl breaks out.", ""),
            ],
        );
        let beat = chapter.beats[0].clone();
        let messages = build_beat_mechanical_messages(
            &sample_story(),
            &[chapter.clone()],
            &chapter,
            &beat,
            "",
        );
        let user = messages[1]["content"].as_str().unwrap();
        let system = messages[0]["content"].as_str().unwrap();

        assert!(user.contains("THIS beat only"));
        assert!(user.contains("do not invent new plot events"));
        assert!(user.contains("do not advance into later beats"));
        assert!(user.contains("Later beats in this chapter (reserved"));
        assert!(user.contains("She haggles for bread"));
        assert!(user.contains("A brawl breaks out"));
        assert!(user.contains("not a checklist"));
        assert!(system.contains("Do not extrapolate beyond what the beat synopsis implies"));
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
        let mut beat = chapter.beats[1].clone();
        beat.mechanical =
            "- She enters the market.\n- She haggles for bread.\n- She leaves with a loaf."
                .to_string();
        let messages = build_beat_prose_messages(
            &sample_story(),
            &[chapter.clone()],
            &chapter,
            &beat,
            "",
            &[],
            false,
        );
        let user = messages[1]["content"].as_str().unwrap();
        let system = messages[0]["content"].as_str().unwrap();

        assert!(user.contains("Write the prose for beat 2 only"));
        assert!(user.contains("Mechanical beat plan:"));
        assert!(user.contains("She haggles for bread"));
        assert!(user.contains("do not advance into later beats"));
        assert!(user.contains("The wagon rolled to a stop."));
        assert!(user.contains("written prose — canonical"));
        assert!(system.contains("Respect established details in prior prose"));
    }

    #[test]
    fn beat_prose_prompt_formats_variables_as_tags_and_includes_tracked_details() {
        let chapter = sample_chapter(0, vec![sample_beat(0, "Start", "They begin.", "")]);
        let beat = chapter.beats[0].clone();
        let mut story = sample_story();
        story.tracked_details = "- protagonist name\n- the silver locket".to_string();
        let messages = build_beat_prose_messages(
            &story,
            &[chapter.clone()],
            &chapter,
            &beat,
            "",
            &[("baker_name".to_string(), "Tomas".to_string())],
            true,
        );
        let system = messages[0]["content"].as_str().unwrap();
        assert!(system.contains(r#"<var key="baker_name">Tomas</var>"#));
        assert!(system.contains("Important details to track"));
        assert!(system.contains("silver locket"));
    }

    #[test]
    fn beat_prose_continue_prompt_includes_existing_prose_and_append_instruction() {
        let chapter = sample_chapter(
            0,
            vec![sample_beat(
                0,
                "Market",
                "She buys bread.",
                "She walked into the market.",
            )],
        );
        let mut beat = chapter.beats[0].clone();
        beat.mechanical = "- She haggles for bread.\n- She leaves with a loaf.".to_string();
        let messages = build_beat_prose_continue_messages(
            &sample_story(),
            &[chapter.clone()],
            &chapter,
            &beat,
            "",
            &[],
            false,
        );
        let user = messages[1]["content"].as_str().unwrap();

        assert!(user.contains("Continue the prose for beat 1"));
        assert!(user.contains("Existing prose for this beat"));
        assert!(user.contains("She walked into the market."));
        assert!(user.contains("do not repeat or rewrite text already written"));
        assert!(user.contains("Write only the new narrative prose to append"));
    }

    #[test]
    fn story_prose_from_plan_includes_premise_and_characters() {
        use dreamwell_types::StoryActor;

        let chapter = sample_chapter(
            0,
            vec![sample_beat(0, "Arrival", "Mira reaches the inn.", "")],
        );
        let beat = chapter.beats[0].clone();
        let story = sample_story();
        let actors = vec![StoryActor {
            id: 1,
            story_id: 1,
            role: "pc".to_string(),
            name: "Mira".to_string(),
            description: "A cautious mapmaker.".to_string(),
            skills: Default::default(),
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];
        let messages = build_story_prose_from_plan_messages(
            &story,
            &chapter,
            &beat,
            &["Mira asks the innkeeper about the cellar.".to_string()],
            "stress (resource): 1/5",
            "Keep it tense.",
            &actors,
        );
        let user = messages[1]["content"].as_str().unwrap();
        let system = messages[0]["content"].as_str().unwrap();

        assert!(user.contains("Plan beats to cover:"));
        assert!(user.contains("Mira asks the innkeeper about the cellar."));
        assert!(user.contains("Premise: A hero finds a map."));
        assert!(user.contains("POV: Third person"));
        assert!(user.contains("Characters:"));
        assert!(user.contains("Mira (pc)"));
        assert!(user.contains("A cautious mapmaker."));
        assert!(user.contains("Current typed state:"));
        assert!(user.contains("Author guidance:"));
        assert!(system.contains("Cover every plan beat in order"));
    }
}
