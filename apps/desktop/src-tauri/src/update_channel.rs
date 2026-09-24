use std::{collections::BTreeSet, time::Duration};

use reqwest::{Client, StatusCode, redirect::Policy};
use serde::{Deserialize, Serialize};
use url::Url;

const UPDATE_MANIFEST_URL: Option<&str> = option_env!("LOCALVIEW_UPDATE_MANIFEST_URL");
const UPDATE_CHANNEL: &str = "stable";
const UPDATE_MANIFEST_SCHEMA: &str = "localview-update-manifest-v1";
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_ARTIFACTS: usize = 16;
const MAX_FIELD_BYTES: usize = 4096;
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UpdateCheckReason {
    ChannelNotConfigured,
    UpToDate,
    UpdateAvailableManualOnly,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateChannelReceipt {
    pub configured: bool,
    pub channel: String,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub install_authorized: bool,
    pub signature_present: bool,
    pub reason: UpdateCheckReason,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateManifestV1 {
    schema: String,
    channel: String,
    version: String,
    candidate_sha: String,
    artifacts: Vec<UpdateArtifactV1>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateArtifactV1 {
    os: String,
    arch: String,
    url: String,
    sha256: String,
    signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidatedUpdate {
    version: String,
    update_available: bool,
    signature_present: bool,
}

#[tauri::command]
pub(crate) async fn check_update_channel() -> Result<UpdateChannelReceipt, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_owned();
    let Some(configured_url) = UPDATE_MANIFEST_URL else {
        return Ok(UpdateChannelReceipt {
            configured: false,
            channel: UPDATE_CHANNEL.into(),
            current_version,
            latest_version: None,
            update_available: false,
            install_authorized: false,
            signature_present: false,
            reason: UpdateCheckReason::ChannelNotConfigured,
        });
    };

    let manifest_url = parse_channel_url(configured_url)?;
    let manifest_bytes = fetch_manifest(&manifest_url).await?;
    let manifest: UpdateManifestV1 = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| "update manifest is not strict valid JSON".to_string())?;
    let validated = validate_manifest(
        manifest,
        &manifest_url,
        &current_version,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )?;

    Ok(UpdateChannelReceipt {
        configured: true,
        channel: UPDATE_CHANNEL.into(),
        current_version,
        latest_version: Some(validated.version),
        update_available: validated.update_available,
        // R16 is intentionally check-only. A transport-authenticated manifest,
        // digest and even a detached signature field do not authorize install
        // until LocalView has a production signature-verification authority.
        install_authorized: false,
        signature_present: validated.signature_present,
        reason: if validated.update_available {
            UpdateCheckReason::UpdateAvailableManualOnly
        } else {
            UpdateCheckReason::UpToDate
        },
    })
}

async fn fetch_manifest(url: &Url) -> Result<Vec<u8>, String> {
    let client = Client::builder()
        .redirect(Policy::none())
        .timeout(UPDATE_CHECK_TIMEOUT)
        .build()
        .map_err(|_| "update client construction failed".to_string())?;
    let mut response = client
        .get(url.clone())
        .header(reqwest::header::ACCEPT, "application/json")
        .header(
            reqwest::header::USER_AGENT,
            format!("localview-update-check/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .map_err(|_| "update channel request failed".to_string())?;

    if response.status() != StatusCode::OK {
        return Err(format!(
            "update channel returned unexpected HTTP status {}",
            response.status().as_u16()
        ));
    }
    if response.url() != url {
        return Err("update channel redirect or URL drift rejected".into());
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_MANIFEST_BYTES as u64)
    {
        return Err("update manifest exceeds size bound".into());
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .ok_or_else(|| "update manifest content type is missing".to_string())?
        .to_str()
        .map_err(|_| "update manifest content type is invalid".to_string())?;
    validate_manifest_content_type(content_type)?;

    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "update manifest body read failed".to_string())?
    {
        if body.len().saturating_add(chunk.len()) > MAX_MANIFEST_BYTES {
            return Err("update manifest exceeds size bound".into());
        }
        body.extend_from_slice(&chunk);
    }
    if body.is_empty() {
        return Err("update manifest is empty".into());
    }
    Ok(body)
}

fn parse_channel_url(value: &str) -> Result<Url, String> {
    if value.len() > MAX_FIELD_BYTES {
        return Err("update manifest URL exceeds bound".into());
    }
    let url = Url::parse(value).map_err(|_| "update manifest URL is invalid".to_string())?;
    validate_https_url(&url, "update manifest")?;
    Ok(url)
}

fn validate_manifest(
    manifest: UpdateManifestV1,
    manifest_url: &Url,
    current_version: &str,
    target_os: &str,
    target_arch: &str,
) -> Result<ValidatedUpdate, String> {
    if manifest.schema != UPDATE_MANIFEST_SCHEMA {
        return Err("unsupported update manifest schema".into());
    }
    if manifest.channel != UPDATE_CHANNEL {
        return Err("update manifest channel mismatch".into());
    }
    validate_bounded_field(&manifest.version, "update version")?;
    validate_bounded_field(&manifest.candidate_sha, "update candidate SHA")?;
    let current = parse_semver(current_version)?;
    let latest = parse_semver(&manifest.version)?;
    if !is_lower_hex(&manifest.candidate_sha, 40) {
        return Err("update candidate SHA must be a full lowercase git object id".into());
    }
    if manifest.artifacts.is_empty() || manifest.artifacts.len() > MAX_ARTIFACTS {
        return Err("update artifact set is empty or exceeds bound".into());
    }

    let mut seen_targets = BTreeSet::new();
    let mut current_target = None;
    for artifact in &manifest.artifacts {
        validate_bounded_field(&artifact.os, "update artifact OS")?;
        validate_bounded_field(&artifact.arch, "update artifact architecture")?;
        validate_bounded_field(&artifact.url, "update artifact URL")?;
        validate_bounded_field(&artifact.sha256, "update artifact digest")?;
        if let Some(signature) = artifact.signature.as_deref() {
            validate_bounded_field(signature, "update artifact signature")?;
        }
        if !artifact.sha256.starts_with("sha256:")
            || !is_lower_hex(&artifact.sha256["sha256:".len()..], 64)
        {
            return Err("update artifact digest must be sha256:<64 lowercase hex>".into());
        }
        if !seen_targets.insert((artifact.os.clone(), artifact.arch.clone())) {
            return Err("update manifest contains duplicate OS/architecture artifacts".into());
        }

        let artifact_url =
            Url::parse(&artifact.url).map_err(|_| "update artifact URL is invalid".to_string())?;
        validate_https_url(&artifact_url, "update artifact")?;
        if !same_origin(manifest_url, &artifact_url) {
            return Err("update artifact must remain on the pinned manifest origin".into());
        }
        if artifact.os == target_os && artifact.arch == target_arch {
            current_target = Some(artifact);
        }
    }

    let artifact = current_target
        .ok_or_else(|| "update manifest is missing an artifact for this target".to_string())?;
    Ok(ValidatedUpdate {
        version: manifest.version,
        update_available: latest > current,
        signature_present: artifact
            .signature
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()),
    })
}

fn validate_manifest_content_type(value: &str) -> Result<(), String> {
    let media_type = value
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if media_type != "application/json" {
        return Err("update manifest content type must be application/json".into());
    }
    Ok(())
}

fn validate_https_url(url: &Url, label: &str) -> Result<(), String> {
    if url.scheme() != "https" {
        return Err(format!("{label} URL must use HTTPS"));
    }
    if url.host_str().is_none() {
        return Err(format!("{label} URL must contain a host"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(format!("{label} URL must not contain credentials"));
    }
    if url.fragment().is_some() || url.query().is_some() {
        return Err(format!("{label} URL must not contain query or fragment"));
    }
    if url.port_or_known_default() != Some(443) {
        return Err(format!("{label} URL must use the default HTTPS port"));
    }
    Ok(())
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn validate_bounded_field(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > MAX_FIELD_BYTES || value.contains(' ') {
        return Err(format!("{label} is empty or exceeds bound"));
    }
    Ok(())
}

fn is_lower_hex(value: &str, exact_len: usize) -> bool {
    value.len() == exact_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn parse_semver(value: &str) -> Result<(u64, u64, u64), String> {
    if value.is_empty() || value.len() > 32 {
        return Err("update version must be strict MAJOR.MINOR.PATCH".into());
    }
    let components = value.split('.').collect::<Vec<_>>();
    if components.len() != 3 {
        return Err("update version must be strict MAJOR.MINOR.PATCH".into());
    }
    let mut parsed = [0_u64; 3];
    for (index, component) in components.into_iter().enumerate() {
        if component.is_empty()
            || !component.bytes().all(|byte| byte.is_ascii_digit())
            || (component.len() > 1 && component.starts_with('0'))
        {
            return Err("update version must be strict MAJOR.MINOR.PATCH".into());
        }
        parsed[index] = component
            .parse::<u64>()
            .map_err(|_| "update version component exceeds bound".to_string())?;
    }
    Ok((parsed[0], parsed[1], parsed[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str, artifact_url: &str) -> UpdateManifestV1 {
        UpdateManifestV1 {
            schema: UPDATE_MANIFEST_SCHEMA.into(),
            channel: UPDATE_CHANNEL.into(),
            version: version.into(),
            candidate_sha: "a".repeat(40),
            artifacts: vec![UpdateArtifactV1 {
                os: "linux".into(),
                arch: "x86_64".into(),
                url: artifact_url.into(),
                sha256: format!("sha256:{}", "b".repeat(64)),
                signature: Some("detached-signature-present-but-not-authoritative".into()),
            }],
        }
    }

    #[test]
    fn strict_semver_rejects_prerelease_and_leading_zero_forms() {
        for invalid in ["", "1", "1.2", "1.2.3-beta", "01.2.3", "1.02.3", "1.2.03"] {
            assert!(parse_semver(invalid).is_err(), "{invalid}");
        }
        assert_eq!(parse_semver("0.2.0").unwrap(), (0, 2, 0));
    }

    #[test]
    fn manifest_content_type_is_required_and_exact() {
        assert!(validate_manifest_content_type("application/json").is_ok());
        assert!(validate_manifest_content_type("application/json; charset=utf-8").is_ok());
        assert!(validate_manifest_content_type("text/json").is_err());
        assert!(validate_manifest_content_type("application/json-patch+json").is_err());
        assert!(validate_manifest_content_type("").is_err());
    }

    #[test]
    fn channel_and_artifact_must_be_https_same_origin() {
        assert!(parse_channel_url("http://updates.example.test/manifest.json").is_err());
        let manifest_url = parse_channel_url("https://updates.example.test/manifest.json").unwrap();
        let wrong_origin = manifest("0.3.0", "https://cdn.example.test/localview.bin");
        assert!(
            validate_manifest(wrong_origin, &manifest_url, "0.2.0", "linux", "x86_64").is_err()
        );
    }

    #[test]
    fn artifact_targets_are_globally_unique_and_all_entries_are_validated() {
        let manifest_url = parse_channel_url("https://updates.example.test/manifest.json").unwrap();
        let mut duplicate = manifest("0.3.0", "https://updates.example.test/localview.bin");
        duplicate.artifacts.push(duplicate.artifacts[0].clone());
        assert!(validate_manifest(duplicate, &manifest_url, "0.2.0", "linux", "x86_64").is_err());

        let mut invalid_other = manifest("0.3.0", "https://updates.example.test/localview.bin");
        invalid_other.artifacts.push(UpdateArtifactV1 {
            os: "windows".into(),
            arch: "x86_64".into(),
            url: "https://cdn.example.test/localview.exe".into(),
            sha256: format!("sha256:{}", "c".repeat(64)),
            signature: None,
        });
        assert!(
            validate_manifest(invalid_other, &manifest_url, "0.2.0", "linux", "x86_64").is_err()
        );
    }

    #[test]
    fn detached_signature_presence_never_authorizes_install() {
        let manifest_url = parse_channel_url("https://updates.example.test/manifest.json").unwrap();
        let validated = validate_manifest(
            manifest("0.3.0", "https://updates.example.test/localview.bin"),
            &manifest_url,
            "0.2.0",
            "linux",
            "x86_64",
        )
        .unwrap();
        assert!(validated.update_available);
        assert!(validated.signature_present);
        let receipt = UpdateChannelReceipt {
            configured: true,
            channel: UPDATE_CHANNEL.into(),
            current_version: "0.2.0".into(),
            latest_version: Some(validated.version),
            update_available: true,
            install_authorized: false,
            signature_present: validated.signature_present,
            reason: UpdateCheckReason::UpdateAvailableManualOnly,
        };
        assert!(!receipt.install_authorized);
    }

    #[test]
    fn same_or_older_manifest_never_claims_update_available() {
        let manifest_url = parse_channel_url("https://updates.example.test/manifest.json").unwrap();
        for version in ["0.1.9", "0.2.0"] {
            let validated = validate_manifest(
                manifest(version, "https://updates.example.test/localview.bin"),
                &manifest_url,
                "0.2.0",
                "linux",
                "x86_64",
            )
            .unwrap();
            assert!(!validated.update_available, "{version}");
        }
    }
}
