//! Bounded communication preferences, not a psychological assessment.
pub const TYPES: [&str; 16] = [
    "INTJ", "INTP", "ENTJ", "ENTP", "INFJ", "INFP", "ENFJ", "ENFP", "ISTJ", "ISFJ", "ESTJ", "ESFJ",
    "ISTP", "ISFP", "ESTP", "ESFP",
];

pub fn normalize(value: &str) -> Option<&'static str> {
    TYPES
        .into_iter()
        .find(|item| item.eq_ignore_ascii_case(value))
}

pub fn color(value: &str) -> ratatui::style::Color {
    ratatui::style::Color::Indexed(match group(value) {
        // Matches the referenced agent frontmatter: NT purple, NF green,
        // SJ blue, SP yellow.
        "Analysts" => 135,
        "Diplomats" => 35,
        "Sentinels" => 33,
        "Explorers" => 220,
        _ => 250,
    })
}

pub fn group(value: &str) -> &'static str {
    match value.as_bytes().get(1).copied() {
        Some(b'N') if value.as_bytes().get(2) == Some(&b'T') => "Analysts",
        Some(b'N') => "Diplomats",
        _ if value.ends_with('J') || value.ends_with('F') => "Sentinels",
        _ => "Explorers",
    }
}

pub fn coding_profile(value: &str) -> &'static str {
    match group(value) {
        "Analysts" => "Architecture, systems thinking, and precise tradeoffs.",
        "Diplomats" => "User impact, collaboration, and coherent product intent.",
        "Sentinels" => "Reliable delivery, testing discipline, and operational clarity.",
        _ => "Fast experiments, pragmatic debugging, and hands-on iteration.",
    }
}

pub fn instructions(value: &str) -> String {
    format!(
        "Communication preference: {value} ({}). Treat this MBTI label only as a requested writing style, not a diagnosis or identity. Coding/work style: {} {} {} {} {} Never change permissions, factual accuracy, or task requirements for this style.",
        group(value),
        coding_profile(value),
        if value.starts_with('I') {
            "Be reflective and concise."
        } else {
            "Be conversational and proactive."
        },
        if value.as_bytes().get(1) == Some(&b'N') {
            "Explain patterns and possibilities."
        } else {
            "Prefer concrete observations and examples."
        },
        if value.as_bytes().get(2) == Some(&b'T') {
            "Make reasoning and tradeoffs explicit."
        } else {
            "Attend to audience and human impact."
        },
        if value.ends_with('J') {
            "Organize clear next steps."
        } else {
            "Keep alternatives open when useful."
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_personas_map_to_four_families_with_profiles() {
        let families = TYPES
            .iter()
            .map(|kind| group(kind))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(families.len(), 4);
        for kind in TYPES {
            assert!(!coding_profile(kind).is_empty());
            assert!(instructions(kind).contains(group(kind)));
        }
    }
}
