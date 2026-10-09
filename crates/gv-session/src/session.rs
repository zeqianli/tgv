//! Sessions: the requests that hosts serve, the handles that send them, and the dataset they
//! serve them against.
//!
//! A host owns one [`Dataset`] and serves requests in order. The headless worker from
//! [`Session::spawn`] is one host; a viewer that displays the dataset is another.

use crate::{
    error::SessionError,
    schema::*,
    tables::{QueryRegion, TableSources},
};
use gv_core::{
    alignment::is_url,
    prelude::*,
    settings::{Settings, classify_and_build_tracks},
    track_registry::TrackEntry,
};
use std::{path::Path, sync::Arc};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

/// Sends one command's result back to the caller.
pub struct Responder<T>(oneshot::Sender<Result<T, SessionError>>);

impl<T> Responder<T> {
    /// Sends the result. A caller that stopped waiting drops it harmlessly.
    pub fn respond(self, result: Result<T, SessionError>) {
        let _ = self.0.send(result);
    }

    fn is_closed(&self) -> bool {
        self.0.is_closed()
    }
}

/// A command that reads the dataset, served by [`Dataset::serve`].
pub struct DataRequest(DataCommand);

enum DataCommand {
    Describe(Responder<Option<DatasetDescription>>),
    Inspect(InspectRequest, Responder<InspectResponse>),
    Query(QueryRequest, Responder<QueryResponse>),
}

impl DataRequest {
    /// Answers the request when the host has no dataset.
    pub fn respond_without_dataset(self) {
        match self.0 {
            DataCommand::Describe(reply) => reply.respond(Ok(None)),
            DataCommand::Inspect(_, reply) => reply.respond(Err(SessionError::NoDataset {
                operation: "inspecting",
            })),
            DataCommand::Query(_, reply) => reply.respond(Err(SessionError::NoDataset {
                operation: "querying",
            })),
        }
    }
}

/// A command that acts on a viewer, served by the host that displays the dataset.
pub enum ViewRequest {
    /// Shows a region.
    Navigate(NavigateRequest, Responder<ViewState>),
    /// Marks intervals in the view.
    Highlight(HighlightRequest, Responder<()>),
    /// Removes all highlights.
    ClearHighlights(Responder<()>),
    /// Reports what the view shows.
    Current(Responder<ViewState>),
}

impl ViewRequest {
    /// Rejects the request, for example because no viewer displays the dataset.
    pub fn reject(self, error: SessionError) {
        match self {
            Self::Navigate(_, reply) | Self::Current(reply) => reply.respond(Err(error)),
            Self::Highlight(_, reply) | Self::ClearHighlights(reply) => reply.respond(Err(error)),
        }
    }
}

/// A request that a session host receives from a [`SessionHandle`].
pub enum Request {
    Data(DataRequest),
    LoadDataset(DatasetRequest, Responder<DatasetDescription>),
    View(ViewRequest),
    Shutdown,
}

impl Request {
    /// Whether the caller stopped waiting, so the work can be skipped.
    fn is_abandoned(&self) -> bool {
        match self {
            Self::Data(DataRequest(DataCommand::Describe(reply))) => reply.is_closed(),
            Self::Data(DataRequest(DataCommand::Inspect(_, reply))) => reply.is_closed(),
            Self::Data(DataRequest(DataCommand::Query(_, reply))) => reply.is_closed(),
            Self::LoadDataset(_, reply) => reply.is_closed(),
            Self::View(ViewRequest::Navigate(_, reply) | ViewRequest::Current(reply)) => {
                reply.is_closed()
            }
            Self::View(ViewRequest::Highlight(_, reply) | ViewRequest::ClearHighlights(reply)) => {
                reply.is_closed()
            }
            Self::Shutdown => false,
        }
    }
}

/// Receives the requests sent through the matching [`SessionHandle`]s.
pub struct Requests(mpsc::Receiver<Request>);

impl Requests {
    /// Waits for the next request that a caller still waits for. Returns `None` once every
    /// handle is dropped.
    pub async fn recv(&mut self) -> Option<Request> {
        loop {
            let request = self.0.recv().await?;
            if !request.is_abandoned() {
                return Some(request);
            }
        }
    }
}

/// Sends requests to a session host. Clones address the same session.
#[derive(Clone)]
pub struct SessionHandle {
    sender: mpsc::Sender<Request>,
}

impl SessionHandle {
    /// Describes the loaded dataset, or returns `None` before the first load.
    pub async fn describe(&self) -> Result<Option<DatasetDescription>, SessionError> {
        self.request(|reply| Request::Data(DataRequest(DataCommand::Describe(reply))))
            .await
    }

