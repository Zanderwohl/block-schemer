# Releases

`.github/workflows/release.yml` builds Block Schemer for Linux, macOS and
Windows and attaches the builds to a draft GitHub release. It runs only when
started by hand.

## Making a release

1. Bump `version` under `[workspace.package]` in the root `Cargo.toml`. The
   macOS app shows it in Get Info and About.
2. Commit and push to `main`.
3. In the Actions tab, choose **Release → Run workflow**, pick `main`, and
   give a tag such as `v0.1.0`.
4. When it finishes, open the draft under Releases, edit the generated notes
   and publish. Publishing a draft creates its tag on the commit the
   workflow built.

With **Create a draft release** unticked, the builds are only uploaded to the
run (its Summary page, kept 90 days), which is the way to try the workflow
without touching Releases. Delete a draft you don't want; it made no tag.

## What it builds

| Job | Runner | Files |
|---|---|---|
| `linux` | `ubuntu-22.04` | `…-linux-x86_64.AppImage`, `…-linux-x86_64.tar.gz` |
| `macos` | `macos-latest` (Apple Silicon) | `…-macos-universal.dmg` |
| `windows` | `windows-latest` | `…-windows-x86_64.zip` |

- **Linux:** built on the oldest runner because the binary needs at least the
  glibc it was built against. It links only libc, libm and libgcc_s; eframe
  loads X11, Wayland and GL at run time. The tarball holds the binary, the
  README, `assets/linux/block-schemer.desktop` and the icon. The AppImage is
  made by `linuxdeploy` from the same desktop entry and icon, whose file name
  must match the entry's `Icon=`.
- **macOS:** `cargo bundle` makes the `.app` for Apple Silicon, then `lipo`
  replaces its binary with one holding both architectures. The `.dmg` has an
  Applications link to drag onto.
- **Windows:** `build.rs` embeds the icon. `main` sets
  `windows_subsystem = "windows"` in release builds, so no console opens
  beside the window.

Not built: Linux and Windows on ARM, an installer for Windows, and a Flatpak.
A Flatpak builds offline, so every crate must be listed in its manifest
(`flatpak-cargo-generator` writes that list from `Cargo.lock`); it is worth
doing to be on Flathub.

## Signing

Unsigned builds work, but the user has to get past a warning first:

- **macOS:** the app is only ad hoc signed (Apple Silicon runs no code
  without some signature), so Gatekeeper says it can't check it for malware,
  or that it is damaged. The user right-clicks it and chooses Open, allows it
  in System Settings → Privacy & Security, or runs
  `xattr -dr com.apple.quarantine "/Applications/Block Schemer.app"`.
- **Windows:** SmartScreen shows "Windows protected your PC"; More info → Run
  anyway.
- **Linux:** nothing to sign. Publishing a `SHA256SUMS` file beside the
  builds lets users check them.

Prices and eligibility below change; check them before paying.

### macOS: Developer ID and notarization

Needs the Apple Developer Program, $99 a year. An individual enrolls with an
Apple ID; an organization also needs a D-U-N-S number, which is free.

1. **Certificate.** In Certificates, Identifiers & Profiles, create a
   **Developer ID Application** certificate (only the account holder can).
   This needs a certificate signing request from Keychain Access
   (Certificate Assistant → Request a Certificate from a Certificate
   Authority). Install the result, then export it with its private key from
   Keychain Access as a `.p12` with a password.
2. **API key.** In App Store Connect → Users and Access → Integrations, make
   a key with the Developer role. Note its key ID and the issuer ID, and keep
   the `.p8`; it downloads once.
3. **Secrets.** In the repository's Settings → Secrets and variables →
   Actions:

   | Secret | Value |
   |---|---|
   | `MACOS_CERTIFICATE` | `base64 -i cert.p12` |
   | `MACOS_CERTIFICATE_PASSWORD` | the `.p12`'s password |
   | `MACOS_SIGNING_IDENTITY` | `Developer ID Application: Name (TEAMID)` |
   | `APPLE_API_KEY` | `base64 -i AuthKey_XXXX.p8` |
   | `APPLE_API_KEY_ID` | the key ID |
   | `APPLE_API_ISSUER` | the issuer ID |

