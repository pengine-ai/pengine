# Releasing pengine

Pengine ships installers for macOS, Windows, and Linux via the
[`App Release`](../../.github/workflows/app-release.yml) GitHub Actions
workflow. Pushing a tag matching `v*` (e.g. `v1.0.1`) triggers a build on each
platform and uploads the installers as assets on a **draft** GitHub Release.

```bash
git tag v1.0.1
git push origin v1.0.1
# watch the run at https://github.com/pengine-ai/pengine/actions
```

The workflow can also be triggered manually from the Actions tab.

## What "signed" and "authorized app" mean

When a user downloads pengine and double-clicks the installer, the OS asks a
simple question: *should I trust this?* Every modern desktop OS has a built-in
gatekeeper that blocks untrusted software by default.

- **macOS Gatekeeper + Notarization.** macOS requires apps distributed outside
  the App Store to be signed with an *Apple Developer ID* certificate and then
  *notarized* — scanned by Apple for malware and stapled with a ticket. An
  unsigned app triggers: *"pengine cannot be opened because the developer
  cannot be verified."* The user has to right-click → *Open* or allow it from
  *System Settings → Privacy & Security*. A signed+notarized app opens with a
  single *Are you sure?* prompt, then never asks again.
- **Windows SmartScreen / Mark-of-the-Web.** Windows flags any executable
  downloaded from the internet. An **unsigned** or low-reputation installer
  shows the blue *"Windows protected your PC"* screen; users must click *More
  info → Run anyway*. A signed installer from a trusted CA (Sectigo, DigiCert,
  SSL.com, Azure Trusted Signing, …) skips the blocking screen once the
  certificate has built up reputation (weeks, or instantly with an EV cert).
- **Linux.** No equivalent gating — `.deb` and `.AppImage` files run as-is,
  though GPG signing of the `.deb` is standard for apt repositories. Not
  covered here.

An **authorized app**, in this guide, means one whose installer is signed by a
recognized Certificate Authority so that macOS and Windows recognize the
publisher and skip (or soften) the "unknown developer" warning. The
workflow currently produces **unsigned** builds — the release pipeline works
end-to-end, but end users will see the warnings above until you add signing
credentials.

## macOS signing — how to obtain the secrets

