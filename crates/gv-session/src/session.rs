//! The session actor: one worker owns the loaded dataset and serves commands in order.

use crate::{
    error::SessionError,
    schema::*,
    tables::{QueryRegion, TableSources},
};
use gv_core::{prelude::*, settings::Settings};
use std::sync::Arc;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

type Reply<T> = oneshot::Sender<Result<T, SessionError>>;

/// A request to the session worker, carrying the channel for its typed reply.
enum Command {
    Describe(Reply<Option<DatasetDescription>>),
    LoadDataset(DatasetRequest, Reply<DatasetDescription>),
    Inspect(InspectRequest, Reply<InspectResponse>),
    Query(QueryRequest, Reply<QueryResponse>),
    Shutdown,
}

impl Command {
    /// Whether the caller stopped waiting, so the work can be skipped.
    fn is_abandoned(&self) -> bool {
        match self {
            Self::Describe(reply) => reply.is_closed(),
            Self::LoadDataset(_, reply) => reply.is_closed(),
            Self::Inspect(_, reply) => reply.is_closed(),
            Self::Query(_, reply) => reply.is_closed(),
            Self::Shutdown => false,
        }
    }
}

/// Sends commands to a session worker. Clones address the same session.
#[derive(Clone)]
pub struct SessionHandle {
    sender: mpsc::Sender<Command>,
}

impl SessionHandle {
    /// Describes the loaded dataset, or returns `None` before the first load.
    pub async fn describe(&self) -> Result<Option<DatasetDescription>, SessionError> {
        self.request(Command::Describe).await
    }

    /// Loads or replaces the dataset. A failed load leaves the previous dataset in place.
    pub async fn load_dataset(
        &self,
        request: DatasetRequest,
    ) -> Result<DatasetDescription, SessionError> {
        self.request(|reply| Command::LoadDataset(request, reply))
            .await
    }

    /// Summarizes an interval.
    pub async fn inspect(&self, request: InspectRequest) -> Result<InspectResponse, SessionError> {
        self.request(|reply| Command::Inspect(request, reply)).await
    }

    /// Runs a read-only SQL query.
    pub async fn query(&self, request: QueryRequest) -> Result<QueryResponse, SessionError> {
        self.request(|reply| Command::Query(request, reply)).await
    }

    /// Asks the worker to close the dataset and stop after earlier commands finish.
    pub async fn shutdown(&self) {
        // A worker that already stopped needs no shutdown.
        let _ = self.sender.send(Command::Shutdown).await;
    }

    async fn request<T>(
        &self,
        command: impl FnOnce(Reply<T>) -> Command,
    ) -> Result<T, SessionError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(command(reply))
            .await
            .map_err(|_| SessionError::Unavailable)?;
        response.await.map_err(|_| SessionError::Stopped)?
    }
}

/// Starts session workers.
pub struct Session;

impl Session {
    const QUEUE: usize = 16;

    /// Starts a worker on the current Tokio runtime and returns its handle and join handle.
    ///
    /// `defaults` provides the backend, cache, and host settings for loaded datasets.
    pub fn spawn(defaults: Settings) -> (SessionHandle, JoinHandle<Result<(), TGVError>>) {
        let (sender, receiver) = mpsc::channel(Self::QUEUE);
        // Synchronous readers and coverage work must not block the caller's runtime threads.
        // Construct the dataset inside this thread because repository types need not be Send.
        let runtime = tokio::runtime::Handle::current();
        let worker =
            tokio::task::spawn_blocking(move || runtime.block_on(Dataset::run(defaults, receiver)));
        (SessionHandle { sender }, worker)
    }
}

/// Owns the loaded dataset and the state that commands read and update.
struct Dataset {
    settings: Settings,
    state: State,
    repository: Repository,
    tracks: Arc<TrackRegistry>,
}

