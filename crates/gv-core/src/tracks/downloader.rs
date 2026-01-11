use crate::tracks::{TRACK_PREFERENCES, TrackService, UcscApiTrackService, UcscDbTrackService};
use crate::{error::TGVError, reference::Reference, tracks::UcscHost};
use bigtools::BigBedRead;
use sqlx::{
    Column, MySqlPool, Pool, Row,
    mysql::MySqlPoolOptions,
    sqlite::{Sqlite, SqliteConnectOptions, SqlitePool, SqlitePoolOptions},
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
/// Download data from UCSC mariaDB to a local sqlite file.
pub struct UCSCDownloader {
    reference: Reference,

    /// TGV main cache directory. Reference data are stored in cache_dir/reference_name/.
    cache_dir: String,
}

/// UCSC column type. Used to map MySQL types to SQLite types.
#[derive(Debug)]
enum UCSCColumnType {
    UnsignedInt,
    Int,
    Float,
    Blob,
    String,
}

impl UCSCColumnType {
    fn to_sqlite_type(&self) -> &str {
        match self {
            UCSCColumnType::UnsignedInt => "INTEGER",
            UCSCColumnType::Int => "INTEGER",
            UCSCColumnType::Float => "REAL",
            UCSCColumnType::Blob => "BLOB",
            UCSCColumnType::String => "TEXT",
        }
    }
}

impl UCSCDownloader {
    pub fn new(reference: Reference, cache_dir: &str) -> Result<Self, TGVError> {
        let cache_dir = reference.cache_dir(cache_dir);
        std::fs::create_dir_all(Path::new(&cache_dir))
            .map_err(|e| TGVError::IOError(format!("Failed to create genome directory: {}", e)))?;
        Ok(Self {
            reference: reference.clone(),
            cache_dir,
        })
    }

    /// Download data for references. This is the main entry point for downloading data.
    pub async fn download(&self) -> Result<(), TGVError> {
        // Create SQLite database file path: cache_dir/reference_name/tracks.sqlite

        let db_path = Path::new(&self.cache_dir).join("tracks.sqlite");

        let sqlite_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&db_path)
                    .create_if_missing(true),
            )
            .await?;

        match &self.reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => {
                self.download_for_ucsc_assembly(&self.reference, &sqlite_pool)
                    .await?
            }
            Reference::UcscAccession(_) => {
                self.download_for_ucsc_accession(&self.reference, &sqlite_pool)
                    .await?
            }
            _ => {
                return Err(TGVError::StateError(
                    "UcscApi cannot be used for a custom reference genome file.".to_string(),
                ));
            }
        }

        sqlite_pool.close().await;
        Ok(())
    }

    /// Download data for UCSC assemblies.
    /// 1. Transfer relevant tables from MariaDB to SQLite.
    /// 2. Find 2bit files from the chromInfo table. Download 2bit files.
    async fn download_for_ucsc_assembly(
        &self,
        reference: &Reference,
        sqlite_pool: &Pool<Sqlite>,
    ) -> Result<(), TGVError> {
        // Connect to MariaDB
        let mysql_url = UcscDbTrackService::get_mysql_url(&self.reference, &UcscHost::Us)?;
        let mysql_pool = MySqlPoolOptions::new()
            .max_connections(5)
            .connect(&mysql_url)
            .await?;

        self.transfer_table(&mysql_pool, sqlite_pool, "chromInfo")
            .await?
            .transfer_table(&mysql_pool, sqlite_pool, "chromAlias")
            .await?
            .transfer_table(&mysql_pool, sqlite_pool, "cytoBandIdeo")
            .await?
            .transfer_gene_tracks(&mysql_pool, sqlite_pool)
            .await?;

        mysql_pool.close().await;

        self.download_genomes(sqlite_pool).await?;

        println!(
            "Successfully downloaded track data for {}",
            reference.to_string(),
        );
        Ok(())
    }

    /// Download data for UCSC accessions
    /// 1. Use UCSC API get hub url
    /// 2. Parse hub file for 2bit file paths and bigbed file paths
    /// 3. Download 2bit files
    /// 4. Download Bigbed files. Convert to SQLite tables. (TODO: calculate exonStarts and exonEnds from blockStarts and blockSizes)
    async fn download_for_ucsc_accession(
        &self,
        reference: &Reference,
        sqlite_pool: &Pool<Sqlite>,
    ) -> Result<(), TGVError> {
        // 1. Get hub url
        let mut ucsc_api_service = UcscApiTrackService::new()?;
        let hub_url = ucsc_api_service
            .get_hub_url_for_genark_accession(&reference.to_string())
            .await?;
        self.download_to_directory(&hub_url).await?;

        // 2. Parse hub file and download files
        println!("Parsing hub file...");
        let hub_content = UcscHubFileParser::parse_hub_file(&hub_url).await?;

        if let Some(twobit_path) = &hub_content.twobit_path {
            self.download_to_directory(twobit_path).await?;
        }

        if let Some(chrom_info_path) = &hub_content.chrom_info_path {
            let local_chrom_info_path = self.download_to_directory(chrom_info_path).await?;
            self.add_chrom_info_to_sqlite(
                &local_chrom_info_path,
                hub_content.twobit_path.as_ref(),
                sqlite_pool,
            )
            .await?;
        }

        if let Some(chrom_alias_path) = &hub_content.chrom_alias_path {
            let local_chrom_alias_path = self.download_to_directory(chrom_alias_path).await?;
            BigBedConverter::save_to_sqlite(
                local_chrom_alias_path.to_str().unwrap(),
                "chromAlias",
                sqlite_pool,
            )
            .await?;
        }

        for (track_name, big_data_url) in hub_content.track_paths.iter() {
            if !(TRACK_PREFERENCES.contains(&track_name.as_str()) || track_name == "cytoBandIdeo") {
                continue;
            }

            let local_big_data_path = self.download_to_directory(big_data_url).await?;
            BigBedConverter::save_to_sqlite(
                local_big_data_path.to_str().unwrap(),
                track_name,
                sqlite_pool,
            )
            .await?;
        }

        Ok(())
    }

    /// Add the chromSizes txt file from UCSC to a sqlite table. Used for UCSC accession download.
    async fn add_chrom_info_to_sqlite(
        &self,
        chrom_info_path: &PathBuf,
        twobit_file_name: Option<&String>,
        sqlite_pool: &SqlitePool,
    ) -> Result<(), TGVError> {
        let chrom_info_content: String = std::fs::read_to_string(chrom_info_path)?;
        let chrom_info_lines = chrom_info_content.lines();

        println!("twobit_file_name: {:?}", twobit_file_name);

        sqlx::query("DROP TABLE IF EXISTS chromInfo")
            .execute(sqlite_pool)
            .await?;

        sqlx::query("CREATE TABLE chromInfo (chrom TEXT, size INTEGER, fileName TEXT)")
            .execute(sqlite_pool)
            .await?;

        for line in chrom_info_lines {
            let fields = line.split("\t").collect::<Vec<&str>>();
            let chrom = fields[0];
            let size = fields[1]
                .parse::<i64>()
                .map_err(|e| TGVError::IOError(format!("Failed to parse chrom size: {}", e)))?;

            sqlx::query("INSERT INTO chromInfo (chrom, size, fileName) VALUES (?, ?, ?)")
                .bind(chrom)
                .bind(size)
                .bind(match twobit_file_name {
                    Some(name) => name.split("/").last().unwrap(),
                    None => "",
                })
                .execute(sqlite_pool)
                .await?;
        }

        Ok(())
    }

    /// Transfer a MariaDb table to a SQLite table.
    async fn transfer_table(
        &self,
        mysql_pool: &MySqlPool,
        sqlite_pool: &SqlitePool,
        table_name: &str,
    ) -> Result<&Self, TGVError> {
        let mut column_types: HashMap<String, UCSCColumnType> = HashMap::new();

        // Check if table exists and get its structure
        let columns_info = sqlx::query(&format!("SHOW COLUMNS FROM {}", table_name))
            .fetch_all(mysql_pool)
            .await?;
        if columns_info.is_empty() {
            return Err(TGVError::IOError(format!(
                "{} table has no columns.",
                table_name
            )));
        }

        // Map MySQL types to SQLite types
        let mut column_defs: Vec<String> = Vec::new();
        let mut valid_columns = Vec::new();

        for col_info in &columns_info {
            let field_name: String = col_info.try_get("Field")?;
            let mysql_type: String = col_info.try_get("Type")?;

            match mysql_type.to_lowercase() {
                t if t.contains("int")
                    || t.contains("tinyint")
                    || t.contains("smallint")
                    || t.contains("mediumint")
                    || t.contains("bigint") =>
                {
                    if t.contains("unsigned") {
                        column_types.insert(field_name.clone(), UCSCColumnType::UnsignedInt);
                    } else {
                        column_types.insert(field_name.clone(), UCSCColumnType::Int);
                    }
                }
                t if t.contains("float")
                    || t.contains("double")
                    || t.contains("decimal")
                    || t.contains("numeric") =>
                {
                    column_types.insert(field_name.clone(), UCSCColumnType::Float);
                }
                t if t.contains("blob") || t.contains("binary") => {
                    column_types.insert(field_name.clone(), UCSCColumnType::Blob);
                }
                t if t.contains("char")
                    || t.contains("text")
                    || t.contains("varchar")
                    || t.contains("enum")
                    || t.contains("set") =>
                {
                    column_types.insert(field_name.clone(), UCSCColumnType::String);
                }
                _ => {
                    return Err(TGVError::IOError(format!(
                        "Skipping unsupported column type: {} {}",
                        field_name, mysql_type
                    )));
                }
            };

            column_defs.push(format!(
                "{} {}",
                field_name.clone(),
                column_types[&field_name].to_sqlite_type()
            ));
            valid_columns.push(field_name);
        }

        if valid_columns.is_empty() {
            return Err(TGVError::IOError(format!(
                "{} table has no supported columns.",
                table_name
            )));
        }

        // Drop existing table if it exists, then create fresh
        sqlx::query(&format!("DROP TABLE IF EXISTS {}", table_name))
            .execute(sqlite_pool)
            .await?;

        sqlx::query(&format!(
            "CREATE TABLE {} ({})",
            table_name,
            column_defs.join(", ")
        ))
        .execute(sqlite_pool)
        .await?;

        // Transfer data
        let select_columns = valid_columns.join(", ");
        let query_sql = format!("SELECT {} FROM {}", select_columns, table_name);
        let rows = sqlx::query(&query_sql).fetch_all(mysql_pool).await?;

        if rows.is_empty() {
            println!("{} table is empty, skipping data transfer", table_name);
            return Ok(self);
        }

        // Insert data
        let placeholders = vec!["?"; valid_columns.len()].join(", ");
        let insert_sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            table_name, select_columns, placeholders
        );

        let mut transaction: sqlx::Transaction<'_, sqlx::Sqlite> = sqlite_pool.begin().await?;
        for row in &rows {
            let mut query = sqlx::query(&insert_sql);
            for col_name in &valid_columns {
                // Bind values based on SQLite type
                match column_types[col_name] {
                    UCSCColumnType::UnsignedInt => {
                        let value: u64 = row.try_get(col_name.as_str())?;
                        query = query.bind(value as i64);
                    }
                    UCSCColumnType::Int => {
                        let value: i64 = row.try_get(col_name.as_str())?;
                        query = query.bind(value);
                    }
                    UCSCColumnType::Float => {
                        let value: f64 = row.try_get(col_name.as_str())?;
                        query = query.bind(value);
                    }
                    UCSCColumnType::Blob => {
                        let value: Vec<u8> = row.try_get(col_name.as_str())?;
                        query = query.bind(value);
                    }
                    UCSCColumnType::String => {
                        let value: String = row.try_get(col_name.as_str())?;
                        query = query.bind(value);
                    }
                }
            }
            query.execute(&mut *transaction).await?;
        }

        transaction.commit().await?;

        println!(
            "Transferred {} table ({} columns, {} rows)",
            table_name,
            valid_columns.len(),
            rows.len()
        );
        Ok(self)
    }

    /// Find relevant table names and tranfer them from MariaDB to SQLite. Used for UCSC assembly download.
    async fn transfer_gene_tracks(
        &self,
        mysql_pool: &MySqlPool,
        sqlite_pool: &SqlitePool,
    ) -> Result<&Self, TGVError> {
        // Get list of available gene tracks
        let table_rows = sqlx::query("SHOW TABLES").fetch_all(mysql_pool).await?;

        let available_tracks: Vec<String> = table_rows
            .into_iter()
            .map(|row| row.try_get::<String, usize>(0))
            .collect::<Result<Vec<String>, sqlx::Error>>()?;

        let preferred_track = get_preferred_track_name_from_vec(&available_tracks)?;

        if let Some(track_name) = preferred_track {
            self.transfer_table(mysql_pool, sqlite_pool, &track_name)
                .await?;
        } else {
            println!("No preferred gene track found");
        }

        Ok(self)
    }

    /// Used for UCSC assembly download.
    /// Query the chromInfo table to get the genome file urls and download them.
    async fn download_genomes(&self, sqlite_pool: &SqlitePool) -> Result<(), TGVError> {
        println!("Downloading genome files...");

        // Query SQLite for unique fileName values from chromInfo table
        let rows = sqlx::query(
            "SELECT DISTINCT fileName FROM chromInfo WHERE fileName IS NOT NULL AND fileName != ''",
        )
        .fetch_all(sqlite_pool)
        .await?;

        if rows.is_empty() {
            println!("No genome files found in chromInfo table");
            return Ok(());
        }

        for row in rows {
            let file_name: String = row.try_get("fileName")?;
            let download_url = format!("http://hgdownload.soe.ucsc.edu/{}", file_name);

            self.download_to_directory(&download_url).await?;
        }

        println!("Genome file download completed");
        Ok(())
    }

    /// Download a file to a directory with the same filename.
    /// Skip if file already exists. Note that no cache invalidation here. TODO.
    async fn download_to_directory(&self, url: &str) -> Result<PathBuf, TGVError> {
        let local_path = Path::new(&self.cache_dir).join(url.split("/").last().unwrap());
        let client = reqwest::Client::new();

        println!("Downloading file: {}", local_path.display());

        // Download the file
        match client.get(url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    let content = response.bytes().await.map_err(|e| {
                        TGVError::IOError(format!("Failed to read response bytes: {}", e))
                    })?;

                    // Write file to disk
                    std::fs::write(&local_path, content).map_err(|e| {
                        TGVError::IOError(format!(
                            "Failed to write file {}: {}",
                            local_path.display(),
                            e
                        ))
                    })?;

                    println!("Downloaded: {}", local_path.display());
                } else {
                    println!(
                        "Failed to download {}: HTTP {}",
                        local_path.display(),
                        response.status()
                    );
                }
            }
            Err(e) => {
                println!("Failed to download {}: {}", local_path.display(), e);
            }
        }

        Ok(local_path)
    }
}

