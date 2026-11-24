# Release

How a desktop release is cut. The tag is the owner's to push.

- Pushing a tag `v<version>` runs `.github/workflows/release.yml`: it fails unless the tag's commit is on `main` and passes `.github/workflows/ci.yml` (which also runs on every pull request and push to `main`), then builds `vxr.app` with one universal binary (`crates/vxr-app/bundle`), publishes the tag's GitHub release with `vxr-<version>-universal-apple-darwin.tar.gz` and its `.sha256`, and points `Formula/vxr.rb` in jrnxf/homebrew-tap at it.
- Development builds are tagged `v0.1.0-pre.N` and published as pre-releases; the build stamps the tag's version in, so no commit is needed. The formula follows them until the first full release (`v0.1.0`, which Cargo.toml must say), then only full releases.
- `crates/vxr-app/bundle <out-dir>` builds the same `vxr.app` for this Mac from a checkout.