impl Dataset {
    /// Processes commands sequentially and closes the dataset when the worker exits.
    async fn run(
        defaults: Settings,
        mut receiver: mpsc::Receiver<Command>,
    ) -> Result<(), TGVError> {
        let mut dataset: Option<Self> = None;
        while let Some(command) = receiver.recv().await {
            if command.is_abandoned() {
                continue;
            }
            // Callers that stop waiting after the check above drop their replies harmlessly.
            match command {
                Command::Shutdown => break,
                Command::Describe(reply) => {
                    let _ = reply.send(Ok(dataset.as_ref().map(Self::description)));
                }
                Command::LoadDataset(request, reply) => {
                    let _ = reply.send(Self::replace(&mut dataset, &defaults, request).await);
                }
                Command::Inspect(request, reply) => {
                    let result = match dataset.as_mut() {
                        Some(dataset) => dataset.inspect(request).await,
                        None => Err(SessionError::NoDataset {
                            operation: "inspecting",
                        }),
                    };
                    let _ = reply.send(result);
                }
                Command::Query(request, reply) => {
                    let result = match dataset.as_mut() {
                        Some(dataset) => dataset.query(request).await,
                        None => Err(SessionError::NoDataset {
                            operation: "querying",
                        }),
                    };
                    let _ = reply.send(result);
                }
            }
        }

        if let Some(mut dataset) = dataset {
            dataset.repository.close().await?;
        }

        Ok(())
    }

    /// Describes the current reference and tracks.
    fn description(&self) -> DatasetDescription {
        DatasetDescription {
            reference: self.settings.reference.to_string(),
            tracks: self
                .tracks
                .entries
                .iter()
                .map(|entry| TrackDescription {
                    id: entry.id,
                    r#type: entry.repository_index.into(),
                    source: self.repository.file_path(entry.repository_index).to_owned(),
                })
                .collect(),
        }
    }

    /// Builds a replacement dataset before swapping it into the worker.
    async fn replace(
        current: &mut Option<Self>,
        defaults: &Settings,
        request: DatasetRequest,
    ) -> Result<DatasetDescription, SessionError> {
        let prior_settings = current
            .as_ref()
            .map_or(defaults, |dataset| &dataset.settings);
        let new_settings = request.update_settings(prior_settings)?;
        let replacement =
            Self::new(new_settings)
                .await
                .map_err(|error| SessionError::InvalidInput {
                    field: "files",
                    message: error.to_string(),
                })?;
        let description = replacement.description();
        if let Some(mut old) = current.replace(replacement) {
            if let Err(error) = old.repository.close().await {
                log::warn!("Failed to close the replaced dataset: {error}");
            }
        }
        Ok(description)
    }

    /// Loads the repositories and initializes one state slot per track.
    async fn new(settings: Settings) -> Result<Self, TGVError> {
        let (repository, contigs, file_indexes) = Repository::new(&settings).await?;
        let mut state = State::new(settings.reference.clone(), contigs)?;
        for file in &settings.file_paths {
            match file {
                FilePath::AlignmentPath(_) => state.add_alignment_track(),
                FilePath::VariantPath(_) => state.add_variant_track(),
                FilePath::BedPath(_) => state.add_bed_track(),
            }
        }
        let tracks = Arc::new(TrackRegistry::new(&file_indexes));
        Ok(Self {
            state,
            repository,
            tracks,
            settings,
        })
    }

    /// Loads the selected tracks, the reference sequence, and gene annotations for a region.
    ///
    /// Data that is already complete for the region is reused. Requests can span 100,000 bases
    /// at high depth, so loads cover exactly the region instead of padding it like the viewer.
    async fn load_region(
        &mut self,
        region: &Region,
        track_ids: &[TrackId],
    ) -> Result<(), TGVError> {
        let files: Vec<RepositoryFileIndex> = track_ids
            .iter()
            .map(|&id| self.tracks.get(id).repository_index)
            .collect();
        self.state
            .ensure_loaded(
                region,
                &LoadRequest {
                    sequence: true,
                    genes: true,
                    files: &files,
                    cache: CachePolicy::EXACT,
                },
                &mut self.repository,
            )
            .await
    }

    /// Validates requested track IDs and returns them in dataset order.
    fn selected_tracks(&self, tracks: Option<&[TrackId]>) -> Result<Vec<TrackId>, SessionError> {
        if let Some(tracks) = tracks {
            if tracks.is_empty()
                || tracks
                    .iter()
                    .enumerate()
                    .any(|(i, id)| tracks[..i].contains(id) || *id >= self.tracks.entries.len())
            {
                return Err(SessionError::InvalidInput {
                    field: "tracks",
                    message: "Select distinct track IDs from the current dataset.".to_owned(),
                });
            }
        }

        Ok(self
            .tracks
            .entries
            .iter()
            .filter(|entry| tracks.is_none_or(|ids| ids.contains(&entry.id)))
            .map(|entry| entry.id)
            .collect())
    }

