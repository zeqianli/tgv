/// The main app object
///
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{buffer::Buffer, layout::Rect, prelude::Backend, widgets::Widget, Frame, Terminal};

use crate::error::TGVError;
use crate::register::{Register, Registers};
use crate::rendering::layout::MouseRegister;
use crate::rendering::RenderingState;
use crate::repository::Repository;
use crate::settings::Settings;
use crate::states::{State, StateHandler};
pub struct App {
    pub state: State, // Holds all states and data

    pub settings: Settings,

    pub repository: Repository, // Data CRUD interface

    pub registers: Registers, // Controls key event translation to StateMessages. Uses the State pattern.

    pub mouse_register: MouseRegister,

    pub rendering_state: RenderingState,
}

// initialization
impl App {
    pub async fn new<B: Backend>(
        settings: Settings,
        terminal: &mut Terminal<B>,
    ) -> Result<Self, TGVError> {
        // Gather resources before initializing the state.

        let (repository, sequence_cache, mut track_cache, contig_header) =
            Repository::new(&settings).await?;

        let mut state = State::new(
            &settings,
            terminal.get_frame().area(),
            sequence_cache,
            track_cache,
            contig_header,
        )?;

        // Find the initial window
        StateHandler::handle_initial_messages(&mut state, &repository, &settings, Vec::new())
            .await?;

        let mouse_register = MouseRegister::new(&state.layout.root);

        Ok(Self {
            state,
            settings: settings.clone(),
            //state_handler: StateHandler::new(&settings).await?,
            repository,
            registers: Registers::new()?,
            mouse_register,
            rendering_state: RenderingState::new(),
        })
    }
}

impl App {
    /// Main loop
    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), TGVError> {
        while !self.state.exit {
            self.registers.update_state(&self.state)?;

            // Prepare rendering
            self.rendering_state.update(&self.state)?;

            if self.rendering_state.needs_refresh() {
                let _ = terminal.clear();
            }

            // Render

            terminal
                .draw(|frame| {
                    self.draw(frame);
                })
                .unwrap();

            if self.settings.test_mode {
                break;
            }

            // handle events
            match event::read() {
                Ok(Event::Key(key_event)) if key_event.kind == KeyEventKind::Press => {
                    let state_messages = self.registers.update_key_event(key_event, &self.state)?;
                    StateHandler::handle(
                        &mut self.state,
                        &self.repository,
                        &self.settings,
                        state_messages,
                    )
                    .await?;
                }

                Ok(Event::Mouse(mouse_event)) => {
                    let ui_message = self
                        .mouse_register
                        .handle_mouse_event(&self.state.layout.root, mouse_event)?;
                    let area = self.state.area;
                    if let Some(ui_message) = ui_message {
                        self.mouse_register.handle_ui_message(
                            &mut self.state.layout,
                            area,
                            ui_message,
                        )?;
                    }
                }

                Ok(Event::Resize(_width, _height)) => {
                    self.state.self_correct_viewing_window();
                }

                _ => {}
            }
        }
        Ok(())
    }

    /// Draw the app
    pub fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, self.state.area);
    }

    /// close connections
    pub async fn close(&mut self) -> Result<(), TGVError> {
        self.repository.close().await?;
        Ok(())
    }
}
const MIN_AREA_WIDTH: u16 = 10;
const MIN_AREA_HEIGHT: u16 = 6;
impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.rendering_state
            .render(
                area,
                buf,
                &self.state,
                &self.registers,
                &self.repository,
                &self.settings.palette,
            )
            .unwrap()
    }
}

/*
Saving some initial window logics here:

    fn go_to_contig_coordinate(
        state: &mut State,
        contig_str: &str,
        n: usize,
    ) -> Result<(), TGVError> {
        // If bam_path is provided, check that the contig is valid.

        if let Some(contig) = state.contigs.get_contig_by_str(contig_str) {
            let current_frame_area = *state.current_frame_area()?;

            match state.window {
                Some(ref mut window) => {
                    window.contig = contig;
                    window.set_middle(&current_frame_area, n, None); // Don't know contig length yet.
                    window.set_top(0);
                }
                None => {
                    state.window = Some(ViewingWindow::new_basewise_window(contig, n, 0));
                }
            }
            Ok(())
        } else {
            Err(TGVError::StateError(format!(
                "Contig {:?} not found for reference {:?}",
                contig_str, state.reference
            )))
        }
    }


*/
