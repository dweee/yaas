use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::{Context, Result, bail, ensure};
use futures::StreamExt;
use tokio::{
    fs,
    sync::{RwLock, mpsc::UnboundedSender},
};
use tokio_util::sync::CancellationToken;

use super::{RepoAppList, RepoCapabilities, RepoDownloadResult};
use crate::{
    archive::decompress_archive,
    downloader::{
        AppDownloadProgress, TransferStats,
        config::DownloaderConfig,
        rclone::{self, RcloneCli, RcloneTransferOperation},
    },
    models::{CloudApp, Settings},
};

#[derive(derive_more::Debug, Clone)]
pub(in crate::downloader) struct PublicServerRepo {
    cli: RcloneCli,
    #[debug(skip)]
    password: String,
    apps: Arc<RwLock<HashMap<String, u64>>>,
}

fn release_source(name: &str) -> String {
    format!(":http:/{:x}/", md5::compute(format!("{name}\n")))
}

impl PublicServerRepo {
    pub(super) async fn new(
        cfg: &DownloaderConfig,
        cache: &Path,
        settings: &Settings,
        cancel: &CancellationToken,
    ) -> Result<Self> {
        let server = cfg.public_server.as_ref().context("Missing public_server configuration")?;
        let key = std::env::var(&server.api_key_env).with_context(|| {
            format!("Set {} to the provider API key before starting YAAS", server.api_key_env)
        })?;
        ensure!(
            !key.trim().is_empty() && !key.contains(['\r', '\n', '\0']),
            "Provider API key must be nonempty and contain no line breaks or NUL"
        );
        let (bin, config) = rclone::prepare_rclone_files(cache, cfg, cancel).await?;
        let mut cli = RcloneCli::new(bin, config, settings.bandwidth_limit.clone());
        cli.configure_public_http(format!("{}/", server.base_uri.trim_end_matches('/')), key);
        Ok(Self { cli, password: server.decoded_password()?, apps: Arc::default() })
    }

    pub(super) fn capabilities() -> RepoCapabilities {
        RepoCapabilities {
            supports_remote_selection: false,
            supports_bandwidth_limit: true,
            supports_download_mode_selection: false,
            supports_donation_upload: false,
        }
    }

    pub(super) fn set_bandwidth_limit(&mut self, limit: String) {
        self.cli.set_bandwidth_limit(limit);
    }

