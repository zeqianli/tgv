use crate::{
    error::TGVError,
    helpers::is_url,
    models::{alignment::{Alignment, AlignmentBuilder}, contig::Contig, reference::Reference, region::Region},
};
use rust_htslib::bam;
use rust_htslib::bam::{Header, IndexedReader, Read, Record};
use url::Url;




enum RemoteSource {
    S3,
    HTTP,
    GS,
}

impl RemoteSource {

    fn from(path: &String) -> Result<Self, TGVError> {

        if path.starts_with("s3://") {
            Ok(Self::S3)
        } else if path.starts_with("http://") || path.starts_with("https://") {
            Ok(Self::HTTP)
        } else  if path.starts_with("gss://") {
            Ok(Self::GS)
        } else {
            Err(TGVError::ValueError(format!(
                "Unsupported remote path {}. Only S3, HTTP/HTTPS, and GS are supported.",
                path
            )))
        }
    }

}


trait AlignmentRepository {
    fn read_alignment(&self, region: &Region) -> Result<Alignment, TGVError>;

    fn read_header(&self) -> Result<Vec<(String, Option<usize>)>, TGVError>;
}

pub struct BAMRepository {
    bam_path: String,
    bai_path: Option<String>,
}

impl BAMRepository {
    fn new(bam_path: String, bai_path: Option<String>) -> Result<Self, TGVError> {
        if is_url(&bam_path){
            return Err(TGVError::IOError(format!(
                "{} is a remote path. Use RemoteBAMRepository for remote BAM IO",
                bam_path
            )));
        }

        Ok(Self{
            bam_path,
            bai_path
        })
    }

    
}



impl AlignmentRepository for BAMRepository {
    fn read_alignment(&self, region: &Region) -> Result<Alignment, TGVError> {

        let mut bam = match self.bai_path.as_ref() {
            Some(bai_path) => IndexedReader::from_path_and_index(self.bam_path.clone(), bai_path.clone())?,
            None =>  IndexedReader::from_path(self.bam_path.clone())?
        };

        let header = bam::Header::from_template(bam.header());

        let query_contig_string = get_query_contig_string(&header, region)?;
        bam.fetch((
            &query_contig_string,
            region.start as i32 - 1,
            region.end as i32,
        ))
        .map_err(|e| TGVError::IOError(e.to_string()))?;

        let mut alignment_builder = AlignmentBuilder::new()?;

        for record in bam.records() {
            let read = record.map_err(|e| TGVError::IOError(e.to_string()))?;
            alignment_builder.add_read(read)?;
        }

        alignment_builder.region(region)?.build()
    }

    /// Read BAM headers and return contig namesa and lengths.
    /// Note that this function does not interprete the contig name as contg vs chromosome.
    fn read_header(&self) -> Result<Vec<(String, Option<usize>)>, TGVError> {

        let mut bam = match self.bai_path.as_ref() {
            Some(bai_path) => IndexedReader::from_path_and_index(self.bam_path.clone(), bai_path.clone())?,
            None =>  IndexedReader::from_path(self.bam_path.clone())?
        };

        let header = bam::Header::from_template(bam.header());
        get_contig_names_and_lengths_from_header(&header)
        
    }
}

struct RemoteBAMRepository {
    bam_path: String,
    source: RemoteSource,
}

impl RemoteBAMRepository {
    pub fn new(bam_path: &String) -> Result<Self, TGVError>{

        Ok(Self{
            bam_path: bam_path.clone(), 
            source: RemoteSource::from(bam_path)?
        })
    }
}

impl AlignmentRepository for RemoteBAMRepository{

    fn read_alignment(&self, region: &Region) -> Result<Alignment, TGVError> {
        let mut bam = IndexedReader::from_url(
            &Url::parse(&self.bam_path).map_err(|e| TGVError::IOError(e.to_string()))?,
        )?;

        let header = bam::Header::from_template(bam.header());


        let query_contig_string = get_query_contig_string(&header, region)?;
        bam.fetch((
            &query_contig_string,
            region.start as i32 - 1,
            region.end as i32,
        ))
        .map_err(|e| TGVError::IOError(e.to_string()))?;

        let mut alignment_builder = AlignmentBuilder::new()?;

        for record in bam.records() {
            let read = record.map_err(|e| TGVError::IOError(e.to_string()))?;
            alignment_builder.add_read(read)?;
        }

        alignment_builder.region(region)?.build()
    }

    fn read_header(&self) -> Result<Vec<(String, Option<usize>)>, TGVError>{
        let mut bam = IndexedReader::from_url(
            &Url::parse(&self.bam_path).map_err(|e| TGVError::IOError(e.to_string()))?,
        )?;

        let header = bam::Header::from_template(bam.header());
        get_contig_names_and_lengths_from_header(&header)
    }
}




// fn is_remote_path {
//     IndexedReader::from_url(
//         &Url::parse(bam_path).map_err(|e| TGVError::IOError(e.to_string()))?,
//     )
//     .unwrap();



// struct CRAMRepository {
//     cram_path: String,
// }


fn get_contig_names_and_lengths_from_header(header: &Header) -> Result<Vec<(String, Option<usize>)>, TGVError>  {

    let mut output = Vec::new();

    for (_key, records) in header.to_hashmap().iter() {
        for record in records {
            if record.contains_key("SN") {
                let contig_name = record["SN"].to_string();
                let contig_length = if record.contains_key("LN") {
                    record["LN"].to_string().parse::<usize>().ok()
                } else {
                    None
                };

                output.push((contig_name, contig_length))
            }
        }
    }

    Ok(output)
}

 /// Get the query string for a region.
/// Look through the header to decide if the bam file chromosome names are abbreviated or full.
fn get_query_contig_string(header: &Header, region: &Region) -> Result<String, TGVError> {
    let full_chromsome_str = region.contig.full_name();
    let abbreviated_chromsome_str = region.contig.abbreviated_name();

    for (_key, records) in header.to_hashmap().iter() {
        for record in records {
            if record.contains_key("SN") {
                let reference_name = record["SN"].to_string();
                if reference_name == full_chromsome_str {
                    return Ok(full_chromsome_str);
                }

                if reference_name == abbreviated_chromsome_str {
                    return Ok(abbreviated_chromsome_str);
                }
            }
        }
    }

    Err(TGVError::IOError("Contig not found in header".to_string()))
}