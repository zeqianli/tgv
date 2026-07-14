# HTTPS download fallback

TGV normally builds a local reference cache by querying the UCSC MariaDB server. Some networks block outbound MariaDB traffic on port 3306 while allowing HTTPS, so the downloader gives the MariaDB connection a five-second acquisition timeout, then falls back to UCSC's HTTPS table dumps when the MariaDB download fails. A MariaDB attempt includes both the connection and table transfer; a failure at either stage starts the HTTPS fallback.

The HTTPS backend imports `chromInfo`, `chromAlias`, `cytoBandIdeo`, and the first available preferred gene table. UCSC publishes these tables as gzip-compressed, tab-separated dumps. TGV uses explicit SQLite schemas for the small cache contract instead of translating arbitrary MySQL DDL. This keeps SQLite types predictable for downstream readers, but a future change to a UCSC dump schema must be reflected in the corresponding schema definition in the downloader.

Rows are streamed from the HTTP response, decompressed, and inserted incrementally to bound memory use. All table changes share one SQLite transaction, and TGV downloads every sequence file referenced by `chromInfo` before committing it. A failed table or sequence download therefore leaves the previous database state intact. If both backends fail, TGV reports both the MariaDB error and the HTTPS error.

Sequence downloads use HTTPS and stream their response into a unique temporary file in the cache directory. Publishing uses a no-clobber operation, so concurrent TGV processes cannot write the same temporary file or replace a completed download. If another process publishes the destination first, both downloads treat the completed destination as the cache entry. Existing cache files are retained without revalidation.
