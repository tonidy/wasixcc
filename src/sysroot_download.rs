use std::{fmt::Display, path::Path, str::FromStr};

use anyhow::{bail, Context};
use reqwest::header::HeaderMap;

use crate::UserSettings;

const SYSROOT_REPO: &str = "wasix-org/wasix-libc";

#[derive(serde::Deserialize)]
struct GithubReleaseData {
    assets: Vec<GithubAsset>,
}

#[derive(serde::Deserialize)]
struct GithubAsset {
    browser_download_url: String,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysrootSpec {
    Latest,
    Tag(String),
}

impl FromStr for SysrootSpec {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "latest" {
            Ok(SysrootSpec::Latest)
        } else if s.starts_with('v') {
            Ok(SysrootSpec::Tag(s.to_string()))
        } else {
            bail!("Invalid sysroot specification: `{s}`. Use 'latest' or a tag starting with 'v'.");
        }
    }
}

impl SysrootSpec {
    fn display_github_url_postfix(&self) -> SysrootSpecGithubUrlPostfix {
        SysrootSpecGithubUrlPostfix { spec: self }
    }
}

struct SysrootSpecGithubUrlPostfix<'a> {
    spec: &'a SysrootSpec,
}

impl Display for SysrootSpecGithubUrlPostfix<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.spec {
            SysrootSpec::Latest => write!(f, "latest"),
            SysrootSpec::Tag(tag) => write!(f, "tags/{}", tag),
        }
    }
}

pub(crate) fn download_sysroot(
    sysroot_spec: SysrootSpec,
    user_settings: &UserSettings,
) -> anyhow::Result<()> {
    let mut headers = HeaderMap::new();

    // Use API token if specified via env var.
    // Prevents 403 errors when IP is throttled by Github API.
    let gh_token = std::env::var("GITHUB_TOKEN")
        .ok()
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty());

    if let Some(token) = gh_token {
        headers.insert("authorization", format!("Bearer {token}").parse()?);
    }

    let client = reqwest::blocking::Client::builder()
        .default_headers(headers)
        .user_agent("wasixcc")
        .build()?;

    let release_url = format!(
        "https://api.github.com/repos/{SYSROOT_REPO}/releases/{}",
        sysroot_spec.display_github_url_postfix()
    );

    eprintln!("Retrieving release info from {release_url} ...");

    let release: GithubReleaseData = client
        .get(&release_url)
        .send()?
        .error_for_status()
        .context("Could not download release info")?
        .json()
        .context("Could not deserialize release info")?;

    for asset_name in [
        "sysroot.tar.gz",
        "sysroot-eh.tar.gz",
        "sysroot-ehpic.tar.gz",
    ] {
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .with_context(|| format!("Could not find asset '{asset_name}' in release"))?;

        download_and_unpack(asset, user_settings.sysroot_prefix(), &client).with_context(|| {
            format!("Failed to download and unpack sysroot asset '{asset_name}'")
        })?;
    }

    Ok(())
}

fn download_and_unpack(
    asset: &GithubAsset,
    target_dir: &Path,
    client: &reqwest::blocking::Client,
) -> anyhow::Result<()> {
    eprintln!(
        "Downloading asset '{}' from url '{}'...",
        asset.name, asset.browser_download_url
    );
    let res = client
        .get(&asset.browser_download_url)
        .send()?
        .error_for_status()?;

    let decoder = flate2::read::GzDecoder::new(res);
    let mut archive = tar::Archive::new(decoder);

    // Unpack to a temp dir, since we need to re-organize the contents.
    let temp_dir = tempfile::TempDir::new().context("Failed to create temporary directory")?;

    archive
        .unpack(temp_dir.path())
        .context("Failed to unpack asset")?;

    // A few sanity checks can't hurt...
    let dirs = std::fs::read_dir(temp_dir.path())
        .context("Failed to read unpacked asset directory")?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to collect unpacked asset entries")?;

    if dirs.len() != 1 {
        bail!("Expected exactly one directory in unpacked asset, found {dirs:?}");
    }

    let asset_dir_file_name = dirs[0].file_name();
    let asset_dir_name = asset_dir_file_name
        .to_str()
        .context("Expected directory name to be valid UTF-8")?;

    let postfix = asset_dir_name
        .strip_prefix("wasix-sysroot")
        .with_context(|| {
            format!(
                "Expected unpacked asset directory to start with \
                'wasix-sysroot', found {asset_dir_name}"
            )
        })?;

    std::fs::create_dir_all(target_dir).context("Failed to create target directory")?;

    let final_dir = target_dir.join(format!("sysroot{postfix}"));
    if final_dir.exists() {
        std::fs::remove_dir_all(&final_dir).with_context(|| {
            format!(
                "Failed to remove existing sysroot directory at {}",
                final_dir.display(),
            )
        })?;
    }

    move_dir(dirs[0].path().join("sysroot"), &final_dir)?;

    eprintln!(
        "Downloaded sysroot asset '{}' to '{}'",
        asset.name,
        final_dir.display()
    );

    Ok(())
}

fn move_dir(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> anyhow::Result<()> {
    let src = src.as_ref();
    let dst = dst.as_ref();

    if dst.exists() {
        bail!("Destination directory {} already exists", dst.display());
    }

    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            // If the rename fails due to crossing device boundaries, copy the directory.
            std::fs::copy(src, dst).context("Failed to copy directory")?;
            std::fs::remove_dir_all(src).context("Failed to remove source directory")?;
            Ok(())
        }
        Err(e) => Err(e).context("Failed to move directory"),
    }
}