/// Convert BigBed files to SQLite database
pub struct BigBedConverter {}

impl BigBedConverter {
    /// Parse BigBed autosql header to sqlite schema.
    /// Bigbed autosql:
    ///   - https://www.linuxjournal.com/article/5949
    ///   - https://genome.ucsc.edu/goldenpath/help/examples/bedExample2.as
    ///
    /// Return: (field_names, field_types)
    fn get_schema(
        bigbed_reader: &mut BigBedRead<bigtools::utils::reopen::ReopenableFile>,
    ) -> Result<(Vec<String>, Vec<UCSCColumnType>), TGVError> {
        use bigtools::bed::autosql::parse::FieldType;
        use bigtools::bed::autosql::parse::parse_autosql;
        let autosql_string = bigbed_reader
            .autosql()?
            .ok_or(TGVError::IOError("Failed to parse autosql".to_string()))?;

        let mut field_names = Vec::new();
        let mut field_types = Vec::new();

        let declarations = parse_autosql(&autosql_string).unwrap();
        let declaration: &bigtools::bed::autosql::parse::Declaration = declarations.first().ok_or(
            TGVError::IOError("Parsing autosql declaration failed".to_string()),
        )?;

        for field in declaration.fields.iter() {
            if field.name == "chrom" || field.name == "chromStart" || field.name == "chromEnd" {
                // These are specially handled in bigtools.
                continue;
            }

            field_names.push(match field.name.as_str() {
                "thickStart" => "txStart".to_string(), // To be consistent with UCSC db.
                "thickEnd" => "txEnd".to_string(),     // To be consistent with UCSC db.
                name => name.to_string(),
            });
            field_types.push(match field.field_type {
                FieldType::Int | FieldType::Short | FieldType::Bigint => {
                    // Example: Field { field_type: Int, field_size: Some("blockCount"), name: "exonFrames", index_type: None, auto: false, comment: "\"Exon frame {0,1,2}, or -1 if no frame for exon\"" }
                    // NCBI's database did the same.
                    if field.field_size.is_none() {
                        UCSCColumnType::Int
                    } else {
                        UCSCColumnType::Blob
                    }
                }
                FieldType::Uint | FieldType::Ushort => {
                    if field.field_size.is_none() {
                        UCSCColumnType::UnsignedInt
                    } else {
                        UCSCColumnType::Blob
                    }
                }
                FieldType::Byte | FieldType::Ubyte => UCSCColumnType::Blob,
                FieldType::Double | FieldType::Float => {
                    if field.field_size.is_none() {
                        UCSCColumnType::Float
                    } else {
                        UCSCColumnType::Blob
                    }
                }
                FieldType::Char | FieldType::String | FieldType::Lstring => UCSCColumnType::String,
                // Not sure what's the best way tp interprete them. Maybe they are uncommon.
                FieldType::Enum(_) | FieldType::Set(_) | FieldType::Declaration(_, _) => {
                    UCSCColumnType::String
                }
            });
        }

        Ok((field_names, field_types))
    }

