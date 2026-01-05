use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Stdio;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use codex_common::CliConfigOverrides;
use codex_core::config::CONFIG_TOML_FILE;
use codex_core::config::ConfigToml;
use codex_core::config::find_codex_home;
use codex_core::config::is_xcodex_invocation;
use codex_core::config_loader::ConfigLayerEntry;
use codex_core::config_loader::ConfigLayerStackOrdering;
use codex_core::config_loader::LoaderOverrides;
use codex_core::config_loader::load_config_layers_state;
use codex_utils_absolute_path::AbsolutePathBuf;
use tokio::process::Command;
use toml::Value as TomlValue;

#[derive(Debug, Parser)]
pub struct ConfigCli {
    #[clap(flatten)]
    pub config_overrides: CliConfigOverrides,

    #[command(subcommand)]
    pub subcommand: ConfigSubcommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum ConfigSubcommand {
    /// Print the resolved config paths and layer precedence.
    Path,
    /// Open the user config file in $VISUAL/$EDITOR (fallback: print the path).
    Edit(EditArgs),
    /// Validate config parsing and print common issues.
    Doctor,
}

#[derive(Debug, Parser)]
pub struct EditArgs {
    /// Edit the project-local config (`./.codex/config.toml`) instead of `$CODEX_HOME/config.toml`.
    #[arg(long, default_value_t = false)]
    project: bool,
}

impl ConfigCli {
    pub async fn run(self) -> Result<()> {
        match self.subcommand {
            ConfigSubcommand::Path => run_config_path(self.config_overrides).await,
            ConfigSubcommand::Edit(args) => run_config_edit(args).await,
            ConfigSubcommand::Doctor => run_config_doctor(self.config_overrides).await,
        }
    }
}

async fn run_config_path(config_overrides: CliConfigOverrides) -> Result<()> {
    let codex_home = find_codex_home()?;
    let resolved_cwd = AbsolutePathBuf::current_dir()?;
    let cli_overrides = config_overrides
        .parse_overrides()
        .map_err(|e| anyhow::anyhow!(e))?;

    let layers = load_config_layers_state(
        &codex_home,
        Some(resolved_cwd),
        &cli_overrides,
        LoaderOverrides::default(),
    )
    .await?;

    println!("CODEX_HOME: {}", codex_home.display());
    println!(
        "User config: {}",
        codex_home.join(CONFIG_TOML_FILE).display()
    );

    println!();
    println!("Layers (highest precedence first):");
    for layer in layers.get_layers(ConfigLayerStackOrdering::HighestPrecedenceFirst) {
        println!("- {}", format_layer_path(layer));
    }

    Ok(())
}

async fn run_config_edit(args: EditArgs) -> Result<()> {
    let codex_home = find_codex_home()?;
    let config_path = if args.project {
        project_config_path()?
    } else {
        codex_home.join(CONFIG_TOML_FILE)
    };
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    if !config_path.exists() {
        fs::write(&config_path, "")
            .with_context(|| format!("failed to create {}", config_path.display()))?;
    }

    let editor_cmd = match resolve_editor_command() {
        Ok(editor_cmd) => editor_cmd,
        Err(message) => {
            let exe = command_name();
            println!("Config file: {}", config_path.display());
            println!("Cannot open editor: {message}");
            println!("Set $VISUAL or $EDITOR and re-run `{exe} config edit`.");
            return Ok(());
        }
    };

    let mut cmd = build_editor_command(&editor_cmd);
    let status = cmd
        .arg(&config_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;
    if !status.success() {
        anyhow::bail!("editor exited with status {status}");
    }
    Ok(())
}

async fn run_config_doctor(config_overrides: CliConfigOverrides) -> Result<()> {
    let codex_home = find_codex_home()?;
    let resolved_cwd = AbsolutePathBuf::current_dir()?;
    let cli_overrides = config_overrides
        .parse_overrides()
        .map_err(|e| anyhow::anyhow!(e))?;
    let exe = command_name();

    let user_config_path = codex_home.join(CONFIG_TOML_FILE);
    if !codex_home.exists() {
        println!(
            "Warning: CODEX_HOME directory does not exist: {}",
            codex_home.display()
        );
        println!(
            "Next step: run `{exe} config edit` to create it (or set $CODEX_HOME to an existing directory)."
        );
    }
    if !user_config_path.exists() {
        println!(
            "Warning: user config does not exist: {} (run `{exe} config edit` to create it)",
            user_config_path.display()
        );
        println!("Next step: run `{exe} config edit` to create it.");
    }

    let mut had_warnings = false;
    let known_paths = known_key_paths()?;

    let mut user_leaf_paths = None;
    for layer in discover_layer_files(codex_home.as_path(), &resolved_cwd) {
        if !layer.path.exists() {
            if layer.is_optional {
                println!(
                    "Note: {} has no {CONFIG_TOML_FILE}: {}",
                    layer.label,
                    layer.path.display()
                );
            }
            continue;
        }

        let contents = match tokio::fs::read_to_string(&layer.path).await {
            Ok(contents) => contents,
            Err(err) => {
                had_warnings = true;
                println!(
                    "Error: failed to read {} {}: {err}",
                    layer.label,
                    layer.path.display()
                );
                println!(
                    "Next step: check file permissions for {}, then re-run `{exe} config doctor`.",
                    layer.path.display()
                );
                continue;
            }
        };

        let parsed: TomlValue = match toml::from_str(&contents) {
            Ok(parsed) => parsed,
            Err(err) => {
                had_warnings = true;
                println!(
                    "Error: failed to parse {} {}: {err}",
                    layer.label,
                    layer.path.display()
                );
                if let Some(edit_cmd) = edit_command_for_layer(exe, layer.kind) {
                    println!(
                        "Next step: fix TOML syntax in {} (open via `{edit_cmd}`), then re-run `{exe} config doctor`.",
                        layer.path.display()
                    );
                } else {
                    println!(
                        "Next step: fix TOML syntax in {} (system-managed; edit the file directly), then re-run `{exe} config doctor`.",
                        layer.path.display()
                    );
                }
                continue;
            }
        };

        if layer.kind == LayerKind::User {
            user_leaf_paths = Some(collect_leaf_paths(&parsed));
        }

        let unknown = unknown_key_paths(&parsed, &known_paths)?;
        if !unknown.is_empty() {
            had_warnings = true;
            println!(
                "Warning: unknown keys in {} {}:",
                layer.label,
                layer.path.display()
            );
            for key in unknown {
                if let Some(suggestion) = best_key_suggestion(&key, &known_paths) {
                    println!("  - {key} (did you mean `{suggestion}`?)");
                } else {
                    println!("  - {key}");
                }
            }
            if let Some(edit_cmd) = edit_command_for_layer(exe, layer.kind) {
                println!(
                    "Next step: remove/rename the unknown keys in {} (open via `{edit_cmd}`).",
                    layer.path.display()
                );
            } else {
                println!(
                    "Next step: remove/rename the unknown keys in {} (system-managed; edit the file directly).",
                    layer.path.display()
                );
            }
        }
    }

    match load_config_layers_state(
        &codex_home,
        Some(resolved_cwd),
        &cli_overrides,
        LoaderOverrides::default(),
    )
    .await
    {
        Ok(layers) => {
            let effective = layers.effective_config();
            let effective_cfg: ConfigToml = effective
                .try_into()
                .context("failed to parse merged effective config")?;
            if let Err(err) = effective_cfg.get_config_profile(None) {
                had_warnings = true;
                println!("Warning: {err}");
                println!(
                    "Next step: remove `profile = ...` from your config, or define it under `[profiles.<name>]` (edit via `{exe} config edit`)."
                );
            }

            if let Some(paths) = user_leaf_paths {
                report_overrides(&layers, &paths, exe);
            }
        }
        Err(err) => {
            had_warnings = true;
            println!("Error: failed to load merged configuration: {err}");
            println!(
                "Next step: fix config file syntax/errors above, then re-run `{exe} config doctor`."
            );
        }
    }

    if !had_warnings {
        println!("OK: config loads successfully.");
    }

    Ok(())
}

fn format_layer_path(layer: &ConfigLayerEntry) -> String {
    use codex_app_server_protocol::ConfigLayerSource;
    match &layer.name {
        ConfigLayerSource::Mdm { domain, key } => format!("MDM ({domain}): {key}"),
        ConfigLayerSource::System { file } => format!("system: {}", file.display()),
        ConfigLayerSource::User { file } => format!("user: {}", file.display()),
        ConfigLayerSource::Project { dot_codex_folder } => format!(
            "project: {}/{}",
            dot_codex_folder.display(),
            CONFIG_TOML_FILE
        ),
        ConfigLayerSource::SessionFlags => "session flags (-c overrides)".to_string(),
        ConfigLayerSource::LegacyManagedConfigTomlFromFile { file } => {
            format!("managed (legacy): {}", file.display())
        }
        ConfigLayerSource::LegacyManagedConfigTomlFromMdm => "managed (legacy): MDM".to_string(),
    }
}

fn command_name() -> &'static str {
    if is_xcodex_invocation() {
        "xcodex"
    } else {
        "codex"
    }
}

fn unknown_key_paths(root: &TomlValue, known_paths: &BTreeSet<String>) -> Result<Vec<String>> {
    let cfg: ConfigToml = root
        .clone()
        .try_into()
        .context("failed to parse config for unknown-key detection")?;
    let canonical_str = toml::to_string(&cfg).context("failed to serialize known config keys")?;
    let canonical: TomlValue =
        toml::from_str(&canonical_str).context("failed to deserialize known config keys")?;

    let original_paths = collect_leaf_paths(root);
    let known_from_cfg = collect_leaf_paths(&canonical)
        .intersection(known_paths)
        .cloned()
        .collect::<BTreeSet<_>>();

    Ok(original_paths
        .difference(&known_from_cfg)
        .cloned()
        .collect::<Vec<_>>())
}

fn collect_leaf_paths(root: &TomlValue) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect_leaf_paths_inner(root, &mut Vec::new(), &mut out);
    out
}

