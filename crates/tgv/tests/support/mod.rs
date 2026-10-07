use clap::Parser;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gv_core::{
    error::TGVError,
    message::{Message as CoreMessage, Movement},
};
use ratatui::{Terminal, backend::TestBackend};
use tgv::{
    app::{App, RenderEvent},
    message::Action,
    settings::{Cli, Settings},
};

pub fn test_data_path(path: &str) -> String {
    format!("{}/tests/data/{path}", env!("CARGO_MANIFEST_DIR"))
}

pub fn cli_from_args(args: &str) -> Cli {
    Cli::parse_from(shlex::split(&format!("tgv {args}")).expect("valid test arguments"))
}

pub struct AppHarness {
    pub app: App,
    terminal: Terminal<TestBackend>,
}

impl AppHarness {
    pub async fn from_args(args: &str) -> Result<Self, TGVError> {
        let cli = cli_from_args(args);
        let mut settings: Settings = cli.try_into()?;
        settings.test_mode = true;

        let app = App::new(settings).await?;
        let terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let mut harness = Self { app, terminal };
        harness.initialize().await?;
        Ok(harness)
    }

    async fn initialize(&mut self) -> Result<(), TGVError> {
        self.terminal
            .draw(|frame| {
                self.app.resolved_layout = self
                    .app
                    .layout
                    .resolve(frame.area(), &self.app.dataset.repository);
            })
            .expect("initial layout draw");

        self.app
            .handle(self.app.settings.initial_actions.clone())
            .await?;
        self.self_correct()?;
        self.render(&vec![RenderEvent::All]);
        Ok(())
    }

    pub async fn handle(&mut self, messages: Vec<Action>) -> Result<(), TGVError> {
        let render_events = self.app.handle(messages).await?;
        self.self_correct()?;
        self.render(&render_events);
        Ok(())
    }

    pub async fn handle_core(&mut self, messages: Vec<CoreMessage>) -> Result<(), TGVError> {
        self.handle(messages.into_iter().map(Action::Core).collect())
            .await
    }

    pub async fn handle_key_codes(
        &mut self,
        key_codes: impl IntoIterator<Item = KeyCode>,
    ) -> Result<(), TGVError> {
        let mut render_events = Vec::new();
        for key_code in key_codes {
            let messages = self.app.registers.handle_key_event(
                KeyEvent::new(key_code, KeyModifiers::NONE),
                &self.app.dataset.view,
            )?;
            render_events.extend(self.app.handle(messages).await?);
        }
        self.self_correct()?;
        self.render(&render_events);
        Ok(())
    }

    pub async fn handle_command(&mut self, command: &str) -> Result<(), TGVError> {
        let mut key_codes = Vec::with_capacity(command.len() + 2);
        key_codes.push(KeyCode::Char(':'));
        key_codes.extend(command.chars().map(KeyCode::Char));
        key_codes.push(KeyCode::Enter);
        self.handle_key_codes(key_codes).await
    }

    pub async fn handle_movement(&mut self, movement: Movement) -> Result<(), TGVError> {
        self.handle_core(vec![CoreMessage::Move(movement)]).await
    }

    /// Sends a session request the way an agent does, serves it in the app, and redraws.
    pub async fn agent<T>(&mut self, request: impl Future<Output = T>) -> T {
        let (result, render_events) = tokio::join!(request, self.app.serve_next_request());
        self.self_correct()
            .expect("self-correct after an agent request");
        self.render(&render_events);
        result
    }

    pub fn locus(&self) -> String {
        self.app
            .alignment_view
            .focus
            .to_locus_str(&self.app.dataset.view.contig_header)
            .expect("focus to locus")
    }

    pub fn terminal_backend(&self) -> &TestBackend {
        self.terminal.backend()
    }

    pub async fn close(self) -> Result<(), TGVError> {
        self.app.close().await
    }

    fn self_correct(&mut self) -> Result<(), TGVError> {
        let contig_length = self
            .app
            .dataset
            .view
            .contig_length(&self.app.alignment_view.focus)?;
        self.app
            .alignment_view
            .self_correct(&self.app.resolved_layout.main_area, contig_length);
        Ok(())
    }

    fn render(&mut self, render_events: &Vec<RenderEvent>) {
        self.terminal
            .draw(|frame| {
                let buffer = frame.buffer_mut();
                self.app.render(buffer, render_events).expect("render");
            })
            .expect("terminal render");
    }
}