4. **The workflow.** Replace the ad hoc `codesign` in the `macos` job with
   these, keeping `lipo` before and making the `.dmg` between signing the
   app and signing the `.dmg`:

   ```sh
   echo "$MACOS_CERTIFICATE" | base64 --decode > cert.p12
   security create-keychain -p ci build.keychain
   security default-keychain -s build.keychain
   security unlock-keychain -p ci build.keychain
   security import cert.p12 -k build.keychain -P "$MACOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
   security set-key-partition-list -S apple-tool:,apple: -s -k ci build.keychain

   codesign --force --options runtime --timestamp --sign "$MACOS_SIGNING_IDENTITY" "$app"
   # … hdiutil create as now …
   codesign --timestamp --sign "$MACOS_SIGNING_IDENTITY" "$dmg"

   echo "$APPLE_API_KEY" | base64 --decode > key.p8
   xcrun notarytool submit "$dmg" --key key.p8 --key-id "$APPLE_API_KEY_ID" \
     --issuer "$APPLE_API_ISSUER" --wait
   xcrun stapler staple "$dmg"
   ```

   `--options runtime` (the hardened runtime) is required for notarization.
   Steel interprets rather than compiling to machine code, so no
   entitlements should be needed; if a notarized build crashes where an
   unsigned one doesn't, check the hardened runtime first. Notarization
   takes from a minute to rarely an hour. Stapling attaches Apple's ticket
   to the `.dmg` so it opens without a network check.

5. **Check** on a Mac:

   ```sh
   spctl -a -vv -t install block-schemer-v0.1.0-macos-universal.dmg  # "source=Notarized Developer ID"
   codesign --verify --strict --verbose=2 "/Applications/Block Schemer.app"
   ```

   If `notarytool` reports Invalid, `xcrun notarytool log <id> --key …` says
   why.

### Windows: Authenticode

No Microsoft developer account is needed to sign files you distribute
yourself. The options, cheapest first:

- **Azure Artifact Signing** (until recently Trusted Signing): about $10 a
  month on an Azure subscription. Microsoft verifies your identity and holds
  the key; certificates last days and are renewed for you, and SmartScreen
  reputation follows your verified identity across them. Individuals and
  organizations are accepted only in some countries, so check before
  setting up.
- **A code signing certificate from a CA** (SSL.com, Sectigo, DigiCert,
  Certum, …): roughly $100–$500 a year. Since 2023 the private key must live
  on a hardware token or a cloud HSM, so for CI buy the CA's cloud signing
  service (SSL.com eSigner, DigiCert KeyLocker and so on). EV certificates
  no longer skip SmartScreen's reputation check, so OV is enough.
- **The Microsoft Store:** a developer account (free for individuals) and an
  MSIX package. The Store signs it and SmartScreen doesn't apply, but it
  only reaches people installing from the Store.

Even signed, a new identity's downloads can be warned about until enough
people have run them; signing makes the reputation accumulate rather than
start over with every release.

**With Artifact Signing:**

1. Make an Azure subscription, then an Artifact Signing account and an
   identity validation (Public Trust). Validation takes from hours to days.
2. Make a certificate profile from the validated identity.
3. Make an app registration (service principal) and give it the
   *Artifact Signing Certificate Profile Signer* role on the account.
4. Secrets: `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, plus
   the account's endpoint (e.g. `https://eus.codesigning.azure.net/`), its
   name and the profile name.
5. In the `windows` job, after `cargo build` and before packaging, sign
   `target/release/block-schemer.exe` with Microsoft's GitHub Action (it was
   `azure/trusted-signing-action`; check its current name), giving it those
   values, `files: target/release/block-schemer.exe`, `file-digest: SHA256`,
   `timestamp-rfc3161: http://timestamp.acs.microsoft.com` and
   `timestamp-digest: SHA256`.

**With a CA certificate**, sign the same file with the CA's tool or with
`signtool sign /fd SHA256 /tr <CA timestamp URL> /td SHA256 …` against the
cloud key, as the CA documents.

**Check** on Windows: `signtool verify /pa /v block-schemer.exe`, or the
file's Properties → Digital Signatures.

### Keeping unsigned builds working

Gate each signing step on its secrets so forks and a repository without them
still produce unsigned builds:

```yaml
macos:
  env:
    MACOS_CERTIFICATE: ${{ secrets.MACOS_CERTIFICATE }}
  steps:
    - name: Sign
      if: env.MACOS_CERTIFICATE != ''
```

Secrets can't be read in `if:` directly, and a step's own `env` isn't set yet
when its `if:` is evaluated, hence the job-level `env`.