    /// Save a BigBed file to a SQLite table.
    ///
    pub async fn save_to_sqlite(
        bigbed_path: &str,
        track_name: &str,
        sqlite_pool: &Pool<Sqlite>,
    ) -> Result<(), TGVError> {
        println!("Converting {} BigBed file to SQLite table...", track_name);

        use bigtools::BigBedRead;

        // Open BigBed file
        let mut bigbed_reader: BigBedRead<bigtools::utils::reopen::ReopenableFile> =
            BigBedRead::open_file(bigbed_path)?;

        let (field_names, schema) = Self::get_schema(&mut bigbed_reader)?;

        // Special case:
        // For gene tracks, bigBed files store blockSizes and chromStarts, but UCSC db stores exonStarts and exonEnds.
        // Convert them to be consistent.
        let need_exon_conversion = field_names.contains(&"blockSizes".to_string())
            && field_names.contains(&"chromStarts".to_string())
            && !(field_names.contains(&"exonStarts".to_string())
                && field_names.contains(&"exonEnds".to_string()));

        let chrom_starts_field_index = field_names
            .iter()
            .position(|name| name == "chromStarts")
            .unwrap_or(0);
        let block_sizes_field_index = field_names
            .iter()
            .position(|name| name == "blockSizes")
            .unwrap_or(0);

        // Drop existing table if it exists
        sqlx::query(&format!("DROP TABLE IF EXISTS {}", track_name))
            .execute(sqlite_pool)
            .await?;

        // Create table with gene track schema

        let mut query = String::new();
        query.push_str("chrom TEXT, chromStart INTEGER, chromEnd INTEGER, ");
        for (i, (field_name, field_type)) in field_names.iter().zip(schema.iter()).enumerate() {
            query.push_str(&format!("{} {}", field_name, field_type.to_sqlite_type()));
            if i < field_names.len() - 1 {
                query.push_str(", ");
            }
        }

        if need_exon_conversion {
            query.push_str(", exonStarts BLOB, exonEnds BLOB, cdsStart INTEGER, cdsEnd INTEGER");
        }

        let query_string = format!("CREATE TABLE {} ({})", track_name, query);

        sqlx::query(&query_string).execute(sqlite_pool).await?;

        let mut transaction: sqlx::Transaction<'_, sqlx::Sqlite> = sqlite_pool.begin().await?;

        // Get all chromosomes from BigBed
        let mut record_count = 0;
        let chromosomes: Vec<(String, u32)> = bigbed_reader
            .chroms()
            .iter()
            .map(|chrom_info| (chrom_info.name.clone(), chrom_info.length))
            .collect();

        for (chromosome_name, chromosome_length) in chromosomes {
            for interval in bigbed_reader.get_interval(&chromosome_name, 0, chromosome_length)? {
                let interval: bigtools::BedEntry = interval?;
                let fields: Vec<&str> = interval.rest.split('\t').collect();

                if fields.len() != field_names.len() {
                    return Err(TGVError::ValueError(format!(
                        "Expected {} fields, got {}. expected fields: {}, got fields: {}",
                        field_names.len(),
                        fields.len(),
                        field_names.join(", "),
                        fields.join("; ")
                    )));
                }

                let query_string = format!(
                    "INSERT INTO {} ({}) VALUES ({})",
                    track_name,
                    "chrom, chromStart, chromEnd, ".to_string()
                        + &field_names.join(", ")
                        + if need_exon_conversion {
                            ", exonStarts, exonEnds, cdsStart, cdsEnd"
                        } else {
                            ""
                        },
                    vec!["?"; field_names.len() + 3 + if need_exon_conversion { 4 } else { 0 }]
                        .join(", ")
                );

                let mut query: sqlx::query::Query<'_, Sqlite, sqlx::sqlite::SqliteArguments<'_>> =
                    sqlx::query(&query_string);
                query = query
                    .bind(chromosome_name.clone())
                    .bind(interval.start as i64)
                    .bind(interval.end as i64);

                for (i, field) in fields.iter().enumerate() {
                    // If field emtpy, use null.
                    if field.trim().is_empty() {
                        match schema[i] {
                            UCSCColumnType::Int => {
                                query = query.bind(None::<i64>);
                            }
                            UCSCColumnType::UnsignedInt => {
                                query = query.bind(None::<i64>);
                            }
                            UCSCColumnType::Float => {
                                query = query.bind(None::<f64>);
                            }
                            UCSCColumnType::Blob => {
                                query = query.bind(None::<Vec<u8>>);
                            }
                            UCSCColumnType::String => {
                                query = query.bind(None::<String>);
                            }
                        }
                    } else {
                        match schema[i] {
                            UCSCColumnType::Int => {
                                query = query.bind(field.parse::<i64>().unwrap());
                            }
                            UCSCColumnType::UnsignedInt => {
                                query = query.bind(field.parse::<i64>().unwrap());
                            }
                            UCSCColumnType::Float => {
                                query = query.bind(field.parse::<f64>().unwrap());
                            }
                            UCSCColumnType::Blob => {
                                query = query.bind(field.as_bytes());
                            }
                            UCSCColumnType::String => {
                                query = query.bind(field.to_string());
                            }
                        }
                    }
                }

                // exonStarts and exonEnds calculation
                if need_exon_conversion {
                    let (exon_starts_blob, exon_ends_blob, cds_start, cds_end) =
                        Self::convert_blocks_to_exons(
                            interval.start,
                            fields[block_sizes_field_index].as_bytes().to_vec(),
                            fields[chrom_starts_field_index].as_bytes().to_vec(),
                        )?;
                    query = query
                        .bind(exon_starts_blob)
                        .bind(exon_ends_blob)
                        .bind(cds_start)
                        .bind(cds_end);
                }

                query.execute(&mut *transaction).await?;

                record_count += 1;
            }
        }

        transaction.commit().await?;

        // Special case:
        // For gene tracks, bigBed files store blockSizes and chromStarts, but UCSC db stores exonStarts and exonEnds.
        // Convert them to be consistent.

        // if need_exon_conversion {
        //     Self::add_exon_columns(track_name, sqlite_pool).await?;
        // }

        println!(
            "Successfully converted {} records from BigBed to SQLite table '{}'",
            record_count, track_name
        );

        Ok(())
    }

