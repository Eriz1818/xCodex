use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use crate::config::Config;
use crate::default_client::build_reqwest_client;

const REMOTE_SKILLS_API_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSkillSummary {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSkillDownloadResult {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct RemoteSkillsResponse {
    hazelnuts: Vec<RemoteSkill>,
}

#[derive(Debug, Deserialize)]
struct RemoteSkill {
    id: String,
    name: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct RemoteSkillsDownloadResponse {
    hazelnuts: Vec<RemoteSkillDownloadPayload>,
}

#[derive(Debug, Deserialize)]
struct RemoteSkillDownloadPayload {
    id: String,
    name: String,
    #[serde(rename = "base_sediment_id")]
    _base_sediment_id: String,
    files: HashMap<String, RemoteSkillFileRangePayload>,
}

#[derive(Debug, Deserialize)]
struct RemoteSkillFileRangePayload {
    start: u64,
    length: u64,
}

pub async fn list_remote_skills(config: &Config) -> Result<Vec<RemoteSkillSummary>> {
    let base_url = config.chatgpt_base_url.trim_end_matches('/');
    let base_url = base_url.strip_suffix("/backend-api").unwrap_or(base_url);
    let url = format!("{base_url}/public-api/hazelnuts/");

    let client = build_reqwest_client();
    let response = client
        .get(&url)
        .timeout(REMOTE_SKILLS_API_TIMEOUT)
        .query(&[("product_surface", "codex")])
        .send()
        .await
        .with_context(|| format!("Failed to send request to {url}"))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("Request failed with status {status} from {url}: {body}");
    }

    let parsed: RemoteSkillsResponse =
        serde_json::from_str(&body).context("Failed to parse skills response")?;

    Ok(parsed
        .hazelnuts
        .into_iter()
        .map(|skill| RemoteSkillSummary {
            id: skill.id,
            name: skill.name,
            description: skill.description,
        })
        .collect())
}

pub async fn download_remote_skill(
    config: &Config,
    hazelnut_id: &str,
    is_preload: bool,
) -> Result<RemoteSkillDownloadResult> {
    let hazelnut = fetch_remote_skill(config, hazelnut_id).await?;

    let client = build_reqwest_client();
    let base_url = config.chatgpt_base_url.trim_end_matches('/');
    let base_url = base_url.strip_suffix("/backend-api").unwrap_or(base_url);
    let url = format!("{base_url}/public-api/hazelnuts/{hazelnut_id}/export");
    let response = client
        .get(&url)
        .timeout(REMOTE_SKILLS_API_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("Failed to send download request to {url}"))?;

    let status = response.status();
    let body = response.bytes().await.context("Failed to read download")?;
    if !status.is_success() {
        let body_text = String::from_utf8_lossy(&body);
        anyhow::bail!("Download failed with status {status} from {url}: {body_text}");
    }

    if !is_zip_payload(&body) {
        anyhow::bail!("Downloaded remote skill payload is not a zip archive");
    }

    let preferred_dir_name = if hazelnut.name.trim().is_empty() {
        None
    } else {
        Some(hazelnut.name.as_str())
    };
    let dir_name = preferred_dir_name
        .and_then(validate_dir_name_format)
        .or_else(|| validate_dir_name_format(&hazelnut.id))
        .ok_or_else(|| anyhow::anyhow!("Remote skill has no valid directory name"))?;
    let output_root = if is_preload {
        config
            .codex_home
            .join("vendor_imports")
            .join("skills")
            .join("skills")
            .join(".curated")
    } else {
        config.codex_home.join("skills").join("downloaded")
    };
    let output_dir = output_root.join(dir_name);
    tokio::fs::create_dir_all(&output_dir)
        .await
        .context("Failed to create downloaded skills directory")?;

    let allowed_files = hazelnut.files.keys().cloned().collect::<HashSet<String>>();
    let zip_bytes = body.to_vec();
    let output_dir_clone = output_dir.clone();
    let prefix_candidates = vec![hazelnut.name.clone(), hazelnut.id.clone()];
    tokio::task::spawn_blocking(move || {
        extract_zip_to_dir(
            zip_bytes,
            &output_dir_clone,
            &allowed_files,
            &prefix_candidates,
        )
    })
    .await
    .context("Zip extraction task failed")??;

    Ok(RemoteSkillDownloadResult {
        id: hazelnut.id,
        name: hazelnut.name,
        path: output_dir,
    })
}

async fn fetch_remote_skill(
    config: &Config,
    hazelnut_id: &str,
) -> Result<RemoteSkillDownloadPayload> {
    let base_url = config.chatgpt_base_url.trim_end_matches('/');
    let base_url = base_url.strip_suffix("/backend-api").unwrap_or(base_url);
    let url = format!("{base_url}/public-api/hazelnuts/");

    let client = build_reqwest_client();
    let response = client
        .get(&url)
        .timeout(REMOTE_SKILLS_API_TIMEOUT)
        .query(&[("product_surface", "codex")])
        .send()
        .await
        .with_context(|| format!("Failed to send request to {url}"))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("Request failed with status {status} from {url}: {body}");
    }

    let parsed: RemoteSkillsDownloadResponse =
        serde_json::from_str(&body).context("Failed to parse skills response")?;
    parsed
        .hazelnuts
        .into_iter()
        .find(|skill| skill.id == hazelnut_id)
        .ok_or_else(|| anyhow::anyhow!("Remote skill not found: {hazelnut_id}"))
}

fn safe_join(base: &Path, name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => anyhow::bail!("Invalid file path in remote skill payload: {name}"),
        }
    }
    Ok(base.join(path))
}

fn validate_dir_name_format(name: &str) -> Option<String> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(component)), None) => {
            let value = component.to_string_lossy().to_string();
            (!value.is_empty()).then_some(value)
        }
        _ => None,
    }
}

fn is_zip_payload(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
}

fn extract_zip_to_dir(
    bytes: Vec<u8>,
    output_dir: &Path,
    allowed_files: &HashSet<String>,
    prefix_candidates: &[String],
) -> Result<()> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("Failed to open zip archive")?;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .context("Failed to read zip entry")?;
        if file.is_dir() {
            continue;
        }
        let raw_name = file.name().to_string();
        let Some(normalized) = normalize_zip_name(&raw_name, prefix_candidates) else {
            continue;
        };
        if !allowed_files.contains(&normalized) {
            continue;
        }
        let file_path = safe_join(output_dir, &normalized)?;
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create parent directory")?;
        }
        let mut output = std::fs::File::create(&file_path).context("Failed to create file")?;
        std::io::copy(&mut file, &mut output).context("Failed to write extracted file")?;
    }
    Ok(())
}

fn normalize_zip_name(raw_name: &str, prefix_candidates: &[String]) -> Option<String> {
    let path = Path::new(raw_name);
    let mut components = path.components();
    let first = components.next()?;
    let Component::Normal(first_component) = first else {
        return None;
    };
    let first_component = first_component.to_string_lossy();
    if !prefix_candidates
        .iter()
        .any(|candidate| candidate == &first_component)
    {
        return None;
    }
    let normalized = components
        .map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?
        .join("/");
    (!normalized.is_empty()).then_some(normalized)
}
