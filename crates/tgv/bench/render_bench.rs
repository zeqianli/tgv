//! Headless timing harness for the interactive render and input paths.
//!
//! Usage: `cargo bench -p tgv --bench render_bench -- <bam> <region> <reference> [iterations] [scenario] [zoom]`.
//! Scenarios are `all` (default), `load`, `render`, `scroll`, `pan`, `hover`, `burst`, and `paired`.
//!
//! Cargo runs benchmarks from the package directory, so pass absolute file paths.
//!
//! A single scenario with many iterations is convenient to profile. The release profile strips
//! symbols, so keep them for flame graphs:
//!
//! ```text
//! CARGO_PROFILE_BENCH_STRIP=false CARGO_PROFILE_BENCH_DEBUG=true \
//!     cargo flamegraph -p tgv --bench render_bench -o render.svg -- \
//!     $PWD/test/output/HG002.GRCh38.300x_chr20.bam chr20:88000 hg38 3000 render
//! ```

use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
use gv_core::{
    intervals::GenomeInterval,
    message::{AlignmentDisplayOption, Message as CoreMessage, Movement, Scroll},
    state::CachePolicy,
};
use ratatui::{buffer::Buffer, layout::Rect};
use std::time::{Duration, Instant};
use tgv::{
    app::{App, RenderEvent},
    layout::AreaType,
    message::Action,
    settings::{Cli, Settings},
};

use clap::Parser;

const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;

struct Bench {
    app: App,
    buffer: Buffer,
}

impl Bench {
    fn render(&mut self, events: &Vec<RenderEvent>) {
        self.app.render(&mut self.buffer, events).expect("render");
    }

    fn self_correct(&mut self) {
        let contig_length = self
            .app
            .dataset
            .view
            .contig_length(&self.app.alignment_view.focus)
            .expect("contig length");
        self.app
            .alignment_view
            .self_correct(&self.app.resolved_layout.main_area, contig_length);
    }

    async fn handle(&mut self, messages: Vec<CoreMessage>) -> Vec<RenderEvent> {
        let events = self
            .app
            .handle(messages.into_iter().map(Action::Core).collect())
            .await
            .expect("handle");
        self.self_correct();
        events
    }

    fn alignment_area(&self) -> Rect {
        self.app
            .resolved_layout
            .areas
            .iter()
            .find_map(|(area_type, rect)| {
                matches!(area_type, AreaType::Alignment(_)).then_some(*rect)
            })
            .expect("an alignment area")
    }
}

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

