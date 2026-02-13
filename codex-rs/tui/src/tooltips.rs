use codex_core::features::FEATURES;
use codex_protocol::account::PlanType;
use lazy_static::lazy_static;
use rand::Rng;

const ANNOUNCEMENT_TIP_URL: &str =
    "https://raw.githubusercontent.com/Eriz1818/xCodex/main/announcement_tip.toml";
const RAW_CODEX_TOOLTIPS: &str = include_str!("../tooltips.txt");
const RAW_XCODEX_TOOLTIPS: &str = include_str!("../tooltips_xcodex.txt");

const IS_MACOS: bool = cfg!(target_os = "macos");

const PAID_TOOLTIP: &str = "*New* Try the **Codex App** with 2x rate limits until *April 2nd*. Run 'codex app' or visit https://chatgpt.com/codex?app-landing-page=true";
const PAID_TOOLTIP_NON_MAC: &str = "*New* 2x rate limits until *April 2nd*.";
const OTHER_TOOLTIP: &str = "*New* Build faster with the **Codex App**. Run 'codex app' or visit https://chatgpt.com/codex?app-landing-page=true";
const OTHER_TOOLTIP_NON_MAC: &str = "*New* Build faster with Codex.";
const FREE_GO_TOOLTIP: &str =
    "*New* Codex is included in your plan for free through *March 2nd* – let’s build together.";

fn normalize_tip_text(tip: &str) -> String {
    let tip = tip.trim();
    let tip = tip
        .strip_prefix("⚡Tips:")
        .or_else(|| tip.strip_prefix("Tips:"))
        .or_else(|| tip.strip_prefix("Tip:"))
        .unwrap_or(tip)
        .trim_start();
    tip.to_string()
}

fn parse_tooltips(raw: &'static str) -> Vec<&'static str> {
    raw.lines()
        .map(str::trim)
        .filter(|line| {
            if line.is_empty() || line.starts_with('#') {
                return false;
            }
            if !IS_MACOS && line.contains("codex app") {
                return false;
            }
            true
        })
        .collect()
}

lazy_static! {
    static ref CODEX_TOOLTIPS: Vec<&'static str> = parse_tooltips(RAW_CODEX_TOOLTIPS);
    static ref XCODEX_TOOLTIPS: Vec<&'static str> = parse_tooltips(RAW_XCODEX_TOOLTIPS);
    static ref ALL_CODEX_TOOLTIPS: Vec<&'static str> = {
        let mut tips = Vec::new();
        tips.extend(CODEX_TOOLTIPS.iter().copied());
        tips.extend(experimental_tooltips());
        tips
    };
}

fn experimental_tooltips() -> Vec<&'static str> {
    FEATURES
        .iter()
        .filter_map(|spec| spec.stage.experimental_announcement())
        .collect()
}

/// Pick a random tooltip to show to the user when starting Codex.
pub(crate) fn random_tooltip() -> Option<String> {
    let mut rng = rand::rng();
    pick_tooltip(&mut rng, ALL_CODEX_TOOLTIPS.as_slice()).map(str::to_string)
}

pub(crate) fn get_tooltip(plan: Option<PlanType>) -> Option<String> {
    let mut rng = rand::rng();

    // Leave small chance for a random tooltip to be shown.
    if rng.random_ratio(8, 10) {
        match plan {
            Some(PlanType::Plus)
            | Some(PlanType::Business)
            | Some(PlanType::Team)
            | Some(PlanType::Enterprise)
            | Some(PlanType::Pro) => {
                let tooltip = if IS_MACOS {
                    PAID_TOOLTIP
                } else {
                    PAID_TOOLTIP_NON_MAC
                };
                return Some(tooltip.to_string());
            }
            Some(PlanType::Go) | Some(PlanType::Free) => {
                return Some(FREE_GO_TOOLTIP.to_string());
            }
            _ => {
                let tooltip = if IS_MACOS {
                    OTHER_TOOLTIP
                } else {
                    OTHER_TOOLTIP_NON_MAC
                };
                return Some(tooltip.to_string());
            }
        }
    }

    pick_tooltip(&mut rng, ALL_CODEX_TOOLTIPS.as_slice()).map(str::to_string)
}

pub(crate) fn random_xcodex_tooltip() -> Option<String> {
    let mut rng = rand::rng();
    let announcement = announcement::fetch_xcodex_announcement_tip()
        .map(|announcement| normalize_tip_text(&announcement));
    pick_xcodex_tooltip(
        &mut rng,
        XCODEX_TOOLTIPS.as_slice(),
        announcement.as_deref(),
    )
}

