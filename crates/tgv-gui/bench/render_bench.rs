//! Headless timing harness for the window's render and input paths.
//!
//! Usage: `cargo bench -p tgv-gui --bench render_bench -- <bam> <region> <reference> [iterations]
//! [scenario] [bases_per_pixel]`. Pass `none` as the reference for `--no-reference`. Scenarios
//! are `all` (default), `prepare`, `snapshot`, `render`, `pan`, `scroll`, `zoom`, and `hover`.
//!
//! The window runs on gpui's test platform, which lays out and paints every element but
//! doesn't shape or rasterize text, so text costs are left out. Data loads happen on the worker
//! before timing starts, and the views stay within the loaded region.
//!
//! Cargo runs benchmarks from the package directory, so pass absolute file paths. The release
//! profile strips symbols, so keep them for flame graphs:
//!
//! ```text
//! CARGO_PROFILE_BENCH_STRIP=false CARGO_PROFILE_BENCH_DEBUG=true \
//!     cargo flamegraph -p tgv-gui --bench render_bench -o render.svg -- \
//!     $PWD/test/output/HG002.GRCh38.300x_chr20.bam chr20:88000 hg38 2000 render
//! ```

use clap::Parser;
use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ScrollDelta,
    ScrollWheelEvent, TestAppContext, TouchPhase, VisualTestContext, point, px,
};
use gv_core::message::AlignmentDisplayOption;
use gv_viewer::prepare::{alignment::AlignmentScene, track::Gene};
use std::time::{Duration, Instant};
use tgv_gui::{
    app::App,
    layout::{ALIGNMENT_ROW_HEIGHT, AreaType, PixelView, Rect},
    settings::{Cli, Settings},
    worker::WorkerHandle,
};

fn report(name: &str, samples: &mut [Duration]) {
    samples.sort();
    let total: Duration = samples.iter().sum();
    let mean = total / samples.len() as u32;
    let p50 = samples[samples.len() / 2];
    let p95 = samples[(samples.len() * 95 / 100).min(samples.len() - 1)];
    println!(
        "{name:<28} n={:<5} mean={:>9.3?} p50={:>9.3?} p95={:>9.3?}",
        samples.len(),
        mean,
        p50,
        p95
    );
}

struct Bench<'a> {
    app: Entity<App>,
    cx: &'a mut VisualTestContext,
}

