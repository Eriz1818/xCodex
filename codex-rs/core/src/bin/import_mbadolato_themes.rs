use anyhow::Context;
use codex_core::themes::ThemeCatalog;
use codex_core::themes::ThemeColor;
use codex_core::themes::ThemeDefinition;
use codex_core::themes::ThemePalette;
use codex_core::themes::ThemeRoles;
use codex_core::themes::ThemeVariant;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct WeztermTheme {
    colors: WeztermColors,
}

#[derive(Debug, Deserialize)]
struct WeztermColors {
    foreground: String,
    background: String,
    cursor_bg: String,
    cursor_fg: String,
    selection_bg: String,
    selection_fg: String,
    ansi: Vec<String>,
    brights: Vec<String>,
}

fn normalize_name(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in name.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            prev_dash = false;
            continue;
        }
        if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "theme".to_string()
    } else {
        out
    }
}

fn unique_name(names: &mut BTreeMap<String, usize>, base: String) -> String {
    let count = names.entry(base.clone()).or_insert(0);
    if *count == 0 {
        *count = 1;
        return base;
    }
    *count += 1;
    format!("{base}-{count}")
}

fn parse_wezterm_theme(path: &Path) -> anyhow::Result<WeztermTheme> {
    let src = fs::read_to_string(path).with_context(|| format!("read {path:?}"))?;
    toml::from_str(&src).with_context(|| format!("parse toml {path:?}"))
}

fn to_theme_definition(name: String, wez: WeztermTheme) -> anyhow::Result<ThemeDefinition> {
    let colors = wez.colors;
    let ansi_len = colors.ansi.len();
    anyhow::ensure!(
        ansi_len == 8,
        "theme {name}: expected 8 ansi colors, got {ansi_len}"
    );
    let brights_len = colors.brights.len();
    anyhow::ensure!(
        brights_len == 8,
        "theme {name}: expected 8 bright colors, got {brights_len}"
    );

    let palette = ThemePalette {
        black: ThemeColor::new(colors.ansi[0].clone()),
        red: ThemeColor::new(colors.ansi[1].clone()),
        green: ThemeColor::new(colors.ansi[2].clone()),
        yellow: ThemeColor::new(colors.ansi[3].clone()),
        blue: ThemeColor::new(colors.ansi[4].clone()),
        magenta: ThemeColor::new(colors.ansi[5].clone()),
        cyan: ThemeColor::new(colors.ansi[6].clone()),
        white: ThemeColor::new(colors.ansi[7].clone()),
        bright_black: ThemeColor::new(colors.brights[0].clone()),
        bright_red: ThemeColor::new(colors.brights[1].clone()),
        bright_green: ThemeColor::new(colors.brights[2].clone()),
        bright_yellow: ThemeColor::new(colors.brights[3].clone()),
        bright_blue: ThemeColor::new(colors.brights[4].clone()),
        bright_magenta: ThemeColor::new(colors.brights[5].clone()),
        bright_cyan: ThemeColor::new(colors.brights[6].clone()),
        bright_white: ThemeColor::new(colors.brights[7].clone()),
    };

    let roles = ThemeRoles {
        fg: ThemeColor::new(colors.foreground),
        bg: ThemeColor::new(colors.background),
        transcript_bg: None,
        composer_bg: None,
        user_prompt_highlight_bg: None,
        status_bg: None,
        status_ramp_fg: None,
        status_ramp_highlight: None,
        selection_fg: ThemeColor::new(colors.selection_fg),
        selection_bg: ThemeColor::new(colors.selection_bg),
        cursor_fg: ThemeColor::new(colors.cursor_fg),
        cursor_bg: ThemeColor::new(colors.cursor_bg),
        border: ThemeColor::new("palette.bright_black"),
        accent: ThemeColor::new("palette.blue"),
        brand: ThemeColor::new("palette.magenta"),
        command: ThemeColor::new("palette.magenta"),
        success: ThemeColor::new("palette.green"),
        warning: ThemeColor::new("palette.yellow"),
        error: ThemeColor::new("palette.red"),
        diff_add_fg: ThemeColor::new("palette.green"),
        diff_add_bg: ThemeColor::inherit(),
        diff_del_fg: ThemeColor::new("palette.red"),
        diff_del_bg: ThemeColor::inherit(),
        diff_hunk_fg: ThemeColor::new("palette.cyan"),
        diff_hunk_bg: ThemeColor::inherit(),
        badge: Some(ThemeColor::new("palette.bright_blue")),
        link: Some(ThemeColor::new("palette.blue")),
    };

    let bg = roles.bg.resolve(&palette).context("resolve roles.bg")?;
    let variant = ThemeVariant::from_background(bg);

    let theme = ThemeDefinition {
        name,
        variant,
        palette,
        roles,
    };

    theme.validate().context("validate ThemeDefinition")?;
    Ok(theme)
}

fn main() -> anyhow::Result<()> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        anyhow::bail!("usage: import_mbadolato_themes <wezterm_dir> <output_json_path>");
    }

    let wezterm_dir = PathBuf::from(args.remove(0));
    let output_path = PathBuf::from(args.remove(0));

    let mut entries = fs::read_dir(&wezterm_dir)
        .with_context(|| format!("read dir {wezterm_dir:?}"))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("collect dir entries {wezterm_dir:?}"))?;
    entries.sort_by_key(std::fs::DirEntry::path);

    let mut used_names: BTreeMap<String, usize> = BTreeMap::new();
    let mut themes = Vec::new();

    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .context("missing file stem")?;
        let normalized = normalize_name(stem);
        let name = unique_name(&mut used_names, normalized);

        let wez = parse_wezterm_theme(&path)?;
        let theme = to_theme_definition(name, wez).with_context(|| format!("convert {path:?}"))?;
        themes.push(theme);
    }

    themes.sort_by(|a, b| a.name.cmp(&b.name));

    fs::create_dir_all(output_path.parent().context("output path missing parent")?)
        .with_context(|| format!("mkdir {}", output_path.display()))?;

    let bytes = serde_json::to_vec(&themes).context("serialize themes json")?;
    fs::write(&output_path, bytes).with_context(|| format!("write {}", output_path.display()))?;

    eprintln!(
        "wrote {} themes to {} (default built-in catalog currently contains {} themes)",
        themes.len(),
        output_path.display(),
        ThemeCatalog::built_in_themes().len()
    );

    Ok(())
}
