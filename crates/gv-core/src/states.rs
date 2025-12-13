use crate::settings::Settings;
use crate::tracks::TrackService;
use crate::{
    alignment::Alignment,
    contig_header::ContigHeader,
    cytoband::Cytoband,
    error::TGVError,
    feature::Gene,
    intervals::{Focus, GenomeInterval, Region},
    message::{AlignmentDisplayOption, AlignmentFilter, DataMessage, Message},
    reference::Reference,
    //register::Registers,
    //rendering::{MainLayout, layout::resize_node},
    repository::Repository,
    sequence::{Sequence, SequenceRepository},
    track::Track,
};
use itertools::Itertools;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Scene {
    Main,
    Help,
    ContigList,
}

/// Holds states of the application.
pub struct State {
    pub messages: Vec<String>,

    pub contig_header: ContigHeader,
    pub reference: Reference,
    pub alignment: Alignment,
    pub alignment_options: Vec<AlignmentDisplayOption>,
    pub track: Track<Gene>,
    pub sequence: Sequence,
}

/// Getters
impl State {
    pub fn new(settings: &Settings, contigs: ContigHeader) -> Result<Self, TGVError> {
        Ok(Self {
            reference: settings.reference.clone(),

            // /settings: settings.clone(),
            messages: Vec::new(),

            alignment: Alignment::default(),
            alignment_options: Vec::new(),
            track: Track::<Gene>::default(),
            sequence: Sequence::default(),
            contig_header: contigs,
        })
    }

    pub fn contig_name(&self, region: &Region) -> Result<&String, TGVError> {
        self.contig_header
            .try_get(region.contig_index())
            .map(|contig| &contig.name)
    }

    pub fn current_cytoband(&self, region: &Region) -> Option<&Cytoband> {
        self.contig_header
            .try_get(region.contig_index())
            .unwrap()
            .cytoband
            .as_ref()
    }

    /// Maximum length of the contig.
    pub fn contig_length(&self, region: &Region) -> Result<Option<u64>, TGVError> {
        Ok(self.contig_header.try_get(region.contig_index())?.length)
    }

    const ALIGNMENT_CACHE_RATIO: u64 = 3;

    pub fn alignment_cache_region(&self, region: Region) -> Region {
        Region {
            focus: region.focus,
            half_width: region.half_width * Self::ALIGNMENT_CACHE_RATIO,
        }
    }

    const SEQUENCE_CACHE_RATIO: u64 = 6;

    pub fn sequence_cache_region(&self, region: Region) -> Region {
        Region {
            focus: region.focus,
            half_width: region.half_width * Self::SEQUENCE_CACHE_RATIO,
        }
    }

    const TRACK_CACHE_RATIO: u64 = 10;

    pub fn track_cache_region(&self, region: Region) -> Region {
        Region {
            focus: region.focus,
            half_width: region.half_width * Self::TRACK_CACHE_RATIO,
        }
    }
}

impl State {
    fn add_message(&mut self, message: String) {
        self.messages.push(message);
    }

    fn get_data_requirements(
        &self,
        region: &Region,
        repository: &mut Repository, // settings: &Settings,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut data_messages = Vec::new();

        // It's important to load sequence first!
        // Alignment IO requires calculating mismatches with the reference sequence.

        if repository.sequence_service.is_some()
            && self.sequence_renderable()
            && !self.sequence.has_complete_data(&region)
        {
            let sequence_cache_region = self.sequence_cache_region(region)?;
            data_messages.push(DataMessage::RequiresCompleteSequences(
                sequence_cache_region,
            ));
        }
        if repository.alignment_repository.is_some()
            && self.alignment_renderable()
            && !self.alignment.has_complete_data(&region)
        {
            let alignment_cache_region = self.alignment_cache_region(region)?;
            data_messages.push(DataMessage::RequiresCompleteAlignments(
                alignment_cache_region,
            ));
        }

        if repository.track_service.is_some() {
            if !self.track.has_complete_data(&region) {
                // viewing_window.zoom <= Self::MAX_ZOOM_TO_DISPLAY_FEATURES is always true
                let track_cache_region = self.track_cache_region(&region)?;
                data_messages.push(DataMessage::RequiresCompleteFeatures(track_cache_region));
            }

            // Cytobands
            data_messages.push(DataMessage::RequiresCytobands(region.contig_index));
        }

        Ok(data_messages)
    }