#[tokio::main]
async fn main() {
    // `cargo bench` appends `--bench` to the arguments of benchmarks without the libtest harness.
    let args: Vec<String> = std::env::args().filter(|arg| arg != "--bench").collect();
    let bam = &args[1];
    let region = &args[2];
    let reference = &args[3];
    let iterations: usize = args.get(4).map_or(200, |n| n.parse().expect("iterations"));
    let scenario = args.get(5).map_or("all", String::as_str);
    let zoom: Option<u64> = args.get(6).map(|zoom| zoom.parse().expect("zoom"));
    let run = |name: &str| scenario == "all" || scenario == name;

    let cli = Cli::parse_from(["tgv", bam, "-r", region, "-g", reference]);
    let mut settings: Settings = cli.try_into().expect("settings");
    settings.test_mode = true;
    settings.zoom = zoom;

    let started = Instant::now();
    let app = App::new(settings).await.expect("app");
    let mut bench = Bench {
        app,
        buffer: Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT)),
    };
    bench.app.resolved_layout = bench
        .app
        .layout
        .resolve(bench.buffer.area, &bench.app.dataset.repository);
    let initial = bench.app.settings.initial_actions.clone();
    bench.app.handle(initial).await.expect("initial load");
    bench.self_correct();
    bench.render(&vec![RenderEvent::All]);
    println!(
        "initial load + first render: {:?} (reads={} runs={} depth={})",
        started.elapsed(),
        bench.app.dataset.view.alignments[0].tables.reads.height(),
        bench.app.dataset.view.alignments[0]
            .tables
            .cigar_runs
            .height(),
        bench.app.dataset.view.alignments[0].depth().expect("depth"),
    );

    if run("load") {
        // Reload the alignment cache window: BAM query, table building, stacking, coverage, and
        // display options. The reference sequence is already cached, so no network is involved.
        let region = CachePolicy::VIEWER.alignment_region(
            &bench
                .app
                .alignment_view
                .region(&bench.app.resolved_layout.main_area),
        );
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let started = Instant::now();
            bench
                .app
                .dataset
                .view
                .load_alignment_data(
                    0,
                    &region,
                    &mut bench.app.dataset.repository.alignment_repositories[0],
                )
                .await
                .expect("load");
            samples.push(started.elapsed());
        }
        println!("cache window: {} bp", region.end() - region.start() + 1);
        report("alignment reload", &mut samples);
    }

    if run("render") {
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let started = Instant::now();
            bench.render(&vec![RenderEvent::All]);
            samples.push(started.elapsed());
        }
        report("full render", &mut samples);
    }

    if run("scroll") {
        let mut samples = Vec::with_capacity(iterations);
        for i in 0..iterations {
            let scroll = if (i / 20) % 2 == 0 {
                Scroll::Down { index: 0, n: 1 }
            } else {
                Scroll::Up { index: 0, n: 1 }
            };
            let started = Instant::now();
            let events = bench.handle(vec![CoreMessage::Scroll(scroll)]).await;
            bench.render(&events);
            samples.push(started.elapsed());
        }
        report("scroll 1 row + render", &mut samples);
    }

    if run("pan") {
        // Oscillate so the view stays inside the cached region and only rendering is measured.
        let mut samples = Vec::with_capacity(iterations);
        for i in 0..iterations {
            let movement = if (i / 20) % 2 == 0 {
                Movement::Right(1)
            } else {
                Movement::Left(1)
            };
            let started = Instant::now();
            let events = bench.handle(vec![CoreMessage::Move(movement)]).await;
            bench.render(&events);
            samples.push(started.elapsed());
        }
        report("pan 1 base + render", &mut samples);
    }

    if run("hover") {
        // Two motion reports per cell, as terminals may emit for sub-cell motion.
        let area = bench.alignment_area();
        let mut samples = Vec::with_capacity(iterations);
        for i in 0..iterations {
            let column = area.x + ((i / 2) as u16 % area.width);
            let row = area.y + ((i / 2) as u16 / area.width) % area.height;
            let event = MouseEvent {
                kind: MouseEventKind::Moved,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            };
            let started = Instant::now();
            let messages = bench
                .app
                .mouse_register
                .handle_mouse_event(
                    &bench.app.dataset.view,
                    &bench.app.resolved_layout,
                    &bench.app.alignment_view,
                    event,
                )
                .expect("mouse");
            let events = if messages.is_empty() {
                Vec::new()
            } else {
                bench.app.handle(messages).await.expect("handle")
            };
            if !events.is_empty() {
                bench.render(&events);
            }
            samples.push(started.elapsed());
        }
        report("hover move (+ render)", &mut samples);
    }

    if run("burst") {
        // A burst of ten queued key-repeat pans, rendered per event versus once per burst.
        const BURST: usize = 10;
        let mut separate = Vec::with_capacity(iterations);
        let mut coalesced = Vec::with_capacity(iterations);
        for i in 0..iterations {
            let movement = if i % 2 == 0 {
                Movement::Right(1)
            } else {
                Movement::Left(1)
            };
            let started = Instant::now();
            for _ in 0..BURST {
                let events = bench
                    .handle(vec![CoreMessage::Move(movement.clone())])
                    .await;
                bench.render(&events);
            }
            separate.push(started.elapsed());

            let started = Instant::now();
            let mut events = Vec::new();
            for _ in 0..BURST {
                events.extend(
                    bench
                        .handle(vec![CoreMessage::Move(movement.clone())])
                        .await,
                );
            }
            bench.render(&events);
            coalesced.push(started.elapsed());
        }
        report("10-pan burst, per event", &mut separate);
        report("10-pan burst, coalesced", &mut coalesced);
    }

    if run("paired") {
        bench
            .handle(vec![CoreMessage::SetAlignmentOption(vec![
                AlignmentDisplayOption::ViewAsPairs,
            ])])
            .await;
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let started = Instant::now();
            bench.render(&vec![RenderEvent::All]);
            samples.push(started.elapsed());
        }
        report("full render (pairs)", &mut samples);
    }

    bench.app.close().await.expect("close");
}
