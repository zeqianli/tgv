# Design: signing and notarizing the macOS binary

Status: proposed, not implemented.

## Context

`.github/workflows/release.yml` builds `aarch64-apple-darwin` on `macos-latest`, packs it into a tarball, and updates the Homebrew formula in `zeqianli/homebrew-tgv`.

- Gatekeeper only checks files with the quarantine attribute, which browsers and some apps add to downloads. `curl` and Homebrew formulas don't add it, so `brew install` users run the unsigned binary without a warning.
- Users who download the tarball from a GitHub release in a browser see "Apple cannot check it for malicious software". Today they have to run `xattr -d com.apple.quarantine tgv`.
- Apple Silicon requires every binary to have at least an ad-hoc signature. The Rust linker already adds one, so the binary runs. What is missing is a Developer ID signature and notarization.

## Plan

### One-time setup

1. **Join the Apple Developer Program** as an individual (developer.apple.com, $99 a year). This gives a 10-character Team ID.
2. **Create a Developer ID Application certificate.**
   - On a Mac: Keychain Access → Certificate Assistant → *Request a Certificate from a Certificate Authority*.
   - Upload the request at developer.apple.com → Certificates → **+** → *Developer ID Application*.
   - Download the certificate, add it to the keychain, and export it with its private key as `devid.p12`, with a password.
   - Without a Mac, `rcodesign` can generate the request on Linux.
3. **Create an App Store Connect API key** for notarization: App Store Connect → Users and Access → Integrations → App Store Connect API → **+**, with the Developer role.
   - Download `AuthKey_XXXX.p8`. It can only be downloaded once.
   - Note the Key ID and the Issuer ID.

### Signing and notarizing by hand

```sh
# Sign with the hardened runtime, which notarization requires.
codesign --force --timestamp --options runtime \
  --sign "Developer ID Application: Your Name (TEAMID)" tgv

# Notarization takes a zip, not a bare binary.
ditto -c -k --keepParent tgv tgv.zip
xcrun notarytool submit tgv.zip \
  --key AuthKey_XXXX.p8 --key-id XXXX --issuer <issuer-uuid> --wait

# Check the result.
codesign -dv --verbose=4 tgv     # Shows "Authority=Developer ID Application: …".
spctl --assess --type execute -vv tgv
```

A notarization ticket can't be stapled to a bare binary. Gatekeeper looks up the ticket online the first time the binary runs.

### Release workflow

Use [`rcodesign`](https://github.com/indygreg/apple-platform-rs). It signs and notarizes without the macOS keychain, and also runs on Linux.

1. **Store secrets:**
   - `MACOS_CERT_P12`: the `.p12`, base64-encoded.
   - `MACOS_CERT_PASSWORD`.
   - The API key as JSON, from `rcodesign encode-app-store-connect-api-key` with the Issuer ID, Key ID, and `.p8`.
2. **Sign and notarize** in `build-macos-arm`, between `cargo build` and the `tar` step:
   ```sh
   rcodesign sign --p12-file devid.p12 --p12-password "$MACOS_CERT_PASSWORD" \
     --code-signature-flags runtime target/aarch64-apple-darwin/release/tgv
   ditto -c -k --keepParent target/aarch64-apple-darwin/release/tgv tgv.zip
   rcodesign notary-submit --api-key-file api-key.json --wait tgv.zip
   ```
3. **Checksums:** keep the checksum step after signing, so the Homebrew formula matches the signed tarball.

`codesign` and `notarytool` also work in CI. They need a temporary keychain set up with `security create-keychain`, `security import`, and `security set-key-partition-list`, which is more setup to get right.

## Verification

- Download the release tarball in Safari, extract it, and run `tgv`. It starts without a Gatekeeper warning.
- `spctl --assess --type execute -vv tgv` reports `source=Notarized Developer ID`.
- `brew install` still works, and the formula's checksum matches.

## Open questions

- Is a direct-download warning worth $99 a year, given that Homebrew users never see it?
- Should the release also build `x86_64-apple-darwin`, or a universal binary, for Intel Macs?