    pub async fn handle_data_message(
        &mut self,
        repository: &mut Repository,
        data_message: DataMessage,
    ) -> Result<bool, TGVError> {
        let mut loaded_data = false;

        match data_message {
            DataMessage::RequiresCompleteAlignments(region) => {
                if !self.alignment.has_complete_data(&region) {
                    self.alignment = {
                        let mut alignment = repository
                            .alignment_repository
                            .as_mut()
                            .unwrap()
                            .read_alignment(&region, &self.sequence, &self.contig_header)
                            .await?;

                        alignment.apply_options(&self.alignment_options, &self.sequence)?;

                        alignment
                    };

                    // apply sorting and filtering

                    loaded_data = true;
                }
            }
            DataMessage::RequiresCompleteFeatures(region) => {
                let has_complete_track = self.track.has_complete_data(&region);
                if let Some(track_service) = repository.track_service.as_mut() {
                    if !has_complete_track {
                        if let Ok(track) = track_service
                            .query_gene_track(&self.reference, &region, &self.contig_header)
                            .await
                        {
                            self.track = track;
                            loaded_data = true;
                        } else {
                            // Do nothing (track not found). TODO: fix this shit properly.
                        }
                    }
                } else {
                    loaded_data = match self.reference {
                        // FIXME: this is duplicate code as Settings.
                        Reference::BYOIndexedFasta(_) => false,
                        _ => true,
                    };
                }
            }
            DataMessage::RequiresCompleteSequences(region) => {
                let sequence_service = repository.sequence_service_checked()?;

                if !self.sequence.has_complete_data(&region) {
                    let sequence = sequence_service
                        .query_sequence(&region, &self.contig_header)
                        .await?;

                    self.sequence = sequence;
                    loaded_data = true;
                }
            }

            DataMessage::RequiresCytobands(contig_index) => {
                if self.contig_header.cytoband_is_loaded(contig_index)? {
                    return Ok(false);
                }

                if let Some(track_service) = repository.track_service.as_mut() {
                    let cytoband = track_service
                        .get_cytoband(&self.reference, contig_index, &self.contig_header)
                        .await?;
                    self.contig_header
                        .try_update_cytoband(contig_index, cytoband)?;
                    loaded_data = true;
                }
            }
        }

        Ok(loaded_data)
    }
}

impl State {
    /// Main function to route state message handling.
    pub fn set_alignment_change(
        &self,
        focus: &Focus,
        options: Vec<AlignmentDisplayOption>,
    ) -> Result<(), TGVError> {
        self.alignment.reset(&self.sequence)?;

        let options = options
            .into_iter()
            .map(|option| match option {
                AlignmentDisplayOption::Filter(AlignmentFilter::BaseAtCurrentPosition(base)) => {
                    AlignmentDisplayOption::Filter(AlignmentFilter::Base(focus.position, base))
                }

                AlignmentDisplayOption::Filter(AlignmentFilter::BaseAtCurrentPositionSoftClip) => {
                    AlignmentDisplayOption::Filter(AlignmentFilter::BaseSoftclip(focus.position))
                }

                _ => option,
            })
            .collect_vec();
        self.alignment_options = options;
        self.alignment
            .apply_options(options, &self.sequence)
            .map(|_| {})
    }

    //Self::get_data_requirements(state, repository)
}

// Movement handling
impl State {
    async fn next_genes_start(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        // The gene is in the track.
        if let Some(gene) = self.track.get_k_genes_after(focus.position, n) {
            return Ok(Focus {
                contig_index: gene.contig_index,
                position: gene.start(),
            });
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_after(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: gene.contig_index,
            position: gene.start(),
        })
    }

    async fn next_genes_end(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(gene) = self.track.get_k_genes_after(focus.position, n) {
            return Ok(Focus {
                contig_index: gene.contig_index,
                position: gene.end() + 1,
            });
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_after(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: gene.contig_index,
            position: gene.end() + 1,
        })
    }