Apple distribution needs **two** things: a Developer ID certificate (signs the
binary) and notarization credentials (submits it to Apple's scanner).
Prerequisite: an [Apple Developer Program](https://developer.apple.com/programs/)
membership (\$99/year, individual or organization).

**1. Create a Developer ID Application certificate.**

- On your Mac, open *Keychain Access* → menu *Certificate Assistant* →
  *Request a Certificate From a Certificate Authority…*
- Enter your email, leave *CA Email* blank, choose *Saved to disk* → save the
  `.certSigningRequest` file.
- Go to [developer.apple.com/account/resources/certificates](https://developer.apple.com/account/resources/certificates),
  click **+**, choose **Developer ID Application**, upload the CSR, download
  the resulting `.cer` file, double-click to install it into Keychain.

**2. Export the certificate as a `.p12`.**

- In Keychain Access, find the new certificate under *My Certificates*
  (expand it so the private key is included).
- Right-click → *Export* → format **Personal Information Exchange (.p12)** →
  choose a password when prompted (this becomes `APPLE_CERTIFICATE_PASSWORD`).

**3. Collect the six values.**

```bash
# APPLE_CERTIFICATE — base64 of the exported .p12
base64 -i certificate.p12 | pbcopy

# APPLE_SIGNING_IDENTITY — the full identity string, including "Developer ID Application: ... (TEAMID)"
security find-identity -v -p codesigning
```

| Secret | Where to get it |
| --- | --- |
| `APPLE_CERTIFICATE` | `base64 -i certificate.p12` (above) |
| `APPLE_CERTIFICATE_PASSWORD` | The password you chose when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | Output of `security find-identity -v -p codesigning` — use the full quoted string, e.g. `"Developer ID Application: Jane Doe (ABCD123456)"` |
| `APPLE_ID` | Your Apple ID email |
| `APPLE_PASSWORD` | An **app-specific password** generated at [appleid.apple.com](https://appleid.apple.com) → *Sign-In and Security → App-Specific Passwords*. **Not** your regular Apple ID password. |
| `APPLE_TEAM_ID` | Ten-character team ID at [developer.apple.com/account](https://developer.apple.com/account) → *Membership details* |

## Windows signing — how to obtain the secrets

Windows code signing requires a certificate issued by a recognized CA. Two
practical paths:

**Path A — OV code signing certificate (PFX file).** Buy an OV ("organization
validated") or IV ("individual validated") code-signing certificate from a CA
such as SSL.com (~\$199/yr), Sectigo, or DigiCert. You will complete a short
identity verification (business documents for OV, government ID for IV), then
the CA issues a `.pfx` / `.p12` file protected by a password. This works in CI
with just two secrets. The downside: the first few weeks of releases still
trigger SmartScreen warnings until the certificate accrues reputation.

> **EV (Extended Validation) certs** give instant SmartScreen reputation but
> the private key lives on a hardware USB token by design, so they can't be
> used directly from GitHub Actions without extra cloud-HSM setup. Skip unless
> you need instant reputation and are ready to configure Azure Key Vault or
> similar.

**Path B — Azure Trusted Signing** ([docs](https://learn.microsoft.com/en-us/azure/trusted-signing/)).
Microsoft's hosted signing service. ~\$10/month, no PFX file, uses a cloud
identity and an Azure subscription. Better long-term choice if you're already
in the Microsoft ecosystem, but wiring it up is different — ask before going
this route and we'll swap the signing step in the workflow.

For **Path A**, collect:

| Secret | Where to get it |
| --- | --- |
| `WINDOWS_CERTIFICATE` | `base64 -i certificate.pfx` (or `certutil -encode` on Windows) |
| `WINDOWS_CERTIFICATE_PASSWORD` | The password that protects the `.pfx` |

## Adding secrets to the repo

Once you have the values:

1. Go to the GitHub repo → *Settings* → *Secrets and variables* → *Actions*.
2. Click *New repository secret* and add each one. Names **must match exactly**
   what the workflow references.
3. Secrets are write-only once stored — if you need to change one, delete and
   re-add.

## Re-enabling signing in the workflow

The workflow currently has signing removed. Once your secrets are in place,
add signing back in two spots:

**1. A Windows pre-build step** that decodes the PFX and writes
`src-tauri/tauri.windows.conf.json` with a `signCommand`. Tauri auto-loads
platform-specific config files and invokes `signtool` on every bundled
artifact:

```yaml
- name: Configure Windows signing
  if: matrix.platform == 'windows-latest'
  shell: pwsh
  env:
    WINDOWS_CERTIFICATE: ${{ secrets.WINDOWS_CERTIFICATE }}
  run: |
    $certBytes = [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE)
    $certPath = Join-Path $env:RUNNER_TEMP 'cert.pfx'
    [IO.File]::WriteAllBytes($certPath, $certBytes)
    $escapedPath = $certPath -replace '\\', '\\\\'
    $signCommand = "signtool sign /fd SHA256 /f `"$escapedPath`" /p `"%WINDOWS_CERTIFICATE_PASSWORD%`" /tr http://timestamp.digicert.com /td SHA256 %1"
    $config = @{
      '$schema' = 'https://schema.tauri.app/config/2'
      bundle = @{ windows = @{ signCommand = $signCommand } }
    } | ConvertTo-Json -Depth 10
    Set-Content -Path 'src-tauri/tauri.windows.conf.json' -Value $config
```

Also add `src-tauri/tauri.windows.conf.json` to `.gitignore` — it's generated
per run.

**2. Env vars on the `tauri-action` step.** Add to the existing
`Build and release` step's `env:` block:

```yaml
env:
  GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
  APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
  APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
  APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
  APPLE_ID: ${{ secrets.APPLE_ID }}
  APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
  APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
  WINDOWS_CERTIFICATE_PASSWORD: ${{ secrets.WINDOWS_CERTIFICATE_PASSWORD }}
```

`tauri-action` sets up the macOS keychain and invokes `codesign` + `notarytool`
automatically when those env vars are present. No other changes needed.

## Verifying before a real release

After adding signing, cut a throwaway tag to confirm the full pipeline:

```bash
git tag v0.0.1-test && git push origin v0.0.1-test
```

The workflow produces a **draft** release — delete it from the Releases page
when done. If notarization fails, the macOS job log shows the `xcrun notarytool`
submission ID and status; most failures are a wrong app-specific password or
Team ID.
