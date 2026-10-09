mod support;

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use gv_core::{
    message::{
        AlignmentDisplayOption, AlignmentSort, Message as CoreMessage, Movement, Scroll, Zoom,
    },
    repository::RepositoryFileIndex,
    track_registry::TrackId,
};
use gv_session::{
    DatasetRequest, HighlightRequest, InspectInterval, NavigateRequest, QueryRequest, SessionError,
};
use rstest::rstest;
use support::{AppHarness, test_data_path};
use tempfile::TempDir;
use tgv::{
    app::{Highlight, Scene},
    layout::AreaType,
    message::{Action, UpdateLayoutAction},
    session::SessionFile,
};

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
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_initialization_succeeds(#[case] args: &str) {
    let args = if args.contains(".bam") {
        offline_case_args(
            None,
            &args.replace("covid.sorted.bam", &test_data_path("covid.sorted.bam")),
        )
    } else {
        offline_case_args(None, args)
    };

    let harness = AppHarness::from_args(&args).await.unwrap();
    assert!(!harness.locus().is_empty());
    harness.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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
    assert_eq!(harness.app.alignment_view.top(0), 1);
    assert!(harness.app.dataset.view.messages.is_empty());

    harness.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_sequence_updates_tracks_and_scenes() {
    let args = offline_case_args(
        Some("ncbi.sorted.bam"),
        "-r chr22:33121120 tests/data/simple.vcf tests/data/simple.bed --no-reference --offline",
    );
    let mut harness = AppHarness::from_args(&args).await.unwrap();

    assert_eq!(harness.app.resolved_layout.sidebar_width, 18);
    assert_eq!(
        harness
            .app
            .resolved_layout
            .sidebar_labels
            .iter()
            .map(|label| label.name.as_str())
            .collect::<Vec<_>>(),
        vec!["ncbi.sorted.bam", "simple.vcf", "simple.bed"],
    );
    let label_area = harness.app.resolved_layout.sidebar_labels[0].area;
    let initial_buffer = harness.terminal_backend().buffer();
    let rendered_label = (0..label_area.height)
        .flat_map(|row| {
            (0..label_area.width).map(move |column| {
                initial_buffer
                    .cell((label_area.x + column, label_area.y + row))
                    .unwrap()
                    .symbol()
            })
        })
        .collect::<String>();
    assert!(rendered_label.contains("ncbi.sorted.bam"));
    let coordinate_index = harness
        .app
        .resolved_layout
        .areas
        .iter()
        .position(|(area, _)| matches!(area, tgv::layout::AreaType::Coordinate))
        .unwrap();
    let coordinate_area = harness.app.resolved_layout.sidebar_areas[coordinate_index];
    let coordinate_text = (0..coordinate_area.width)
        .map(|column| {
            initial_buffer
                .cell((coordinate_area.x + column, coordinate_area.y))
                .unwrap()
                .symbol()
        })
        .collect::<String>();
    assert!(coordinate_text.starts_with("chr22:33121120"));
    let depth_area = harness.app.resolved_layout.sidebar_alignment_depths[0].0;
    let depth_text = (0..depth_area.width)
        .map(|column| {
            initial_buffer
                .cell((depth_area.x + column, depth_area.y))
                .unwrap()
                .symbol()
        })
        .collect::<String>();
    assert!(depth_text.contains('%'));
    harness
        .handle_key_codes([KeyCode::Char('s')])
        .await
        .unwrap();
    assert_eq!(harness.app.resolved_layout.sidebar_width, 0);
    let collapsed_buffer = harness.terminal_backend().buffer();
    let collapsed_text = collapsed_buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(!collapsed_text.contains("chr22:33121120"));
    assert!(!collapsed_text.contains(depth_text.trim()));
    harness
        .handle_key_codes([KeyCode::Char('s')])
        .await
        .unwrap();
    assert_eq!(harness.app.resolved_layout.sidebar_width, 18);
    let down = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 18,
        row: 0,
        modifiers: KeyModifiers::NONE,
    };
    harness
        .app
        .mouse_register
        .handle_mouse_event(
            &harness.app.dataset.view,
            &harness.app.resolved_layout,
            &harness.app.alignment_view,
            down,
        )
        .unwrap();
    let drag = MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 24,
        ..down
    };
    let messages = harness
        .app
        .mouse_register
        .handle_mouse_event(
            &harness.app.dataset.view,
            &harness.app.resolved_layout,
            &harness.app.alignment_view,
            drag,
        )
        .unwrap();
    harness.handle(messages).await.unwrap();
    assert_eq!(harness.app.resolved_layout.sidebar_width, 24);
    harness
        .handle_key_codes([KeyCode::Char('s')])
        .await
        .unwrap();
    harness
        .handle_key_codes([KeyCode::Char('s')])
        .await
        .unwrap();
    assert_eq!(harness.app.resolved_layout.sidebar_width, 24);
    harness
        .handle(vec![Action::UpdateLayout(
            UpdateLayoutAction::SetSidebarWidth(6),
        )])
        .await
        .unwrap();
    let vcf_area = harness.app.resolved_layout.sidebar_labels[1].area;
    let sidebar_buffer = harness.terminal_backend().buffer();
    let first_vcf_line = (0..vcf_area.width)
        .map(|column| {
            sidebar_buffer
                .cell((vcf_area.x + column, vcf_area.y))
                .unwrap()
                .symbol()
        })
        .collect::<String>();
    let second_vcf_line = (0..vcf_area.width)
        .map(|column| {
            sidebar_buffer
                .cell((vcf_area.x + column, vcf_area.y + 1))
                .unwrap()
                .symbol()
        })
        .collect::<String>();
    assert_eq!(first_vcf_line, "simple");
    assert!(second_vcf_line.starts_with(".vcf"));
    assert_eq!(
        sidebar_buffer
            .cell((vcf_area.x, vcf_area.y - 1))
            .unwrap()
            .symbol(),
        "_"
    );
    let bed_area = harness.app.resolved_layout.sidebar_labels[2].area;
    assert!(bed_area.height >= 2);

    harness
        .handle(vec![
            Action::Core(CoreMessage::Move(Movement::Position(33_121_130))),
            Action::Core(CoreMessage::Message("scripted-note".to_string())),
            Action::SwitchScene(Scene::ContigList),
            Action::SwitchScene(Scene::Main),
        ])
        .await
        .unwrap();

    assert_eq!(harness.app.scene, Scene::Main);
    let displayed = harness
        .app
        .alignment_view
        .region(&harness.app.resolved_layout.main_area);
    assert!(harness.app.dataset.view.variants[0].has_complete_data(&displayed));
    assert!(harness.app.dataset.view.bed_intervals[0].has_complete_data(&displayed));
    assert_eq!(harness.app.alignment_view.focus.position, 33_121_130);
    assert_eq!(
        harness.app.dataset.view.messages,
        vec!["scripted-note".to_string()]
    );

    harness.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_sequence_handles_sorting_command() {
    let args = offline_case_args(
        Some("ncbi.sorted.bam"),
        "-r chr22:33121120 --no-reference --offline",
    );
    let mut harness = AppHarness::from_args(&args).await.unwrap();
    let sort_position = harness.app.alignment_view.focus.position;
    let initial_messages = harness.app.dataset.view.messages.clone();

    harness
        .handle_core(vec![CoreMessage::SetAlignmentOption(vec![
            AlignmentDisplayOption::Sort(AlignmentSort::BaseAtCurrentPosition),
        ])])
        .await
        .unwrap();

    assert_eq!(
        harness.app.dataset.view.alignment_options[0],
        vec![AlignmentDisplayOption::Sort(AlignmentSort::BaseAt(
            sort_position
        ))]
    );
    assert_eq!(harness.app.dataset.view.messages, initial_messages);
    assert!(harness.app.dataset.view.alignments[0].depth().unwrap() > 0);

    harness.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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

    assert_eq!(harness.app.settings.session_path, Some(save_path.clone()));
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
    assert_eq!(harness.app.settings.session_path, Some(quit_path.clone()));
    assert!(quit_path.exists());

    harness.close().await.unwrap();
}

