use crate::settings::Settings;
use crate::tracks::TrackService;
use crate::{
    alignment::Alignment,
    contig_header::ContigHeader,
    cytoband::Cytoband,
    error::TGVError,
    feature::Gene,
    intervals::{Focus, GenomeInterval, Region},
    message::{AlignmentDisplayOption, AlignmentFilter, Movement},
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
    pub async fn movement(
        &self,
        focus: Focus,
        repository: &mut Repository,
        movement: Movement,
    ) -> Result<Focus, TGVError> {
        match movement {
            Movement::Left(n) => Ok(focus.move_to(focus.position.saturating_sub(n))),
            Movement::Right(n) => Ok(focus.move_to(focus.position.saturating_add(n))),
            Movement::Position(position) => Ok(focus.move_to(position)),
            Movement::ContigNamePosition(contig_name, position) => Ok(Focus {
                contig_index: self
                    .contig_header
                    .try_get_index_by_str(contig_name.as_ref())?,
                position,
            }),
            Movement::NextExonsStart(n) => self.go_to_next_exons_start(focus, repository, n).await,
        }
    }

    fn add_message(&mut self, message: String) {
        self.messages.push(message);
    }

    pub async fn ensure_complete_alignment_data(
        &mut self,
        region: &Region,
        repository: &mut Repository,
    ) -> Result<bool, TGVError> {
        if !self.alignment.has_complete_data(&region) {
            Ok(false)
        } else {
            self.alignment = repository
                .alignment_repository
                .as_mut()
                .unwrap()
                .read_alignment(&region, &self.sequence, &self.contig_header)
                .await?
                .apply_options(&self.alignment_options, &self.sequence)?;

            Ok(true)
        }
    }

    pub async fn ensure_complete_track_data(
        &mut self,
        region: &Region,
        repository: &mut Repository,
    ) -> Result<bool, TGVError> {
        if let Some(track_service) = repository.track_service.as_mut() {
            if !self.track.has_complete_data(&region) {
                if let Ok(track) = track_service
                    .query_gene_track(&self.reference, &region, &self.contig_header)
                    .await
                {
                    self.track = track;
                    Ok(true)
                } else {
                    // Do nothing (track not found). TODO: fix this shit properly.
                    Ok(false)
                }
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    pub async fn ensure_complete_sequence_data(
        &mut self,
        region: &Region,
        repository: &mut Repository,
    ) -> Result<bool, TGVError> {
        if let Some(sequence_service) = repository.sequence_service.as_mut() {
            if !self.sequence.has_complete_data(&region) {
                let sequence = sequence_service
                    .query_sequence(&region, &self.contig_header)
                    .await?;

                self.sequence = sequence;
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    pub async fn ensure_complete_cytoband_data(
        &mut self,
        region: &Region,
        repository: &mut Repository,
    ) -> Result<bool, TGVError> {
        if self
            .contig_header
            .cytoband_is_loaded(region.contig_index())?
        {
            Ok(false)
        } else if let Some(track_service) = repository.track_service.as_mut() {
            let cytoband = track_service
                .get_cytoband(&self.reference, region.contig_index(), &self.contig_header)
                .await?;
            self.contig_header
                .try_update_cytoband(region.contig_index(), cytoband)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl State {
    /// Main function to route state message handling.
    pub fn set_alignment_change(
        mut self,
        focus: &Focus,
        options: Vec<AlignmentDisplayOption>,
    ) -> Result<Self, TGVError> {
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
        self.alignment = self
            .alignment
            .apply_options(&self.alignment_options, &self.sequence)?;
        Ok(self)
    }

    //Self::get_data_requirements(state, repository)
}

// Movement handling
impl State {
    pub async fn next_genes_start(
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

    pub async fn next_genes_end(
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

    pub async fn go_to_previous_genes_start(
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

    pub async fn go_to_previous_genes_end(
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

    pub async fn go_to_next_exons_start(
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

    pub async fn go_to_next_exons_end(
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

    pub async fn go_to_previous_exons_start(
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

    pub async fn go_to_previous_exons_end(
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

    pub async fn go_to_gene(
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

    pub async fn default_focus(&self, repository: &mut Repository) -> Result<Focus, TGVError> {
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
