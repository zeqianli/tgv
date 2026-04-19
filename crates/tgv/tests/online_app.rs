mod support;

use gv_core::{reference::Reference, tracks::UCSCDownloader};
use rstest::rstest;
use std::path::Path;
use support::{AppHarness, test_data_path};
use tempfile::TempDir;

fn online_case_args(bam_path: Option<&str>, args: &str) -> String {
    match bam_path {
        Some(bam_path) => format!("{} {args}", test_data_path(bam_path)),
        None => args.to_string(),
    }
}

#[rstest]
#[case(None, "--online")]
#[case(Some("ncbi.sorted.bam"), "-r 22:33121120 -g hg19 --online")]
#[case(None, "-g GCF_028858775.2 -r NC_072398.2:76951800 --online")]
#[tokio::test]
#[ignore = "requires network and third-party services"]
async fn online_initialization_succeeds(#[case] bam_path: Option<&str>, #[case] args: &str) {
    let args = online_case_args(bam_path, args);
    let harness = AppHarness::from_args(&args).await.unwrap();
    assert!(!harness.locus().is_empty());
    harness.close().await.unwrap();
}

#[rstest]
#[case("wuhCor1")]
#[case("ecoli")]
#[tokio::test]
#[ignore = "requires network and third-party services"]
async fn online_download_integration_test(#[case] reference_str: &str) {
    let reference = reference_str.parse::<Reference>().unwrap();
    let temp_dir = TempDir::new().unwrap();
    let temp_dir_str = temp_dir.path().to_str().unwrap();
    let downloader = UCSCDownloader::new(reference, temp_dir_str).unwrap();

    downloader.download().await.unwrap();

    assert!(Path::new(temp_dir_str).join(reference_str).exists());
    assert!(
        Path::new(temp_dir_str)
            .join(reference_str)
            .join("tracks.sqlite")
            .exists()
    );
}
