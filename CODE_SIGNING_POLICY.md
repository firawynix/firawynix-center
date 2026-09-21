# Code signing policy

## Scope

The Firawynix Center signing identity covers release artifacts built from this
repository. It doesn't cover private Firawynix products, game clients, remote
administration editions, or third-party applications downloaded by a catalog.

## Source and build origin

Release artifacts must:

- originate from this public repository;
- correspond to a signed or protected version tag;
- be built by the checked-in GitHub Actions workflow;
- use dependencies declared in `package-lock.json` and `Cargo.lock`; and
- pass the frontend build and Rust test suite.

## Approval

A maintainer reviews each release commit and approves each public signing
request. A signing request must identify the repository, commit, workflow run,
artifact name, version, and SHA-256 digest.

## Key protection

Private signing keys aren't stored in this repository or on developer machines
when a managed signing service is used. Test certificates may sign local builds,
but they aren't published as trusted production releases.

## Verification

Each release publishes SHA-256 checksums. Users must verify that the signer and
checksum match the information on the GitHub release page.

## Revocation and incidents

Maintainers stop signing immediately if the repository, build pipeline, or
signing account may be compromised. They revoke affected credentials, publish a
security advisory, and issue a clean release only after completing an audit.