fn collect_leaf_paths_inner(
    root: &TomlValue,
    prefix: &mut Vec<String>,
    out: &mut BTreeSet<String>,
) {
    match root {
        TomlValue::Table(table) => {
            for (key, value) in table {
                prefix.push(key.clone());
                collect_leaf_paths_inner(value, prefix, out);
                prefix.pop();
            }
        }
        TomlValue::Array(values) => {
            for (idx, value) in values.iter().enumerate() {
                prefix.push(idx.to_string());
                collect_leaf_paths_inner(value, prefix, out);
                prefix.pop();
            }
        }
        _ => {
            if !prefix.is_empty() {
                out.insert(prefix.join("."));
            }
        }
    }
}

fn resolve_editor_command() -> std::result::Result<Vec<String>, &'static str> {
    let raw = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .map_err(|_| "neither VISUAL nor EDITOR is set")?;

    let parts = {
        #[cfg(target_os = "windows")]
        {
            winsplit::split(&raw)
        }
        #[cfg(not(target_os = "windows"))]
        {
            shlex::split(&raw).ok_or("failed to parse editor command")?
        }
    };

    if parts.is_empty() {
        return Err("editor command is empty");
    }

    Ok(parts)
}

fn known_key_paths() -> Result<BTreeSet<String>> {
    let cfg = ConfigToml::default();
    let canonical_str = toml::to_string(&cfg).context("failed to serialize known config keys")?;
    let canonical: TomlValue =
        toml::from_str(&canonical_str).context("failed to deserialize known config keys")?;
    Ok(collect_leaf_paths(&canonical))
}

