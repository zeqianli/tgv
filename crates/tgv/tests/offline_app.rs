mod support;

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use gv_core::message::{
    AlignmentDisplayOption, AlignmentSort, Message as CoreMessage, Movement, Scroll, Zoom,
};
use rstest::rstest;
use support::{AppHarness, test_data_path};
use tempfile::TempDir;
use tgv::{
    app::Scene,
    message::{Message, UpdateLayoutMessage},
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
            CoreMessage::Scroll(Scroll::Down { index: 0, n: 2 }),
            CoreMessage::Zoom(Zoom::Out(4)),
            CoreMessage::Move(Movement::Position(33121140)),
            CoreMessage::Scroll(Scroll::Up { index: 0, n: 1 }),
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
    assert!(harness.app.state.messages.is_empty());

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
            .map(|(_, label)| label.as_str())
            .collect::<Vec<_>>(),
        vec!["ncbi.sorted.bam", "simple.vcf", "simple.bed"],
    );
    let label_area = harness.app.resolved_layout.sidebar_labels[0].0;
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
            &harness.app.state,
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
            &harness.app.state,
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
        .handle(vec![Message::UpdateLayout(
            UpdateLayoutMessage::SetSidebarWidth(6),
        )])
        .await
        .unwrap();
    let vcf_area = harness.app.resolved_layout.sidebar_labels[1].0;
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
    let bed_area = harness.app.resolved_layout.sidebar_labels[2].0;
    assert!(bed_area.height >= 2);

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
    assert_eq!(harness.app.state.variant_loaded, vec![true]);
    assert_eq!(harness.app.state.bed_loaded, vec![true]);
    assert_eq!(harness.app.alignment_view.focus.position, 33_121_130);
    assert_eq!(
        harness.app.state.messages,
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
    let initial_messages = harness.app.state.messages.clone();

    harness.handle_command("sort base").await.unwrap();

    assert_eq!(
        harness.app.state.alignment_options[0],
        vec![AlignmentDisplayOption::Sort(AlignmentSort::BaseAt(
            sort_position
        ))]
    );
    assert_eq!(harness.app.state.messages, initial_messages);
    assert!(harness.app.state.alignments[0].depth().unwrap() > 0);

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