fn track_indexes(harness: &AppHarness) -> Vec<(TrackId, RepositoryFileIndex)> {
    harness
        .app
        .dataset
        .tracks
        .entries
        .iter()
        .map(|entry| (entry.id, entry.repository_index))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_tracks_open_and_remove_at_runtime() {
    use RepositoryFileIndex::{Alignment, Bed, Variant};

    let mut harness = covid_harness().await;
    let opened = [
        "ncbi.sorted.bam",
        "simple.vcf",
        "covid.sorted.bam",
        "simple.bed",
    ]
    .map(test_data_path);
    harness
        .handle_command(&format!("e {}", opened.join(" ")))
        .await
        .unwrap();
    assert!(harness.app.dataset.view.messages[0].starts_with("Opening ncbi.sorted.bam, "));
    harness.open_pending().await.unwrap();

    assert_eq!(
        harness.app.dataset.view.messages,
        vec!["Opened 4 files.".to_string()]
    );
    assert_eq!(
        track_indexes(&harness),
        vec![
            (0, Alignment(0)),
            (1, Alignment(1)),
            (2, Variant(0)),
            (3, Alignment(2)),
            (4, Bed(0)),
        ]
    );
    assert_eq!(harness.app.alignment_view.y.len(), 3);
    assert_eq!(
        harness.app.layout.tracks,
        vec![
            AreaType::Coordinate,
            AreaType::Coverage(0),
            AreaType::Alignment(0),
            AreaType::AlignmentDivider { upper: 0, lower: 1 },
            AreaType::Coverage(1),
            AreaType::Alignment(1),
            AreaType::Variant(2),
            AreaType::AlignmentDivider { upper: 1, lower: 3 },
            AreaType::Coverage(3),
            AreaType::Alignment(3),
            AreaType::Bed(4),
            AreaType::Sequence,
            AreaType::Console,
            AreaType::Error,
        ]
    );

    harness.app.alignment_view.y = vec![1, 2, 3];
    harness.app.focused_alignment = 2;
    harness.handle(vec![Action::RemoveTrack(1)]).await.unwrap();

    assert_eq!(
        harness.app.dataset.view.messages,
        vec!["Removed ncbi.sorted.bam.".to_string()]
    );
    assert_eq!(
        track_indexes(&harness),
        vec![
            (0, Alignment(0)),
            (2, Variant(0)),
            (3, Alignment(1)),
            (4, Bed(0)),
        ]
    );
    assert_eq!(harness.app.alignment_view.y, vec![1, 3]);
    assert_eq!(harness.app.focused_alignment, 1);
    assert_eq!(harness.app.dataset.view.alignments.len(), 2);
    let layout_tracks = &harness.app.layout.tracks;
    assert!(!layout_tracks.contains(&AreaType::Alignment(1)));
    assert!(layout_tracks.contains(&AreaType::AlignmentDivider { upper: 0, lower: 3 }));
    assert_eq!(harness.app.layout.track_heights.len(), layout_tracks.len());

    let temp_dir = TempDir::new().unwrap();
    let save_path = temp_dir.path().join("runtime-tracks.toml");
    harness
        .handle_command(&format!("w {}", save_path.display()))
        .await
        .unwrap();
    let session = SessionFile::from_path(&save_path).unwrap();
    assert_eq!(session.genome, test_data_path("covid.fa"));
    let saved: Vec<String> = session.tracks.into_iter().map(|track| track.path).collect();
    assert_eq!(
        saved,
        vec![
            test_data_path("covid.sorted.bam"),
            opened[1].clone(),
            opened[2].clone(),
            opened[3].clone(),
        ]
    );

    harness.close().await.unwrap();
}

#[rstest]
#[case::missing_file("open missing.bam", "Not a file: missing.bam")]
#[case::unsupported_type("e notes.txt", "CLI error: Unrecognized file format: notes.txt.")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_track_open_errors_leave_tracks_unchanged(
    #[case] command: &str,
    #[case] message_prefix: &str,
) {
    let mut harness = covid_harness().await;
    harness.handle_command(command).await.unwrap();
    harness.open_pending().await.unwrap();

    assert!(
        harness.app.dataset.view.messages[0].starts_with(message_prefix),
        "{:?}",
        harness.app.dataset.view.messages
    );
    assert_eq!(
        track_indexes(&harness),
        vec![(0, RepositoryFileIndex::Alignment(0))]
    );
    assert_eq!(harness.app.alignment_view.y.len(), 1);

    harness.close().await.unwrap();
}