    /// Reports reference and gene services that are unavailable to this dataset.
    fn availability_warnings(&self) -> Vec<InspectWarning> {
        let mut warnings = Vec::new();
        if self.repository.sequence_service.is_none() {
            warnings.push(InspectWarning::ReferenceUnavailable {
                message: "The dataset has no reference sequence.".to_owned(),
            });
        }
        if self.repository.track_service.is_none() {
            warnings.push(InspectWarning::GenesUnavailable {
                message: "The dataset has no gene annotation service.".to_owned(),
            });
        }
        warnings
    }

    /// Summarizes an inclusive region, clamping its end to a known contig length.
    async fn inspect(&mut self, request: InspectRequest) -> Result<InspectResponse, SessionError> {
        let selected = self.selected_tracks(request.tracks.as_deref())?;
        let (query, region) = request.region.resolve(&self.state.contig_header)?;
        self.load_region(&query, &selected).await?;
        let contig_index = query.contig_index();

        let mut track_summaries = Vec::new();
        for &id in &selected {
            let summary = match self.tracks.get(id).repository_index {
                RepositoryFileIndex::Alignment(index) => TrackSummary::from_alignment(
                    id,
                    &self.state.alignments[index],
                    contig_index,
                    &region,
                )?,
                RepositoryFileIndex::Variant(index) => TrackSummary::from_variants(
                    id,
                    &self.state.variants[index],
                    contig_index,
                    &region,
                )?,
                RepositoryFileIndex::Bed(index) => TrackSummary::from_bed(
                    id,
                    &self.state.bed_intervals[index],
                    contig_index,
                    &region,
                )?,
            };
            track_summaries.push(summary);
        }
        let summary = InspectSummary {
            tracks: track_summaries,
            genes: GeneSummary::from_genes(
                &self.state.track,
                self.repository.track_service.is_some(),
                contig_index,
                &region,
            )?,
        };
        Ok(InspectResponse {
            region,
            summary,
            warnings: self.availability_warnings(),
        })
    }

    /// Runs a read-only SQL query over the session's tables.
    async fn query(&mut self, request: QueryRequest) -> Result<QueryResponse, SessionError> {
        let limit = request.limit()?;
        let resolved = request
            .region
            .as_ref()
            .map(|region| region.resolve(&self.state.contig_header))
            .transpose()?;
        match &resolved {
            Some((query, _)) => {
                let all: Vec<TrackId> = self.tracks.entries.iter().map(|entry| entry.id).collect();
                self.load_region(query, &all).await?;
            }
            None => {
                // Variant and BED repositories read whole files, so the region only labels logs.
                let anywhere = Region {
                    focus: Focus {
                        contig_index: 0,
                        position: 1,
                    },
                    half_width: 0,
                };
                let files: Vec<RepositoryFileIndex> = self
                    .tracks
                    .entries
                    .iter()
                    .map(|entry| entry.repository_index)
                    .filter(|index| !matches!(index, RepositoryFileIndex::Alignment(_)))
                    .collect();
                self.state
                    .ensure_loaded(
                        &anywhere,
                        &LoadRequest {
                            sequence: false,
                            genes: false,
                            files: &files,
                            cache: CachePolicy::EXACT,
                        },
                        &mut self.repository,
                    )
                    .await?;
            }
        }

        let region = resolved.as_ref().map(|(query, interval)| QueryRegion {
            contig_index: query.contig_index(),
            contig: &interval.contig,
            start: interval.start,
            end: interval.end,
        });
        let sources = TableSources {
            state: &self.state,
            tracks: &self.tracks,
            sources: self
                .tracks
                .entries
                .iter()
                .map(|entry| self.repository.file_path(entry.repository_index))
                .collect(),
        };
        let tables = sources.build(region.as_ref())?;
        let (frame, truncated) = request.execute(tables, limit)?;
        let warnings = if resolved.is_some() {
            self.availability_warnings()
        } else {
            Vec::new()
        };
        Ok(QueryResponse::from_frame(
            resolved.map(|(_, interval)| interval),
            &frame,
            truncated,
            warnings,
        )?)
    }
}