fn best_key_suggestion(unknown: &str, known_paths: &BTreeSet<String>) -> Option<String> {
    let unknown = unknown.trim();
    if unknown.is_empty() {
        return None;
    }

    let mut best: Option<(&str, usize)> = None;
    for candidate in known_paths {
        let dist = levenshtein(unknown, candidate);
        best = match best {
            Some((_, best_dist)) if dist >= best_dist => best,
            _ => Some((candidate.as_str(), dist)),
        };
    }

    let (candidate, dist) = best?;
    let max_len = unknown.len().max(candidate.len());
    if max_len == 0 {
        return None;
    }
    let score = 1.0 - (dist as f64 / max_len as f64);
    (score >= 0.72).then(|| candidate.to_string())
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr: Vec<usize> = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        prev.clone_from_slice(&curr);
    }
    prev[b.len()]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayerKind {
    User,
    Project,
    System,
}

#[derive(Debug)]
struct LayerFile {
    kind: LayerKind,
    label: &'static str,
    path: PathBuf,
    is_optional: bool,
}

fn discover_layer_files(codex_home: &std::path::Path, cwd: &AbsolutePathBuf) -> Vec<LayerFile> {
    let mut layers = Vec::new();
    layers.push(LayerFile {
        kind: LayerKind::User,
        label: "user config",
        path: codex_home.join(CONFIG_TOML_FILE),
        is_optional: false,
    });

    let project_root = find_project_root(cwd.as_path());
    for ancestor in cwd
        .as_path()
        .ancestors()
        .take_while(|a| a.starts_with(&project_root))
    {
        let dot_codex = ancestor.join(".codex");
        if dot_codex.is_dir() {
            layers.push(LayerFile {
                kind: LayerKind::Project,
                label: "project config",
                path: dot_codex.join(CONFIG_TOML_FILE),
                is_optional: true,
            });
        }
    }

    #[cfg(unix)]
    {
        layers.push(LayerFile {
            kind: LayerKind::System,
            label: "system config",
            path: PathBuf::from(codex_core::config_loader::SYSTEM_CONFIG_TOML_FILE_UNIX),
            is_optional: true,
        });
    }

    layers
}

fn find_project_root(cwd: &std::path::Path) -> PathBuf {
    for ancestor in cwd.ancestors() {
        if ancestor.join(".git").exists() {
            return ancestor.to_path_buf();
        }
    }
    cwd.to_path_buf()
}