    pub(super) async fn load_app_list(
        &self,
        cache: &Path,
        cancel: CancellationToken,
    ) -> Result<RepoAppList> {
        let staging = tempfile::Builder::new().prefix("public-meta-").tempdir_in(cache)?;
        let archive = staging.path().join("meta.7z");
        self.cli
            .transfer(
                ":http:/meta.7z".into(),
                archive.to_string_lossy().into_owned(),
                RcloneTransferOperation::CopyTo,
                Some(cancel.clone()),
            )
            .await?;
        let extracted = staging.path().join("extracted");
        fs::create_dir_all(&extracted).await?;
        decompress_archive(&archive, &extracted, Some(&self.password), None, Some(cancel.clone()))
            .await?;
        let mut entries = fs::read_dir(&extracted).await?;
        let mut catalogs = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with("gamelist.txt")
            {
                catalogs.push(entry.path());
            }
        }
        ensure!(catalogs.len() == 1, "Metadata must contain exactly one *GameList.txt catalog");
        let apps = read_catalog(&catalogs[0], &cancel).await?;
        ensure!(!apps.is_empty(), "Public server catalog is empty");
        let index: HashMap<_, _> =
            apps.iter().map(|app| (app.full_name.clone(), app.size)).collect();
        ensure!(index.len() == apps.len(), "Public server catalog contains duplicate releases");
        ensure!(!cancel.is_cancelled(), "Operation cancelled");
        *self.apps.write().await = index;
        Ok(RepoAppList { apps, donation_blacklist: Vec::new() })
    }

    pub(super) async fn download_app(
        &self,
        name: &str,
        destination: &Path,
        progress: UnboundedSender<AppDownloadProgress>,
        cancel: CancellationToken,
    ) -> Result<RepoDownloadResult> {
        let size = *self
            .apps
            .read()
            .await
            .get(name)
            .context("Release is absent from the loaded public server catalog")?;
        ensure!(
            !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\']),
            "Invalid release name"
        );
        let parent = destination.parent().context("Download destination has no parent")?;
        fs::create_dir_all(parent).await?;
        let staging = tempfile::Builder::new().prefix(".public-download-").tempdir_in(parent)?;
        let parts = staging.path().join("parts");
        fs::create_dir(&parts).await?;
        let _ = progress.send(AppDownloadProgress::Status("Downloading archive parts...".into()));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TransferStats>();
        let transfer = self.cli.transfer_with_stats(
            release_source(name),
            parts.to_string_lossy().into_owned(),
            RcloneTransferOperation::Copy,
            size,
            Some(tx),
            Some(cancel.clone()),
        );
        tokio::pin!(transfer);
        loop {
            tokio::select! {
                result = &mut transfer => { result?; break; },
                Some(stats) = rx.recv() => { let _ = progress.send(AppDownloadProgress::Transfer(stats)); },
            }
        }
        let mut entries = fs::read_dir(&parts).await?;
        let mut archives = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let filename = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if entry.file_type().await?.is_file()
                && (filename.ends_with(".7z.001") || filename.ends_with(".7z"))
            {
                archives.push(entry.path());
            }
        }
        ensure!(archives.len() == 1, "Release must contain exactly one 7z archive or first volume");
        let extracted = staging.path().join("extracted");
        fs::create_dir(&extracted).await?;
        let _ = progress.send(AppDownloadProgress::Status("Extracting archive...".into()));
        decompress_archive(
            &archives[0],
            &extracted,
            Some(&self.password),
            None,
            Some(cancel.clone()),
        )
        .await?;
        ensure!(!cancel.is_cancelled(), "Operation cancelled");
        let release_dir = extracted.join(name);
        let source = if release_dir.is_dir() { &release_dir } else { &extracted };
        ensure!(
            fs::read_dir(source).await?.next_entry().await?.is_some(),
            "Release archive contains no files"
        );
        // Preserve the previous download until extraction has succeeded.
        let backup = staging.path().join("previous");
        let had_previous = destination.exists();
        if had_previous {
            fs::rename(destination, &backup).await?;
        }
        if let Err(error) = fs::rename(source, destination).await {
            if had_previous {
                fs::rename(&backup, destination)
                    .await
                    .context("Failed to restore previous download")?;
            }
            return Err(error.into());
        }
        Ok(RepoDownloadResult { skipped: false })
    }
}

