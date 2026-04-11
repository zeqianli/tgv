# Troubleshoot

## Remote files

Remote BAM files might run into a [curl certificate issue](https://github.com/rust-bio/rust-htslib/issues/404). The solution is setting the `CURL_CA_BUNDLE` environmental variable:

```bash
export CURL_CA_BUNDLE="/etc/ssl/certs/ca-certificates.crt"
# Or, the correct certificate path

tgv s3://path/to.bam
```