fn pick_xcodex_tooltip<R: Rng + ?Sized>(
    rng: &mut R,
    tooltips: &[&'static str],
    announcement: Option<&str>,
) -> Option<String> {
    let total = tooltips.len() + usize::from(announcement.is_some());
    if total == 0 {
        return None;
    }

    let idx = rng.random_range(0..total);
    if idx < tooltips.len() {
        Some(tooltips[idx].to_string())
    } else {
        announcement.map(str::to_string)
    }
}

fn pick_tooltip<R: Rng + ?Sized>(rng: &mut R, tooltips: &[&'static str]) -> Option<&'static str> {
    if tooltips.is_empty() {
        None
    } else {
        tooltips.get(rng.random_range(0..tooltips.len())).copied()
    }
}

pub(crate) mod announcement {
    use crate::tooltips::ANNOUNCEMENT_TIP_URL;
    use crate::version::CODEX_CLI_VERSION;
    use chrono::NaiveDate;
    use chrono::Utc;
    use regex_lite::Regex;
    use serde::Deserialize;
    use std::sync::OnceLock;
    use std::thread;
    use std::time::Duration;

    static ANNOUNCEMENT_TIP: OnceLock<Option<String>> = OnceLock::new();

    /// Prewarm the cache of the announcement tip.
    pub(crate) fn prewarm() {
        let _ = thread::spawn(|| ANNOUNCEMENT_TIP.get_or_init(init_announcement_tip_in_thread));
    }

    pub(crate) fn fetch_xcodex_announcement_tip() -> Option<String> {
        ANNOUNCEMENT_TIP
            .get()
            .cloned()
            .flatten()
            .and_then(|raw| parse_announcement_tip_toml_for_target(&raw, "xcodex"))
    }

    #[derive(Debug, Deserialize)]
    struct AnnouncementTipRaw {
        content: String,
        from_date: Option<String>,
        to_date: Option<String>,
        version_regex: Option<String>,
        target_app: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct AnnouncementTipDocument {
        announcements: Vec<AnnouncementTipRaw>,
    }

    #[derive(Debug)]
    struct AnnouncementTip {
        content: String,
        from_date: Option<NaiveDate>,
        to_date: Option<NaiveDate>,
        version_regex: Option<Regex>,
        target_app: String,
    }

    fn init_announcement_tip_in_thread() -> Option<String> {
        thread::spawn(blocking_init_announcement_tip)
            .join()
            .ok()
            .flatten()
    }

    fn blocking_init_announcement_tip() -> Option<String> {
        // Avoid system proxy detection to prevent macOS system-configuration panics (#8912).
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .ok()?;
        let response = client
            .get(ANNOUNCEMENT_TIP_URL)
            .timeout(Duration::from_millis(2000))
            .send()
            .ok()?;
        response.error_for_status().ok()?.text().ok()
    }

    #[cfg(test)]
    pub(crate) fn parse_announcement_tip_toml(text: &str) -> Option<String> {
        parse_announcement_tip_toml_for_target(text, "cli")
    }

    pub(crate) fn parse_announcement_tip_toml_for_target(
        text: &str,
        target_app: &str,
    ) -> Option<String> {
        let announcements = toml::from_str::<AnnouncementTipDocument>(text)
            .map(|doc| doc.announcements)
            .or_else(|_| toml::from_str::<Vec<AnnouncementTipRaw>>(text))
            .ok()?;

        let mut latest_match = None;
        let today = Utc::now().date_naive();
        for raw in announcements {
            let Some(tip) = AnnouncementTip::from_raw(raw) else {
                continue;
            };
            if tip.version_matches(CODEX_CLI_VERSION)
                && tip.date_matches(today)
                && tip.target_app == target_app
            {
                latest_match = Some(tip.content);
            }
        }
        latest_match
    }

    impl AnnouncementTip {
        fn from_raw(raw: AnnouncementTipRaw) -> Option<Self> {
            let content = raw.content.trim();
            if content.is_empty() {
                return None;
            }

            let from_date = match raw.from_date {
                Some(date) => Some(NaiveDate::parse_from_str(&date, "%Y-%m-%d").ok()?),
                None => None,
            };
            let to_date = match raw.to_date {
                Some(date) => Some(NaiveDate::parse_from_str(&date, "%Y-%m-%d").ok()?),
                None => None,
            };
            let version_regex = match raw.version_regex {
                Some(pattern) => Some(Regex::new(&pattern).ok()?),
                None => None,
            };

            Some(Self {
                content: content.to_string(),
                from_date,
                to_date,
                version_regex,
                target_app: raw.target_app.unwrap_or("cli".to_string()).to_lowercase(),
            })
        }

        fn version_matches(&self, version: &str) -> bool {
            self.version_regex
                .as_ref()
                .is_none_or(|regex| regex.is_match(version))
        }

        fn date_matches(&self, today: NaiveDate) -> bool {
            if let Some(from) = self.from_date
                && today < from
            {
                return false;
            }
            if let Some(to) = self.to_date
                && today >= to
            {
                return false;
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tooltips::announcement::parse_announcement_tip_toml;
    use crate::tooltips::announcement::parse_announcement_tip_toml_for_target;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn random_tooltip_returns_some_tip_when_available() {
        let mut rng = StdRng::seed_from_u64(42);
        assert!(pick_tooltip(&mut rng, ALL_CODEX_TOOLTIPS.as_slice()).is_some());
    }

    #[test]
    fn random_tooltip_is_reproducible_with_seed() {
        let expected = {
            let mut rng = StdRng::seed_from_u64(7);
            pick_tooltip(&mut rng, ALL_CODEX_TOOLTIPS.as_slice())
        };

        let mut rng = StdRng::seed_from_u64(7);
        assert_eq!(
            expected,
            pick_tooltip(&mut rng, ALL_CODEX_TOOLTIPS.as_slice())
        );
    }

    #[test]
    fn random_xcodex_tooltip_returns_some_tip_when_available() {
        let mut rng = StdRng::seed_from_u64(42);
        assert!(pick_tooltip(&mut rng, XCODEX_TOOLTIPS.as_slice()).is_some());
    }

    #[test]
    fn random_xcodex_tooltip_rotates_with_announcement() {
        let mut saw_local_tip = false;
        let mut saw_announcement = false;

        for seed in 0..64 {
            let mut rng = StdRng::seed_from_u64(seed);
            match pick_xcodex_tooltip(&mut rng, &["local tip"], Some("xcodex announcement tip")) {
                Some(tip) if tip == "local tip" => saw_local_tip = true,
                Some(tip) if tip == "xcodex announcement tip" => saw_announcement = true,
                Some(other) => panic!("unexpected tooltip selected: {other}"),
                None => panic!("expected a tooltip to be selected"),
            }
        }

        assert!(saw_local_tip);
        assert!(saw_announcement);
    }

    #[test]
    fn announcement_tip_toml_picks_last_matching() {
        let toml = r#"
[[announcements]]
content = "first"
from_date = "2000-01-01"

[[announcements]]
content = "latest match"
version_regex = ".*"
target_app = "cli"

[[announcements]]
content = "should not match"
to_date = "2000-01-01"
        "#;

        assert_eq!(
            Some("latest match".to_string()),
            parse_announcement_tip_toml(toml)
        );

        let toml = r#"
[[announcements]]
content = "first"
from_date = "2000-01-01"
target_app = "cli"

[[announcements]]
content = "latest match"
version_regex = ".*"

[[announcements]]
content = "should not match"
to_date = "2000-01-01"
        "#;

        assert_eq!(
            Some("latest match".to_string()),
            parse_announcement_tip_toml(toml)
        );
    }

    #[test]
    fn announcement_tip_toml_picks_no_match() {
        let toml = r#"
[[announcements]]
content = "first"
from_date = "2000-01-01"
to_date = "2000-01-05"

[[announcements]]
content = "latest match"
version_regex = "invalid_version_name"

[[announcements]]
content = "should not match either "
target_app = "vsce"
        "#;

        assert_eq!(None, parse_announcement_tip_toml(toml));
    }

    #[test]
    fn announcement_tip_toml_target_app_filters_cli_vs_xcodex() {
        let toml = r#"
[[announcements]]
content = "codex cli announcement"
target_app = "cli"

[[announcements]]
content = "xcodex announcement"
target_app = "xcodex"
        "#;

        assert_eq!(
            Some("codex cli announcement".to_string()),
            parse_announcement_tip_toml_for_target(toml, "cli")
        );
        assert_eq!(
            Some("xcodex announcement".to_string()),
            parse_announcement_tip_toml_for_target(toml, "xcodex")
        );
    }

    #[test]
    fn announcement_tip_toml_bad_deserialization() {
        let toml = r#"
[[announcements]]
content = 123
from_date = "2000-01-01"
        "#;

        assert_eq!(None, parse_announcement_tip_toml(toml));
    }

    #[test]
    fn announcement_tip_toml_parse_comments() {
        let toml = r#"
# Example announcement tips for Codex TUI.
# Each [[announcements]] entry is evaluated in order; the last matching one is shown.
# Dates are UTC, formatted as YYYY-MM-DD. The from_date is inclusive and the to_date is exclusive.
# version_regex matches against the CLI version (env!("CARGO_PKG_VERSION")); omit to apply to all versions.
# target_app specify which app should display the announcement (cli, vsce, ...).

[[announcements]]
content = "Welcome to Codex! Check out the new onboarding flow."
from_date = "2024-10-01"
to_date = "2024-10-15"
target_app = "cli"
version_regex = "^0\\.0\\.0$"

[[announcements]]
content = "This is a test announcement"
        "#;

        assert_eq!(
            Some("This is a test announcement".to_string()),
            parse_announcement_tip_toml(toml)
        );
    }
}