async fn read_catalog(path: &Path, cancel: &CancellationToken) -> Result<Vec<CloudApp>> {
    let file = fs::File::open(path).await?;
    let mut reader = csv_async::AsyncReaderBuilder::new()
        .delimiter(b';')
        .trim(csv_async::Trim::All)
        .create_deserializer(file);
    let mut records = reader.deserialize::<CloudApp>();
    let mut apps = Vec::new();
    loop {
        let record = tokio::select! {
            _ = cancel.cancelled() => bail!("Operation cancelled"),
            record = records.next() => record,
        };
        let Some(record) = record else { break };
        apps.push(record.context("Invalid public server catalog row")?);
    }
    Ok(apps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_hash_includes_line_feed_and_directory_slash() {
        assert_eq!(release_source("Example v1"), ":http:/1e99966fc31c9243b67cfa741ccd2f1a/");
    }

    #[tokio::test]
    async fn parses_provider_catalog_and_rejects_malformed_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("VRP-GameList.txt");
        fs::write(&path, "Game Name;Release Name;Package Name;Version Code;Last Updated;Size (MB)\nExample;Example v1;com.example;1;2026-01-01;12.5\n").await.unwrap();
        let cancel = CancellationToken::new();
        let apps = read_catalog(&path, &cancel).await.unwrap();
        assert_eq!(apps[0].size, 12_500_000);
        assert_eq!(apps[0].full_name, "Example v1");
        fs::write(&path, "invalid\nrow\n").await.unwrap();
        assert!(read_catalog(&path, &cancel).await.is_err());
        cancel.cancel();
        assert!(read_catalog(&path, &cancel).await.is_err());
    }
    #[tokio::test]
    #[ignore = "requires rclone and 7z on PATH"]
    async fn authenticated_catalog_and_archive_round_trip() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{header, path},
        };
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let fixtures = dir.path().join("fixtures");
        fs::create_dir(&fixtures).await.unwrap();
        let catalog = "Game Name;Release Name;Package Name;Version Code;Last Updated;Size (MB)\nExample;Example v1;com.example;1;2026-01-01;1\n";
        fs::write(fixtures.join("VRP-GameList.txt"), catalog).await.unwrap();
        fs::write(fixtures.join("example.apk"), b"fixture apk").await.unwrap();
        for (archive, input) in [("meta.7z", "VRP-GameList.txt"), ("release.7z", "example.apk")] {
            let output = std::process::Command::new("7z")
                .current_dir(&fixtures)
                .args(["a", "-pfixture-password", "-mhe=on", archive, input])
                .output()
                .unwrap();
            assert!(output.status.success());
        }
        let meta = fs::read(fixtures.join("meta.7z")).await.unwrap();
        let release = fs::read(fixtures.join("release.7z")).await.unwrap();
        let hash = "1e99966fc31c9243b67cfa741ccd2f1a";
        for (url, body) in
            [("/meta.7z".to_string(), meta), (format!("/{hash}/release.7z"), release)]
        {
            Mock::given(path(url))
                .and(header("X-API-Key", "fixture-key"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
                .mount(&server)
                .await;
        }
        Mock::given(path(format!("/{hash}/")))
            .and(header("X-API-Key", "fixture-key"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                b"<html><body><a href=\"release.7z\">release.7z</a></body></html>".to_vec(),
                "text/html",
            ))
            .mount(&server)
            .await;
        let null_config = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let mut cli = RcloneCli::new("rclone".into(), null_config.into(), String::new());
        cli.configure_public_http(format!("{}/", server.uri()), "fixture-key".into());
        assert!(!format!("{cli:?}").contains("fixture-key"));
        let repo =
            PublicServerRepo { cli, password: "fixture-password".into(), apps: Arc::default() };
        let cancel = CancellationToken::new();
        let apps = repo.load_app_list(dir.path(), cancel.clone()).await.unwrap();
        assert_eq!(apps.apps[0].full_name, "Example v1");
        let destination = dir.path().join("downloads").join("Example v1");
        fs::create_dir_all(&destination).await.unwrap();
        fs::write(destination.join("previous.apk"), b"previous").await.unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        repo.download_app("Example v1", &destination, tx.clone(), cancel.clone()).await.unwrap();
        assert_eq!(fs::read(destination.join("example.apk")).await.unwrap(), b"fixture apk");
        assert!(!destination.join("previous.apk").exists());
        // An invalid refreshed archive must not replace the loaded release index.
        server.reset().await;
        Mock::given(path("/meta.7z"))
            .respond_with(ResponseTemplate::new(200).set_body_string("corrupt"))
            .mount(&server)
            .await;
        assert!(repo.load_app_list(dir.path(), cancel.clone()).await.is_err());
        assert!(repo.apps.read().await.contains_key("Example v1"));
        Mock::given(path(format!("/{hash}/")))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                b"<html><body><a href=\"release.7z\">release.7z</a></body></html>".to_vec(),
                "text/html",
            ))
            .mount(&server)
            .await;
        Mock::given(path(format!("/{hash}/release.7z")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"corrupt archive".to_vec()))
            .mount(&server)
            .await;
        assert!(
            repo.download_app("Example v1", &destination, tx.clone(), cancel.clone())
                .await
                .is_err()
        );
        assert_eq!(fs::read(destination.join("example.apk")).await.unwrap(), b"fixture apk");
        cancel.cancel();
        assert!(repo.download_app("Example v1", &destination, tx, cancel).await.is_err());
        assert_eq!(fs::read(destination.join("example.apk")).await.unwrap(), b"fixture apk");
    }
}
