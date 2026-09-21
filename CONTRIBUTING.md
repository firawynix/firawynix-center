# Contributing

## Prepare your environment

1. Install Node.js 20 or later and Rust stable.
2. Install the Tauri prerequisites for your operating system.
3. Run `npm ci` in the repository root.

## Submit a change

1. Create a focused branch from `main`.
2. Keep credentials, certificates, personal data, and private catalog entries
   out of commits.
3. Run `npm run typecheck` and `npm run build`.
4. Run `cargo test --manifest-path src-tauri/Cargo.toml --locked`.
5. Open a pull request that explains the behavior and verification performed.

All contributions are licensed under the MIT License.

