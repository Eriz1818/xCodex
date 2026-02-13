pub const WHATS_NEW_MD: &str = include_str!("whats_new.md");

pub fn whats_new_bullets_for_version(version: &str) -> Option<Vec<String>> {
    let mut in_section = false;
    let mut bullets = Vec::new();
    let version_token = format!("v{version}");

    for line in WHATS_NEW_MD.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("What's new in") {
            if in_section {
                break;
            }
            if trimmed.contains(&version_token) {
                in_section = true;
            }
            continue;
        }

        if !in_section {
            continue;
        }

        let Some(rest) = trimmed.strip_prefix('-') else {
            continue;
        };
        let bullet = rest.trim();
        if bullet.is_empty() {
            continue;
        }
        bullets.push(bullet.to_string());
    }

    (!bullets.is_empty()).then_some(bullets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn extracts_bullets_for_matching_version() {
        let bullets = whats_new_bullets_for_version("0.4.0").expect("expected bullets");
        assert_eq!(
            bullets,
            vec![
                "New side-by-side diff view for clearer, faster review workflows.".to_string(),
                "Plan mode now toggles with `Shift+Tab` (no `/plan` command needed).".to_string(),
                "Plan mode now supports `default` (Codex-style), `adr-lite`, and `custom` workflows."
                    .to_string(),
                "You can customize and seed your own Plan workflow template from `default` or `adr-lite`."
                    .to_string(),
                "Expanded Plan mode with richer options for planning and execution.".to_string(),
                "Improved mode UX and discoverability across TUI/TUI2.".to_string(),
                "Durable plan-file CLI commands are available (`xcodex plan status|list|open|done|archive`)."
                    .to_string(),
                "Read more: docs/xcodex/releases/0.4.0.md".to_string(),
            ]
        );
    }

    #[test]
    fn returns_none_for_unknown_version() {
        assert_eq!(whats_new_bullets_for_version("9.9.9"), None);
    }
}