    /// Loads or replaces the dataset. A failed load leaves the previous dataset in place.
    pub async fn load_dataset(
        &self,
        request: DatasetRequest,
    ) -> Result<DatasetDescription, SessionError> {
        self.request(|reply| Request::LoadDataset(request, reply))
            .await
    }

    /// Summarizes an interval.
    pub async fn inspect(&self, request: InspectRequest) -> Result<InspectResponse, SessionError> {
        self.request(|reply| Request::Data(DataRequest(DataCommand::Inspect(request, reply))))
            .await
    }

    /// Runs a read-only SQL query.
    pub async fn query(&self, request: QueryRequest) -> Result<QueryResponse, SessionError> {
        self.request(|reply| Request::Data(DataRequest(DataCommand::Query(request, reply))))
            .await
    }

    /// Shows a region in the viewer.
    pub async fn navigate(&self, request: NavigateRequest) -> Result<ViewState, SessionError> {
        self.request(|reply| Request::View(ViewRequest::Navigate(request, reply)))
            .await
    }

    /// Marks intervals in the viewer.
    pub async fn highlight(&self, request: HighlightRequest) -> Result<(), SessionError> {
        self.request(|reply| Request::View(ViewRequest::Highlight(request, reply)))
            .await
    }

    /// Removes all highlights from the viewer.
    pub async fn clear_highlights(&self) -> Result<(), SessionError> {
        self.request(|reply| Request::View(ViewRequest::ClearHighlights(reply)))
            .await
    }

    /// Reports what the viewer shows.
    pub async fn view_state(&self) -> Result<ViewState, SessionError> {
        self.request(|reply| Request::View(ViewRequest::Current(reply)))
            .await
    }

    /// Asks a headless worker to close the dataset and stop after earlier requests finish.
    pub async fn shutdown(&self) {
        // A worker that already stopped needs no shutdown.
        let _ = self.sender.send(Request::Shutdown).await;
    }

    async fn request<T>(
        &self,
        request: impl FnOnce(Responder<T>) -> Request,
    ) -> Result<T, SessionError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(request(Responder(reply)))
            .await
            .map_err(|_| SessionError::Unavailable)?;
        response.await.map_err(|_| SessionError::Stopped)?
    }
}

/// Creates sessions.
pub struct Session;

impl Session {
    const QUEUE: usize = 16;

    /// Creates a handle and the requests it sends, for a host that serves them itself.
    pub fn channel() -> (SessionHandle, Requests) {
        let (sender, receiver) = mpsc::channel(Self::QUEUE);
        (SessionHandle { sender }, Requests(receiver))
    }

    /// Starts a headless worker on the current Tokio runtime and returns its handle and join
    /// handle. The worker rejects view requests with [`SessionError::NoViewer`].
    ///
    /// `defaults` provides the backend, cache, and host settings for loaded datasets.
    pub fn spawn(defaults: Settings) -> (SessionHandle, JoinHandle<Result<(), TGVError>>) {
        let (handle, requests) = Self::channel();
        // Synchronous readers and coverage work must not block the caller's runtime threads.
        // Construct the dataset inside this thread because repository types need not be Send.
        let runtime = tokio::runtime::Handle::current();
        let worker =
            tokio::task::spawn_blocking(move || runtime.block_on(Dataset::run(defaults, requests)));
        (handle, worker)
    }
}

/// Owns a loaded dataset: its repositories, its tracks, and two states over them.
///
/// `view` holds what a viewer displays. `query` holds what `inspect` and `query` commands load,
/// so an agent's queries never replace the region a person is looking at. Both states read
/// through the same repositories, but load regions independently.
pub struct Dataset {
    pub settings: Settings,
    pub repository: Repository,
    pub tracks: Arc<TrackRegistry>,
    pub view: State,
    query: State,
}

impl Dataset {
    /// Serves requests headlessly, in order, and closes the dataset when the worker exits.
    async fn run(defaults: Settings, mut requests: Requests) -> Result<(), TGVError> {
        let mut dataset: Option<Self> = None;
        while let Some(request) = requests.recv().await {
            match request {
                Request::Shutdown => break,
                Request::Data(request) => match dataset.as_mut() {
                    Some(dataset) => dataset.serve(request).await,
                    None => request.respond_without_dataset(),
                },
                Request::LoadDataset(request, reply) => {
                    reply.respond(Self::replace(&mut dataset, &defaults, request).await);
                }
                Request::View(request) => request.reject(SessionError::NoViewer),
            }
        }

        if let Some(mut dataset) = dataset {
            dataset.repository.close().await?;
        }

        Ok(())
    }