    // Note: UCSC database store exon information with exonStarts and exonEnds. But the bigbed files stores blockStarts and blockSize.
    // Compute them to new columns to reduce compute at run time.
    /// Return: (exonStarts (blob), exonEnds (blob), cdsStart (u32), cdsEnd (u32))
    fn convert_blocks_to_exons(
        chrom_start: u32,
        block_sizes_blob: Vec<u8>,
        chrom_starts_blob: Vec<u8>,
    ) -> Result<(Vec<u8>, Vec<u8>, u32, u32), TGVError> {
        let block_sizes_str = String::from_utf8(block_sizes_blob).map_err(|e| {
            TGVError::ValueError(format!("Failed to convert block sizes to string: {}", e))
        })?;
        let chrom_starts_str = String::from_utf8(chrom_starts_blob).map_err(|e| {
            TGVError::ValueError(format!("Failed to convert block starts to string: {}", e))
        })?;

        // Example block_sizes_str, chrom_starts_str:
        // 66,
        // 0,

        let block_sizes: Vec<u32> = block_sizes_str
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().parse::<u32>().unwrap())
            .collect();

        let chrom_starts: Vec<u32> = chrom_starts_str
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().parse::<u32>().unwrap())
            .collect();

        if block_sizes.len() != chrom_starts.len() {
            return Err(TGVError::ValueError(
                "blockSizes and chromStarts arrays have different lengths".to_string(),
            ));
        }

        let mut exon_starts = Vec::new();
        let mut exon_ends = Vec::new();

        for (block_start, block_size) in chrom_starts.iter().zip(block_sizes.iter()) {
            let exon_start = chrom_start + *block_start;
            let exon_end = exon_start + *block_size;
            exon_starts.push(exon_start);
            exon_ends.push(exon_end);
        }

        let cds_start = exon_starts[0];
        let cds_end = exon_ends[exon_ends.len() - 1];

        // Convert to blob
        let exon_starts_str = exon_starts
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let exon_starts_blob = exon_starts_str.into_bytes();
        let exon_ends_str = exon_ends
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let exon_ends_blob = exon_ends_str.into_bytes();

        Ok((exon_starts_blob, exon_ends_blob, cds_start, cds_end))
    }
}