impl Bench<'_> {
    /// Redraws the window and runs the work it queues, as a frame does.
    fn draw(&mut self) {
        self.cx.update(|window, _| window.refresh());
        self.cx.run_until_parked();
    }

    /// Waits until the worker answered every message, and the window drew its last frame.
    fn settle(&mut self) {
        let started = Instant::now();
        loop {
            self.draw();
            if self.cx.update(|_, cx| self.app.read(cx).caught_up()) {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(120),
                "the worker answers"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        self.draw();
    }

    fn alignment_area(&mut self) -> Rect {
        self.cx.update(|_, cx| {
            self.app
                .read(cx)
                .resolved_layout
                .as_ref()
                .expect("a layout")
                .areas
                .iter()
                .find_map(|(area, rect)| matches!(area, AreaType::Alignment(_)).then_some(*rect))
                .expect("an alignment area")
        })
    }

    fn event<E: gpui::InputEvent>(&mut self, event: E) {
        self.cx.simulate_event(event);
    }
}

fn mouse_move(x: f32, y: f32, button: Option<MouseButton>) -> MouseMoveEvent {
    MouseMoveEvent {
        position: point(px(x), px(y)),
        pressed_button: button,
        modifiers: Modifiers::default(),
    }
}

fn scroll(x: f32, y: f32, dx: f32, dy: f32, zoom: bool) -> ScrollWheelEvent {
    ScrollWheelEvent {
        position: point(px(x), px(y)),
        delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
        modifiers: Modifiers {
            control: zoom,
            ..Modifiers::default()
        },
        touch_phase: TouchPhase::Moved,
    }
}

fn main() {
    // `cargo bench` appends `--bench` to the arguments of benchmarks without the libtest harness.
    let args: Vec<String> = std::env::args().filter(|arg| arg != "--bench").collect();
    let bam = &args[1];
    let region = &args[2];
    let reference = &args[3];
    let iterations: usize = args.get(4).map_or(200, |n| n.parse().expect("iterations"));
    let scenario = args.get(5).map_or("all", String::as_str);
    let bases_per_pixel: Option<f64> = args.get(6).map(|zoom| zoom.parse().expect("zoom"));
    let run = |name: &str| scenario == "all" || scenario == name;

    let mut cli_args = vec!["tgv-gui", bam.as_str(), "-r", region.as_str()];
    if reference == "none" {
        cli_args.push("--no-reference");
    } else {
        cli_args.extend(["-g", reference.as_str()]);
    }
    let mut settings: Settings = Cli::parse_from(cli_args).try_into().expect("settings");
    settings.zoom = bases_per_pixel;

    let cx = TestAppContext::single();
    cx.background_executor.allow_parking();
    let started = Instant::now();
    let (worker, events) = WorkerHandle::spawn(settings.clone());
    let mut cx = cx;
    let (app, cx) = cx.add_window_view(|_, cx| App::new(settings, worker, events, cx));
    cx.update(|window, cx| window.focus(&app.read(cx).focus_handle));
    let mut bench = Bench { app, cx };
    bench.settle();
    let area = bench.alignment_area();
    let (reads, runs, bpp, rows) = bench.cx.update(|_, cx| {
        let frame = bench.app.read(cx).frame.clone().expect("a frame");
        let alignment = &frame.state.alignments[0];
        (
            alignment.tables.reads.height(),
            alignment.tables.cigar_runs.height(),
            frame.viewport.zoom.bases_per_pixel,
            (area.height / ALIGNMENT_ROW_HEIGHT).ceil() as usize,
        )
    });
    println!(
        "initial load + first frame: {:?} (reads={reads} runs={runs} bases_per_pixel={bpp} track_width={} rows={rows})",
        started.elapsed(),
        area.width,
    );

    if run("prepare") {
        // The screen-independent work a redraw does today: the alignment scene, the genes, and
        // the coverage, with nothing drawn.
        let (frame, view) = bench.cx.update(|_, cx| {
            let frame = bench.app.read(cx).frame.clone().expect("a frame");
            let view = PixelView {
                viewport: frame.viewport.clone(),
                offset: 0.0,
            };
            (frame, view)
        });
        let region = view.region(&area);
        let paired = frame.state.alignment_options[0]
            .contains(&AlignmentDisplayOption::ViewAsPairs)
            .then(|| frame.state.paired_alignments[0].as_ref().expect("pairs"));
        let mut samples = Vec::with_capacity(iterations);
        let mut size = 0;
        for _ in 0..iterations {
            let started = Instant::now();
            let scene = AlignmentScene::new(&frame.state.alignments[0], paired, &region, 0, rows)
                .expect("scene");
            samples.push(started.elapsed());
            size = scene.bodies.len() + scene.mismatches.len() + scene.soft_clips.len();
        }
        report(&format!("alignment scene ({size} items)"), &mut samples);
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let started = Instant::now();
            let _ = Gene::in_region(&frame.state, &region).expect("genes");
            samples.push(started.elapsed());
        }
        report("genes", &mut samples);
        let (left, right) = view.visible(&area);
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let started = Instant::now();
            let _ = frame.state.alignments[0]
                .coverage
                .query(left, right)
                .expect("coverage");
            samples.push(started.elapsed());
        }
        report("coverage query", &mut samples);
    }

    if run("snapshot") {
        // The worker clones the view state into every frame it sends.
        let frame = bench
            .cx
            .update(|_, cx| bench.app.read(cx).frame.clone().expect("a frame"));
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let started = Instant::now();
            let state = frame.state.clone();
            samples.push(started.elapsed());
            drop(state);
        }
        report("state clone", &mut samples);
    }

    if run("render") {
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let started = Instant::now();
            bench.draw();
            samples.push(started.elapsed());
        }
        report("redraw", &mut samples);
    }

    let (x, y) = (area.x + area.width / 2.0, area.y + area.height / 3.0);

    if run("pan") {
        // A drag that oscillates by 4 pixels a move, so the view stays inside the loaded
        // region. Each move is handled and drawn, as at 60 frames a second.
        bench.event(MouseDownEvent {
            button: MouseButton::Left,
            position: point(px(x), px(y)),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        });
        let mut position = x;
        let mut samples = Vec::with_capacity(iterations);
        for i in 0..iterations {
            position += if (i / 50) % 2 == 0 { 4.0 } else { -4.0 };
            let started = Instant::now();
            bench.event(mouse_move(position, y, Some(MouseButton::Left)));
            bench.draw();
            samples.push(started.elapsed());
        }
        bench.event(MouseUpEvent {
            button: MouseButton::Left,
            position: point(px(position), px(y)),
            modifiers: Modifiers::default(),
            click_count: 1,
        });
        report("drag 4px + redraw", &mut samples);
        bench.settle();
    }

    if run("scroll") {
        let mut samples = Vec::with_capacity(iterations);
        for i in 0..iterations {
            let dy = if (i / 20) % 2 == 0 { -12.0 } else { 12.0 };
            let started = Instant::now();
            bench.event(scroll(x, y, 0.0, dy, false));
            bench.draw();
            samples.push(started.elapsed());
        }
        report("scroll 1 row + redraw", &mut samples);
        bench.settle();
    }

    if run("zoom") {
        // Small steps around the pointer, as a pinch sends them.
        let mut samples = Vec::with_capacity(iterations);
        for i in 0..iterations {
            let dy = if (i / 25) % 2 == 0 { 2.0 } else { -2.0 };
            let started = Instant::now();
            bench.event(scroll(x, y, 0.0, dy, true));
            bench.draw();
            samples.push(started.elapsed());
        }
        report("zoom step + redraw", &mut samples);
        bench.settle();
    }

    if run("hover") {
        let mut samples = Vec::with_capacity(iterations);
        for i in 0..iterations {
            let position = area.x + (i % area.width as usize) as f32;
            let started = Instant::now();
            bench.event(mouse_move(position, y, None));
            bench.draw();
            samples.push(started.elapsed());
        }
        report("hover move + redraw", &mut samples);
    }
}