    /// Serves a data request against this dataset's query state.
    pub async fn serve(&mut self, request: DataRequest) {
        match request.0 {
            DataCommand::Describe(reply) => reply.respond(Ok(Some(self.description()))),
            DataCommand::Inspect(request, reply) => reply.respond(self.inspect(request).await),
            DataCommand::Query(request, reply) => reply.respond(self.query(request).await),
        }
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
                    source: entry.file_path.path().to_owned(),
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
        let (new_settings, files) = request.update_settings(prior_settings)?;
        let replacement =
            Self::new(new_settings, files)
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

    /// Opens the reference and the files, and creates empty view and query states.
    pub async fn new(settings: Settings, files: Vec<FilePath>) -> Result<Self, TGVError> {
        let (repository, contigs) = Repository::new(&settings).await?;
        let mut dataset = Self {
            view: State::new(settings.reference.clone(), contigs.clone())?,
            query: State::new(settings.reference.clone(), contigs)?,
            settings,
            repository,
            tracks: Arc::new(TrackRegistry::default()),
        };
        dataset.add_file_paths(files).await?;
        Ok(dataset)
    }

    /// Classifies paths by file type, then adds them like [`Dataset::add_file_paths`].
    pub async fn add_files(&mut self, files: &[String]) -> Result<Vec<TrackId>, TGVError> {
        self.add_file_paths(classify_and_build_tracks(files)?).await
    }

    /// Opens files and appends them as tracks with fresh IDs, in order.
    ///
    /// Either every file is added or none is. Each file's contig names are recorded in both
    /// states' contig headers before the tracks are added. New contigs are appended, so existing
    /// contig indexes stay valid.
    pub async fn add_file_paths(
        &mut self,
        file_paths: Vec<FilePath>,
    ) -> Result<Vec<TrackId>, TGVError> {
        if let Some(missing) = file_paths
            .iter()
            .map(FilePath::path)
            .find(|path| !is_url(path) && !Path::new(path).is_file())
        {
            return Err(TGVError::NotAFile(missing.to_owned()));
        }

        let mut opened = Vec::with_capacity(file_paths.len());
        let mut view_contigs = self.view.contig_header.clone();
        let mut query_contigs = self.query.contig_header.clone();
        if let Err(error) = self
            .open_files(
                &file_paths,
                &mut opened,
                &mut view_contigs,
                &mut query_contigs,
            )
            .await
        {
            // Later files of a kind sit after earlier ones, so remove them last-first.
            for &index in opened.iter().rev() {
                self.repository.remove(index);
            }
            return Err(error);
        }
        self.view.contig_header = view_contigs;
        self.query.contig_header = query_contigs;

        let tracks = Arc::make_mut(&mut self.tracks);
        let mut ids = Vec::with_capacity(opened.len());
        for (file_path, index) in file_paths.into_iter().zip(opened) {
            self.view.add_track(&file_path);
            self.query.add_track(&file_path);
            ids.push(tracks.push(index, file_path));
        }
        Ok(ids)
    }

    /// Opens repositories into `opened` and merges their contigs, stopping at the first error.
    async fn open_files(
        &mut self,
        file_paths: &[FilePath],
        opened: &mut Vec<RepositoryFileIndex>,
        view_contigs: &mut ContigHeader,
        query_contigs: &mut ContigHeader,
    ) -> Result<(), TGVError> {
        for file_path in file_paths {
            let index = self
                .repository
                .open_file(file_path, self.settings.quality_encoding)
                .await?;
            opened.push(index);
            self.repository.merge_contigs(index, view_contigs)?;
            self.repository.merge_contigs(index, query_contigs)?;
        }
        Ok(())
    }

    /// Removes a track and returns its entry. Later tracks of the same kind shift down by one
    /// in the per-kind state, and front ends apply the same shift to theirs.
    pub fn remove_track(&mut self, id: TrackId) -> Result<TrackEntry, TGVError> {
        let entry = Arc::make_mut(&mut self.tracks)
            .remove(id)
            .ok_or_else(|| TGVError::StateError(format!("Track {id} does not exist.")))?;
        self.repository.remove(entry.repository_index);
        self.view.remove_track(entry.repository_index);
        self.query.remove_track(entry.repository_index);
        Ok(entry)
    }

    /// Loads the selected tracks, the reference sequence, and gene annotations for a region
    /// into the query state.
    ///
    /// Data that is already complete for the region is reused. Requests can span 100,000 bases
    /// at high depth, so loads cover exactly the region instead of padding it like the viewer.
    async fn load_query_region(
        &mut self,
        region: &Region,
        track_ids: &[TrackId],
    ) -> Result<(), TGVError> {
        let files: Vec<RepositoryFileIndex> = track_ids
            .iter()
            .map(|&id| self.tracks.get(id).repository_index)
            .collect();
        self.query
            .ensure_loaded(
                region,
                &LoadRequest {
                    sequence: true,
                    genes: true,
                    cytobands: false,
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
                    .any(|(i, id)| tracks[..i].contains(id) || !self.tracks.contains(*id))
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
        let (query, region) = request.region.resolve(&self.query.contig_header)?;
        self.load_query_region(&query, &selected).await?;
        let contig_index = query.contig_index();

        let mut track_summaries = Vec::new();
        for &id in &selected {
            let summary = match self.tracks.get(id).repository_index {
                RepositoryFileIndex::Alignment(index) => TrackSummary::from_alignment(
                    id,
                    &self.query.alignments[index],
                    contig_index,
                    &region,
                )?,
                RepositoryFileIndex::Variant(index) => TrackSummary::from_variants(
                    id,
                    &self.query.variants[index],
                    contig_index,
                    &region,
                )?,
                RepositoryFileIndex::Bed(index) => TrackSummary::from_bed(
                    id,
                    &self.query.bed_intervals[index],
                    contig_index,
                    &region,
                )?,
            };
            track_summaries.push(summary);
        }
        let summary = InspectSummary {
            tracks: track_summaries,
            genes: GeneSummary::from_genes(
                &self.query.track,
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
            .map(|region| region.resolve(&self.query.contig_header))
            .transpose()?;
        // Without a region, only dataset tables exist, so there is nothing to load.
        if let Some((query, _)) = &resolved {
            let all: Vec<TrackId> = self.tracks.entries.iter().map(|entry| entry.id).collect();
            self.load_query_region(query, &all).await?;
        }

        let region = resolved.as_ref().map(|(query, interval)| QueryRegion {
            contig_index: query.contig_index(),
            contig: &interval.contig,
            start: interval.start,
            end: interval.end,
        });
        let sources = TableSources {
            state: &self.query,
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

#[cfg(test)]
mod tests {
    use super::*;

    const CONTIG: &str = "MN908947.3";

    const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../tgv/tests/data");

    fn covid_settings() -> Settings {
        Settings {
            reference: format!("{DATA}/covid.fa").parse().unwrap(),
            ..Settings::default()
        }
    }

    async fn covid_dataset() -> Dataset {
        let files = classify_and_build_tracks(&[format!("{DATA}/covid.sorted.bam")]).unwrap();
        Dataset::new(covid_settings(), files).await.unwrap()
    }

    fn region(dataset: &Dataset, start: u64, end: u64) -> Region {
        InspectInterval {
            contig: CONTIG.to_owned(),
            start,
            end,
        }
        .resolve(&dataset.view.contig_header)
        .unwrap()
        .0
    }

    /// A query elsewhere loads into the query state and leaves the view's data in place.
    // Polars blocks in place inside Tokio, which needs the multi-threaded runtime.
    #[tokio::test(flavor = "multi_thread")]
    async fn queries_leave_the_view_loaded() {
        let mut dataset = covid_dataset().await;
        let viewed = region(&dataset, 100, 300);
        let files: Vec<RepositoryFileIndex> = dataset
            .tracks
            .entries
            .iter()
            .map(|entry| entry.repository_index)
            .collect();
        dataset
            .view
            .ensure_loaded(
                &viewed,
                &LoadRequest {
                    sequence: true,
                    genes: true,
                    cytobands: false,
                    files: &files,
                    cache: CachePolicy::VIEWER,
                },
                &mut dataset.repository,
            )
            .await
            .unwrap();
        let viewed_reads = dataset.view.alignments[0].tables.reads.height();
        assert!(viewed_reads > 0);

        dataset
            .query(QueryRequest {
                region: Some(InspectInterval {
                    contig: CONTIG.to_owned(),
                    start: 20_000,
                    end: 20_200,
                }),
                sql: "SELECT count(*) FROM reads".to_owned(),
                limit: None,
            })
            .await
            .unwrap();

        assert!(dataset.view.alignments[0].has_complete_data(&viewed));
        assert_eq!(
            dataset.view.alignments[0].tables.reads.height(),
            viewed_reads
        );
        assert!(dataset.query.alignments[0].has_complete_data(&region(&dataset, 20_000, 20_200)));
        assert!(!dataset.query.alignments[0].has_complete_data(&viewed));
    }

    /// Without a viewer, view requests fail with `NoViewer` and data requests still work.
    #[tokio::test(flavor = "multi_thread")]
    async fn headless_sessions_reject_view_requests() {
        let (session, worker) = Session::spawn(covid_settings());
        let error = session
            .navigate(NavigateRequest {
                region: InspectInterval {
                    contig: CONTIG.to_owned(),
                    start: 1,
                    end: 100,
                },
            })
            .await
            .unwrap_err();
        assert!(matches!(error, SessionError::NoViewer));
        assert!(session.describe().await.unwrap().is_none());

        session.shutdown().await;
        worker.await.unwrap().unwrap();
    }
}
