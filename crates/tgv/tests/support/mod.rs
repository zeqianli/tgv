use clap::Parser;
use gv_core::{
    error::TGVError,
    message::{Message as CoreMessage, Movement},
};
use ratatui::{Terminal, backend::TestBackend};
use tgv::{
    app::App,
    message::Message,
    session::SessionFile,
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

        let app = App::new(settings, SessionFile::default_path()).await?;
        let terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let mut harness = Self { app, terminal };
        harness.initialize().await?;
        Ok(harness)
    }

    async fn initialize(&mut self) -> Result<(), TGVError> {
        self.terminal
            .draw(|frame| {
                let _ = self.app.layout.set_area(frame.area());
            })
            .expect("initial layout draw");

        self.app
            .handle(self.app.settings.initial_state_messages.clone())
            .await?;
        self.self_correct()?;
        self.render();
        Ok(())
    }

    pub async fn handle(&mut self, messages: Vec<Message>) -> Result<(), TGVError> {
        self.app.handle(messages).await?;
        self.self_correct()?;
        self.render();
        Ok(())
    }

    pub async fn handle_core(&mut self, messages: Vec<CoreMessage>) -> Result<(), TGVError> {
        self.handle(messages.into_iter().map(Message::Core).collect())
            .await
    }

    pub async fn handle_movement(&mut self, movement: Movement) -> Result<(), TGVError> {
        self.handle_core(vec![CoreMessage::Move(movement)]).await
    }

    pub fn locus(&self) -> String {
        self.app
            .alignment_view
            .focus
            .to_locus_str(&self.app.state.contig_header)
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
            .state
            .contig_length(&self.app.alignment_view.focus)?;
        self.app
            .alignment_view
            .self_correct(&self.app.layout.main_area, contig_length);
        Ok(())
    }

    fn render(&mut self) {
        self.terminal
            .draw(|frame| {
                let buffer = frame.buffer_mut();
                let _ = self.app.layout.set_area(buffer.area);
                self.app.render(buffer).expect("render");
            })
            .expect("terminal render");
    }
}
