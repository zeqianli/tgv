mod support;

use gv_core::message::{Message as CoreMessage, Movement, Scroll, Zoom};
use rstest::rstest;
use support::{AppHarness, test_data_path};
use tempfile::TempDir;
use tgv::{app::Scene, message::Message, session::SessionFile};

fn absolutize_fixture_args(args: &str) -> String {
    args.replace(
        "tests/data/cache/wuhCor1/wuhCor1.2bit",
        &test_data_path("cache/wuhCor1/wuhCor1.2bit"),
    )
        .replace("tests/data/covid.fa", &test_data_path("covid.fa"))
        .replace("tests/data/cache", &test_data_path("cache"))
        .replace("tests/data/simple.vcf", &test_data_path("simple.vcf"))
        .replace("tests/data/simple.bed", &test_data_path("simple.bed"))
}

fn offline_case_args(bam_path: Option<&str>, args: &str) -> String {
    let args = absolutize_fixture_args(args);
    match bam_path {
        Some(bam_path) => format!("{} {args}", test_data_path(bam_path)),
        None => args,
    }
}

#[rstest]
#[case("-g ecoli --offline --cache-dir tests/data/cache")]
#[case("covid.sorted.bam --no-reference -r MN908947.3:100 --offline")]
#[case("covid.sorted.bam -g tests/data/covid.fa --offline")]
#[tokio::test]
async fn offline_initialization_succeeds(#[case] args: &str) {
    let args = if args.contains(".bam") {
        offline_case_args(None, &args.replace("covid.sorted.bam", &test_data_path("covid.sorted.bam")))
    } else {
        offline_case_args(None, args)
    };

    let harness = AppHarness::from_args(&args).await.unwrap();
    assert!(!harness.locus().is_empty());
    harness.close().await.unwrap();
}

#[tokio::test]
async fn offline_sequence_navigates_and_zooms() {
    let args = offline_case_args(
        Some("ncbi.sorted.bam"),
        "-r chr22:33121120 --no-reference --offline",
    );
    let mut harness = AppHarness::from_args(&args).await.unwrap();

    let initial_focus = harness.app.alignment_view.focus.clone();
    let initial_zoom = harness.app.alignment_view.zoom;

    harness
        .handle_core(vec![
            CoreMessage::Scroll(Scroll::Down(2)),
            CoreMessage::Zoom(Zoom::Out(4)),
            CoreMessage::Move(Movement::Position(33121140)),
            CoreMessage::Scroll(Scroll::Up(1)),
            CoreMessage::Zoom(Zoom::In(2)),
        ])
        .await
        .unwrap();

    assert_eq!(
        harness.app.alignment_view.focus.contig_index,
        initial_focus.contig_index
    );
    assert_eq!(harness.app.alignment_view.focus.position, 33_121_140);
    assert_eq!(harness.app.alignment_view.zoom, initial_zoom * 2);
    assert_eq!(harness.app.alignment_view.y, 1);
    assert!(harness.app.state.messages.is_empty());

    harness.close().await.unwrap();
}

#[tokio::test]
async fn offline_sequence_updates_tracks_and_scenes() {
    let args = offline_case_args(
        Some("ncbi.sorted.bam"),
        "-r chr22:33121120 tests/data/simple.vcf tests/data/simple.bed --no-reference --offline",
    );
    let mut harness = AppHarness::from_args(&args).await.unwrap();

    harness
        .handle(vec![
            Message::SwitchScene(Scene::Help),
            Message::SwitchScene(Scene::Main),
            Message::Core(CoreMessage::Move(Movement::Position(33_121_130))),
            Message::Core(CoreMessage::Message("scripted-note".to_string())),
            Message::SwitchScene(Scene::ContigList),
            Message::SwitchScene(Scene::Main),
        ])
        .await
        .unwrap();

    assert_eq!(harness.app.scene, Scene::Main);
    assert!(harness.app.state.variant_loaded);
    assert!(harness.app.state.bed_loaded);
    assert_eq!(harness.app.alignment_view.focus.position, 33_121_130);
    assert_eq!(harness.app.state.messages, vec!["scripted-note".to_string()]);

    harness.close().await.unwrap();
}

#[tokio::test]
async fn offline_sequence_saves_session_and_save_and_quit() {
    let args = offline_case_args(
        Some("ncbi.sorted.bam"),
        "-r chr22:33121120 --no-reference --offline",
    );
    let mut harness = AppHarness::from_args(&args).await.unwrap();
    let temp_dir = TempDir::new().unwrap();
    let save_path = temp_dir.path().join("saved-session.toml");

    harness
        .handle_core(vec![CoreMessage::SaveSession(Some(
            save_path.display().to_string(),
        ))])
        .await
        .unwrap();

    assert_eq!(harness.app.session_path, save_path);
    assert!(save_path.exists());

    let session = SessionFile::from_path(&save_path).unwrap();
    assert_eq!(session.locus, harness.locus());

    let quit_path = temp_dir.path().join("quit-session.toml");
    harness
        .handle_core(vec![CoreMessage::SaveAndQuit(Some(
            quit_path.display().to_string(),
        ))])
        .await
        .unwrap();

    assert!(harness.app.exit);
    assert_eq!(harness.app.session_path, quit_path);
    assert!(quit_path.exists());

    harness.close().await.unwrap();
}