fn project_config_path() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to resolve current directory")?;
    let project_root = find_project_root(&cwd);

    for ancestor in cwd.ancestors() {
        if !ancestor.starts_with(&project_root) {
            break;
        }
        let dot_codex = ancestor.join(".codex");
        if dot_codex.is_dir() {
            return Ok(dot_codex.join(CONFIG_TOML_FILE));
        }
    }

    Ok(project_root.join(".codex").join(CONFIG_TOML_FILE))
}

fn report_overrides(
    layers: &codex_core::config_loader::ConfigLayerStack,
    user_paths: &BTreeSet<String>,
    exe: &str,
) {
    let origins = layers.origins();
    let mut overridden = Vec::new();

    for key in user_paths {
        let Some(meta) = origins.get(key) else {
            continue;
        };
        if matches!(
            meta.name,
            codex_app_server_protocol::ConfigLayerSource::User { .. }
        ) {
            continue;
        }
        overridden.push((key.clone(), meta.name.clone()));
    }

    if overridden.is_empty() {
        return;
    }

    let max = 20;
    println!();
    println!("Note: some user config values are overridden:");
    let mut has_project = false;
    let mut has_session_flags = false;
    let mut has_managed = false;
    for (idx, (key, source)) in overridden.iter().take(max).enumerate() {
        let _ = idx;
        println!("  - {key} overridden by {}", describe_layer_source(source));
        has_project |= matches!(
            source,
            codex_app_server_protocol::ConfigLayerSource::Project { .. }
        );
        has_session_flags |= matches!(
            source,
            codex_app_server_protocol::ConfigLayerSource::SessionFlags
        );
        has_managed |= matches!(
            source,
            codex_app_server_protocol::ConfigLayerSource::System { .. }
                | codex_app_server_protocol::ConfigLayerSource::Mdm { .. }
                | codex_app_server_protocol::ConfigLayerSource::LegacyManagedConfigTomlFromFile { .. }
                | codex_app_server_protocol::ConfigLayerSource::LegacyManagedConfigTomlFromMdm
        );
    }
    if overridden.len() > max {
        println!("  - … and {} more", overridden.len() - max);
    }

    println!();
    println!(
        "Next step: update the overriding layer (or remove the key from your user config) and re-run `{exe} config doctor`."
    );
    if has_project {
        println!(
            "  - Project overrides: edit via `{exe} config edit --project` (or the project path shown above)."
        );
    }
    if has_session_flags {
        println!(
            "  - Session flag overrides: remove conflicting `-c ...` CLI overrides from your invocation."
        );
    }
    if has_managed {
        println!(
            "  - System/MDM overrides: these are managed; contact your admin or update the managed config."
        );
    }
}

fn describe_layer_source(source: &codex_app_server_protocol::ConfigLayerSource) -> String {
    use codex_app_server_protocol::ConfigLayerSource;
    match source {
        ConfigLayerSource::Mdm { domain, key } => format!("MDM ({domain}): {key}"),
        ConfigLayerSource::System { file } => format!("system: {}", file.display()),
        ConfigLayerSource::User { file } => format!("user: {}", file.display()),
        ConfigLayerSource::Project { dot_codex_folder } => format!(
            "project: {}/{}",
            dot_codex_folder.display(),
            CONFIG_TOML_FILE
        ),
        ConfigLayerSource::SessionFlags => "session flags (-c overrides)".to_string(),
        ConfigLayerSource::LegacyManagedConfigTomlFromFile { file } => {
            format!("managed (legacy): {}", file.display())
        }
        ConfigLayerSource::LegacyManagedConfigTomlFromMdm => "managed (legacy): MDM".to_string(),
    }
}

#[cfg(target_os = "windows")]
fn build_editor_command(editor_cmd: &[String]) -> Command {
    let resolved = which::which(&editor_cmd[0]).unwrap_or_else(|_| editor_cmd[0].clone().into());
    let mut cmd = Command::new(resolved);
    if editor_cmd.len() > 1 {
        cmd.args(&editor_cmd[1..]);
    }
    cmd
}

#[cfg(not(target_os = "windows"))]
fn build_editor_command(editor_cmd: &[String]) -> Command {
    let mut cmd = Command::new(&editor_cmd[0]);
    if editor_cmd.len() > 1 {
        cmd.args(&editor_cmd[1..]);
    }
    cmd
}

fn edit_command_for_layer(exe: &str, kind: LayerKind) -> Option<String> {
    match kind {
        LayerKind::Project => Some(format!("{exe} config edit --project")),
        LayerKind::User => Some(format!("{exe} config edit")),
        LayerKind::System => None,
    }
}
