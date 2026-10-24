# Windows signing with SignPath Foundation

SignPath Foundation signs open-source builds free with an HSM-backed OV
certificate in the Foundation's name. Signing identifies the publisher; it does
not guarantee immediate SmartScreen reputation.

## Prerequisites

- Public, OSI-licensed repository: `holuto-org/workshop` uses MIT.
- Published release: <https://github.com/holuto-org/workshop/releases/latest>
  must resolve.
- GitHub Actions builds on GitHub-hosted runners, as in
  `.github/workflows/release.yml`. SignPath verifies workflow artifacts;
  manually uploaded binaries are insufficient.
- GitHub organization admin access to install the SignPath App and configure
  secrets.

SignPath prefers established projects. Describe real users in the application;
new projects may be asked to reapply.

## 1. Apply

Use <https://signpath.org/apply> (embedded HubSpot form):

- **Project Name**: `Holuto Workshop`
- **Repository URL**: `https://github.com/holuto-org/workshop`
- **Homepage URL**: Repository URL, or `https://holuto.com` if it links to the
  repository
- **Download URL**: `https://github.com/holuto-org/workshop/releases/latest`
- **Privacy Policy URL**: Applicable policy for the app's Holuto API
  communication
- **Wikipedia URL**: Leave empty
- **Tagline**: Desktop app and driver for Posnet Temo Online fiscal printers
  used in Polish car workshops
- **Description**: Holuto Workshop is a Rust/Slint desktop app for
  Holuto-queued fiscal receipts and daily, monthly and periodic reports on
  Posnet Temo Online printers over USB. The MIT-licensed repository includes
  the Posnet driver and background worker. GitHub Actions builds Windows,
  macOS and Linux packages on version increases. We are applying to sign the
  Windows executable.
- **Reputation**: Describe production use by Polish workshops, Holuto's
  maintenance of the app and backend, and the September 2026 open-source
  release. Add verified workshop/user counts and review/CI practices.
- **Maintainer Type**: Company / organization
- **First Name, Last Name, Email**: Applicant details; prefer an `@holuto.com`
  address matching the organization domain
- **Company Name**: `Holuto`
- **Discovery source**: Optional
- **Code of Conduct**: Read <https://signpath.org/terms> before accepting.
  Builds must come from public-source CI and match the repository; violations
  can revoke certificates.
- **Data processing**: Required consent
- **Other communications**: Optional

## 2. Configure SignPath

After approval, accept the emailed invitation to <https://app.signpath.io>.

1. Copy **Organization → Settings → Organization ID**.
2. Create or open the repository's project; copy its slug from the URL.
3. Select the trusted **`release-signing`** policy. Foundation reviewers may
   need to approve the first requests.
4. Configure the artifact as the executable inside `actions/upload-artifact`'s
   ZIP:

   ```xml
   <artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
     <zip-file>
       <pe-file path="workshop-win-x64.exe">
         <authenticode-sign />
       </pe-file>
     </zip-file>
   </artifact-configuration>
   ```

5. Create a CI user with project **Submitter** permission; generate its API
   token.
6. Install <https://github.com/apps/signpath> for `holuto-org/workshop` so
   SignPath can inspect workflow runs.
7. Under **Organization → Trusted Build Systems**, add **GitHub.com** and link
   the project.

## 3. Configure GitHub

In **Settings → Secrets and variables → Actions**, add:

| Name                           | Kind     | Value               |
| ------------------------------ | -------- | ------------------- |
| `SIGNPATH_API_TOKEN`           | Secret   | CI user's API token |
| `SIGNPATH_ORGANIZATION_ID`     | Variable | Organization ID     |
| `SIGNPATH_PROJECT_SLUG`        | Variable | Project slug        |
| `SIGNPATH_SIGNING_POLICY_SLUG` | Variable | `release-signing`   |

## 4. Update the workflow

In `.github/workflows/package.yml`, insert these steps after
`bash packaging/build.sh` and before the final artifact upload. SignPath
retrieves the unsigned artifact from GitHub, then replaces the executable in
`release/` for the final upload.

```yaml
- if: runner.os == 'Windows'
  id: unsigned
  uses: actions/upload-artifact@v7
  with:
    name: win-x64-unsigned
    path: release/workshop-win-x64.exe
    retention-days: 1

- if: runner.os == 'Windows'
  uses: signpath/github-action-submit-signing-request@v2
  with:
    api-token: ${{ secrets.SIGNPATH_API_TOKEN }}
    organization-id: ${{ vars.SIGNPATH_ORGANIZATION_ID }}
    project-slug: ${{ vars.SIGNPATH_PROJECT_SLUG }}
    signing-policy-slug: ${{ vars.SIGNPATH_SIGNING_POLICY_SLUG }}
    github-artifact-id: ${{ steps.unsigned.outputs.artifact-id }}
    wait-for-completion: true
    output-artifact-directory: release
```

In `checksums` and `publish`, restrict `actions/download-artifact` to the final
matrix artifacts (and `checksums` for publishing). Exclude `win-x64-unsigned`:
merging it could overwrite the signed executable.

For unsigned builds without secrets (such as fork PRs), set a job-level
environment variable from the secret and gate both steps on it.
[Secrets cannot appear directly in `if` conditions](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/use-secrets#using-secrets-in-a-workflow):

```yaml
env:
  SIGNPATH_API_TOKEN: ${{ secrets.SIGNPATH_API_TOKEN }}
```

```yaml
if: runner.os == 'Windows' && env.SIGNPATH_API_TOKEN != ''
```

## 5. Verify

After the first signed release, run on Windows:

```powershell
Get-AuthenticodeSignature .\workshop-win-x64.exe | Format-List
```

Expect `Status: Valid` and _SignPath Foundation_ in `SignerCertificate.Subject`.
SmartScreen warnings may persist while reputation builds.

## Branch policies

SignPath supports optional
[GitHub source/build policies](https://docs.signpath.io/trusted-build-systems/github#pipeline-policies-for-github),
including on Open Source Code Signing plans. Constraints must match active
GitHub rulesets. For example, `allow_bypass_actors: false` requires disabling
bypasses; a pull-request constraint with one approval requires that approval in
GitHub. `enforced_from: EARLIEST` requires continuous compliance, not just
today's settings. Check current rulesets before adopting constraints; local
workflow files do not prove repository settings.

## Other signing options

- **Azure Artifact Signing** (formerly Trusted Signing): about USD 10/month, no
  purchased certificate; `azure/trusted-signing-action`. Requires an Azure
  tenant and an eligible company with three years of verifiable history. An
  option for a private repository.
- **Commercial OV/EV** (Sectigo, DigiCert, SSL.com): roughly USD 200–400/year;
  private key stored on a hardware token or CA cloud HSM.
- **macOS**: Apple Developer Program (USD 99/year), a **Developer ID
  Application** certificate and notarization. In `packaging/build.sh`, sign the
  `.app` with `codesign --options runtime`, submit the `.dmg` with
  `xcrun notarytool submit --wait`, then `xcrun stapler staple`. Required
  secrets: base64 `.p12`, certificate password, Apple ID, app-specific password
  and Team ID.
- **Linux**: `.deb` packages are not signed by these steps.
