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
        let bullets = whats_new_bullets_for_version("0.3.5").expect("expected bullets");
        assert_eq!(
            bullets,
            vec![
                "Syntax highlighting for: Bash (bash/sh/zsh), C, C++, CSS, Go, HTML, Java, JavaScript, JSON, Python, Ruby, Rust, TypeScript, YAML (see `docs/xcodex/themes.md`)".to_string(),
                "Resume/startup responsiveness fixes for faster session loading".to_string(),
                "Lazy MCP loading to avoid pulling servers until needed (see `docs/xcodex/lazy-mcp-loading.md`)".to_string(),
                "Internal code restructure for stability and maintainability".to_string(),
                "Read more: docs/xcodex/releases/0.3.5.md".to_string(),
            ]
        );
    }

    #[test]
    fn returns_none_for_unknown_version() {
        assert_eq!(whats_new_bullets_for_version("9.9.9"), None);
    }
}
