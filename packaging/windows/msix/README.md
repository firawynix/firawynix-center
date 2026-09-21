# Windows MSIX packaging

The packaging script creates two distinct channels:

- `Store` uses the identity reserved in Microsoft Partner Center and remains
  unsigned because Microsoft signs an approved MSIX.
- `Site` uses the independent `Firawynix.Center` identity and requires a code
  signing certificate whose subject matches the package publisher.

Build the Windows executable before creating the package:

```powershell
npm ci
npm run build
cargo build --manifest-path src-tauri/Cargo.toml --release --locked
```

Create an unsigned Store package:

```powershell
.\tools\msix\build-msix.ps1 `
  -Config .\packaging\windows\msix\firawynix-center.json `
  -Channel Store `
  -Publisher 'CN=REPLACE_WITH_PARTNER_CENTER_PUBLISHER'
```

Create a signed package for local testing:

```powershell
.\tools\msix\build-msix.ps1 `
  -Config .\packaging\windows\msix\firawynix-center.json `
  -Channel Site `
  -Publisher 'CN=YOUR_TEST_CERTIFICATE' `
  -CertificateThumbprint 'YOUR_CERTIFICATE_THUMBPRINT'
```

Never commit a private certificate or signing password.