/// A parsed UCSC hub file. Example: https://hgdownload.soe.ucsc.edu/hubs/GCF/000/005/845/GCF_000005845.2/hub.txt
#[derive(Debug, Clone)]
struct UcscHub {
    hub_url: String,

    twobit_path: Option<String>,

    chrom_info_path: Option<String>,

    chrom_alias_path: Option<String>,

    track_paths: HashMap<String, String>,
}

/// Parse UCSC hub.txt files.
/// Example: https://hgdownload.soe.ucsc.edu/hubs/GCF/000/005/845/GCF_000005845.2/hub.txt
struct UcscHubFileParser {}

impl UcscHubFileParser {
    pub async fn parse_hub_file(hub_url: &str) -> Result<UcscHub, TGVError> {
        let response = reqwest::get(hub_url).await?;
        let body = response.text().await?;

        let mut current_track = None; // track name of the current track block being parsed
        let mut track_paths: HashMap<String, String> = HashMap::new();
        let mut twobit_path = None;
        let mut chrom_info_path = None;
        let mut chrom_alias_path = None;

        for line in body.lines() {
            if line.is_empty() {
                current_track = None;
                continue;
            }

            let values = line.split(" ").collect::<Vec<&str>>();
            if values[0] == "track" {
                current_track = Some(values[1].to_string());
                continue;
            }

            if let Some(track_name) = &current_track {
                if values[0] == "bigDataUrl" {
                    track_paths.insert(track_name.clone(), Self::join_url(hub_url, values[1]));
                }
            } else if values[0] == "twoBitPath" {
                twobit_path = Some(Self::join_url(hub_url, values[1]));
            } else if values[0] == "chromSizes" {
                chrom_info_path = Some(Self::join_url(hub_url, values[1]));
            } else if values[0] == "chromAliasBb" {
                chrom_alias_path = Some(Self::join_url(hub_url, values[1]));
            }
        }

        Ok(UcscHub {
            hub_url: hub_url.to_string(),
            twobit_path,
            chrom_info_path,
            chrom_alias_path,
            track_paths,
        })
    }

    /// Replace the last part of the hub url with the file name
    fn join_url(hub_url: &str, file_name: &str) -> String {
        let base_url = hub_url.split("/").collect::<Vec<&str>>();
        let base_url = base_url[..base_url.len() - 1].join("/");
        let full_url = format!("{}/{}", base_url, file_name);
        full_url
    }
}

fn get_preferred_track_name_from_vec(names: &Vec<String>) -> Result<Option<String>, TGVError> {
    for pref in TRACK_PREFERENCES {
        if names.contains(&pref.to_string()) {
            return Ok(Some(pref.to_string()));
        }
    }

    Ok(None)
}
