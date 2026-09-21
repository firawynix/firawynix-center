# Firawynix Center

Firawynix Center is an open-source desktop catalog and launcher for Windows and
Linux. It presents web projects, installs native applications, detects existing
installations, and consumes a configurable remote catalog.

The desktop application uses React, TypeScript, Rust, and Tauri 2. The source
doesn't contain production credentials, private signing keys, or privileged
editions of other Firawynix products.

## Features

- Display games, web projects, and native applications from one catalog.
- Install and open native applications on Windows and Linux.
- Detect existing installations and offer controlled cleanup.
- Cache the last valid catalog for offline use.
- Verify downloads when the catalog provides a SHA-256 hash.
- Use a custom catalog API at build time.

## Build from source

Install Node.js 20 or later, Rust stable, and the platform prerequisites listed
in the [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/).

Run the following commands:

```text
npm ci
npm run build
npm run tauri:build
```

The default build reads the public Firawynix catalog. To build for another
catalog service, set `FIRAWYNIX_CENTER_API` before running the build:

```text
FIRAWYNIX_CENTER_API=https://catalog.example.org npm run tauri:build
```

On PowerShell, use:

```powershell
$env:FIRAWYNIX_CENTER_API = 'https://catalog.example.org'
npm run tauri:build
```

The embedded fallback catalog contains only neutral examples. Production
entries arrive from the configured API and aren't required to compile the app.

## Verify the project

Run the frontend checks and Rust tests before opening a pull request:

```text
npm run typecheck
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

GitHub Actions performs the same checks on Windows and Linux.

## Repository safety

Don't commit certificates, encrypted private keys, credentials, deployment
hostnames, private catalogs, or production configuration. Use repository
secrets for CI credentials and local environment variables for development.

Report vulnerabilities through
[GitHub private vulnerability reporting](../../security/advisories/new).
Read the [privacy policy](PRIVACY.md) for the application's network and local
storage behavior.

## Code signing policy

Release artifacts are built from tagged commits by GitHub Actions. Public
release signing is requested only for artifacts whose source and build scripts
are present in this repository. Maintainers don't sign private editions or
unrelated binaries with the project's signing identity.

Read the complete [code signing policy](CODE_SIGNING_POLICY.md).

Public builds are available on the
[GitHub releases page](https://github.com/firawynix/firawynix-center/releases).

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before submitting a change.

## License

Firawynix Center is available under the [MIT License](LICENSE).