    async fn go_to_previous_genes_start(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(gene) = self.track.get_k_genes_before(focus.position, n) {
            return Ok(Focus {
                contig_index: gene.contig_index,
                position: gene.start() - 1,
            });
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_before(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: gene.contig_index,
            position: gene.start() - 1,
        })
    }

    async fn go_to_previous_genes_end(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(gene) = self.track.get_k_genes_before(focus.position, n) {
            return Ok(Focus {
                contig_index: gene.contig_index,
                position: gene.end() - 1,
            });
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_before(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: gene.contig_index,
            position: gene.end() - 1,
        })
    }

    async fn go_to_next_exons_start(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(exon) = self.track.get_k_exons_after(focus.position, n) {
            return Ok(Focus {
                contig_index: exon.contig_index,
                position: exon.start() + 1,
            });
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_after(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: exon.contig_index,
            position: exon.start() + 1,
        })
    }

    async fn go_to_next_exons_end(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(exon) = self.track.get_k_exons_after(focus.position, n) {
            return Ok(Focus {
                contig_index: exon.contig_index,
                position: exon.end() + 1,
            });
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_after(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: exon.contig_index,
            position: exon.end() + 1,
        })
    }

    async fn go_to_previous_exons_start(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(exon) = self.track.get_k_exons_before(focus.position, n) {
            return Ok(Focus {
                contig_index: exon.contig_index,
                position: exon.start() - 1,
            });
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_before(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: exon.contig_index,
            position: exon.start() - 1,
        })
    }

    async fn go_to_previous_exons_end(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        let exon = self.track.get_k_exons_before(focus.position, n);
        if let Some(exon) = exon {
            return Ok(Focus {
                contig_index: exon.contig_index,
                position: exon.end() - 1,
            });
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_before(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: exon.contig_index,
            position: exon.end() - 1,
        })
    }

    async fn go_to_gene(
        &self,
        repository: &mut Repository,
        gene_name: &str,
    ) -> Result<Focus, TGVError> {
        repository
            .track_service_checked()?
            .query_gene_name(&self.reference, gene_name, &self.contig_header)
            .await
            .map(|gene| Focus {
                contig_index: gene.contig_index(),
                position: gene.start() + 1,
            })
    }

    async fn go_to_next_contig(&self, focus: Focus, n: usize) -> Focus {
        Focus {
            contig_index: self.contig_header.next(focus.contig_index, n),
            position: 1,
        }
    }

    async fn go_to_previous_contig(&self, focus: Focus, n: usize) -> Focus {
        Focus {
            contig_index: self.contig_header.previous(focus.contig_index, n),

            position: 1,
        }
    }

    async fn default_focus(&self, repository: &mut Repository) -> Result<Focus, TGVError> {
        match self.reference {
            Reference::Hg38 | Reference::Hg19 => {
                return self.go_to_gene(repository, "TP53").await;
            }

            Reference::UcscGenome(_) | Reference::UcscAccession(_) => {
                // Find the first gene on the first contig. If anything is not found, handle it later.

                let first_contig = self.contig_header.first()?;

                // Try to get the first gene in the first contig.
                // We use query_k_genes_after starting from coordinate 0 with k=1.
                match repository
                    .track_service_checked()?
                    .query_k_genes_after(&self.reference, first_contig, 0, 1, &self.contig_header)
                    .await
                {
                    Ok(gene) => {
                        // Found a gene, go to its start (using 1-based coordinates for Goto)
                        return Ok(Focus {
                            contig_index: gene.contig_index,
                            position: gene.start() + 1,
                        });
                    }
                    Err(_) => {} // Gene not found. Handle later.
                }
            }

            Reference::BYOIndexedFasta(_) | Reference::BYOTwoBit(_) | Reference::NoReference => {} // handle later
        };

        // If reaches here, go to the first contig:1
        self.contig_header
            .first()
            .map(|contig_index| Focus {
                contig_index,
                position: 1,
            })
            .map_err(|_| {
                TGVError::StateError(
            "Failed to find a default initial region. Please provide a starting region with -r."
                .to_string() )
            })
    }
}