fn covid_interval(start: u64, end: u64) -> InspectInterval {
    InspectInterval {
        contig: "MN908947.3".to_owned(),
        start,
        end,
    }
}

async fn covid_harness() -> AppHarness {
    let args = offline_case_args(
        Some("covid.sorted.bam"),
        "-g tests/data/covid.fa -r MN908947.3:100 --offline",
    );
    AppHarness::from_args(&args).await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_navigates_and_highlights_the_view() {
    let mut harness = covid_harness().await;
    let session = harness.app.session.clone();

    let view = harness
        .agent(session.navigate(NavigateRequest {
            region: covid_interval(20_000, 20_200),
        }))
        .await
        .unwrap();
    assert!(view.region.start <= 20_000 && view.region.end >= 20_200);
    assert_eq!(harness.app.alignment_view.focus.position, 20_100);
    assert!(harness.app.dataset.view.messages[0].contains("Press u to go back to MN908947.3:100"));

    harness
        .agent(session.highlight(HighlightRequest {
            intervals: vec![covid_interval(20_050, 20_060)],
            label: Some("candidate site".to_owned()),
        }))
        .await
        .unwrap();
    assert_eq!(
        harness.app.highlights,
        [Highlight {
            contig_index: 0,
            start: 20_050,
            end: 20_060,
        }]
    );

    harness.agent(session.clear_highlights()).await.unwrap();
    assert!(harness.app.highlights.is_empty());

    harness.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_queries_leave_the_view_and_cannot_replace_the_dataset() {
    let mut harness = covid_harness().await;
    let session = harness.app.session.clone();
    let focus = harness.app.alignment_view.focus.clone();
    let viewed = harness
        .app
        .alignment_view
        .region(&harness.app.resolved_layout.main_area);
    let viewed_reads = harness.app.dataset.view.alignments[0].tables.reads.height();

    let response = harness
        .agent(session.query(QueryRequest {
            region: Some(covid_interval(20_000, 20_200)),
            sql: "SELECT count(*) AS n FROM reads".to_owned(),
            limit: None,
        }))
        .await
        .unwrap();
    assert_eq!(response.row_count, 1);
    assert_eq!(harness.app.alignment_view.focus, focus);
    assert!(harness.app.dataset.view.alignments[0].has_complete_data(&viewed));
    assert_eq!(
        harness.app.dataset.view.alignments[0].tables.reads.height(),
        viewed_reads
    );

    let request: DatasetRequest =
        serde_json::from_str(r#"{"reference": null, "files": []}"#).unwrap();
    let error = harness
        .agent(session.load_dataset(request))
        .await
        .unwrap_err();
    assert!(matches!(error, SessionError::DatasetFixed));

    harness.close().await.unwrap();
}
