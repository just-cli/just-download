use indicatif::ProgressBar;
use just_core::manifest::{Manifest, Package};
use just_core::result::BoxedResult;
use semver::{Version, VersionReq};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub struct DownloadInfo<'a> {
    pub package: &'a Package,
    pub version: Version,
    pub size: u64,
    pub compressed_path: PathBuf,
    pub uncompressed_path: PathBuf,
}

struct DownloadPath {
    compressed_path: PathBuf,
    uncompressed_path: PathBuf,
}

impl DownloadPath {
    fn from(download_url: &str) -> BoxedResult<Self> {
        use reqwest::Url;

        let url = Url::parse(download_url)?;
        let uncompressed_path = url
            .path_segments()
            .and_then(|segments| segments.last())
            .expect("Could not extract uncompressed filename");

        let compressed_path = url
            .fragment()
            .expect("Could not extract compressed filename");

        Ok(Self {
            compressed_path: Path::new(compressed_path).to_owned(),
            uncompressed_path: Path::new(uncompressed_path).to_owned(),
        })
    }
}

struct DownloadProgress<'a, R> {
    inner: R,
    progress_bar: &'a ProgressBar,
}

impl<'a, R: Read> Read for DownloadProgress<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf).map(|n| {
            self.progress_bar.inc(n as u64);
            n
        })
    }
}

fn assemble_download_url(
    manifest: &Manifest,
    req: Option<VersionReq>,
) -> Option<(String, Version)> {
    use just_versions::find_matching_version;

    let download_url = manifest.download.url.as_str();
    manifest
        .versions
        .as_ref()
        .and_then(|versions| {
            find_matching_version(versions, req).and_then(|version| {
                let url = download_url.replace("{version}", version.to_string().as_str());

                Some((url, version))
            })
        })
        .or_else(|| {
            manifest.download.version.as_ref().and_then(|version| {
                let url = download_url.replace("{version}", version.to_string().as_str());

                Some((url, version.clone()))
            })
        })
}

pub fn download(manifest: &Manifest, req: Option<VersionReq>) -> BoxedResult<DownloadInfo> {
    use indicatif::ProgressStyle;
    use log::{debug, info};
    use reqwest::header::{HeaderValue, CONTENT_LENGTH};
    use std::fs::OpenOptions;
    use std::io::copy;

    let (download_url, version) =
        assemble_download_url(manifest, req).expect("No Download-URL or valid Version given");
    info!("Downloading from {}...", download_url);

    let response = reqwest::get(&download_url)?;
    let byte_size: u64 = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|hv: &HeaderValue| hv.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .expect("No (numeric) Content-Length given");

    debug!("Downloaded {} Bytes", byte_size);

    let pb = ProgressBar::new(byte_size);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
        .progress_chars("=>"));

    let mut source = DownloadProgress {
        progress_bar: &pb,
        inner: response,
    };
    let download_path = DownloadPath::from(&download_url)?;

    info!("Downloading into {:?}", download_path.compressed_path);
    let mut dest = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&download_path.compressed_path)
        .unwrap_or_else(|e| {
            panic!(
                "Could not open compressed path {:?}: {:?}",
                download_path.compressed_path, e
            )
        });
    info!("Copy into {:?}", download_path.compressed_path);

    let download_size = copy(&mut source, &mut dest)?;

    pb.finish();
    info!(
        "Download of '{}' has been completed.",
        manifest.package.name.as_str()
    );

    Ok(DownloadInfo {
        package: &manifest.package,
        version,
        size: download_size,
        compressed_path: download_path.compressed_path.to_owned(),
        uncompressed_path: download_path.uncompressed_path.to_owned(),
    })
}
