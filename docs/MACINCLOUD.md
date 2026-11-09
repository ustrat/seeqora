# iOS on MacInCloud

Use MacInCloud to run the app in the Simulator, sign it, and upload to TestFlight. GitHub Actions already builds the iOS app on every push (the `ios` job in [`ci`](../.github/workflows/core.yml)), so compile errors show up there before you open a session.

## One-time setup (per Mac)

Everything installs per user, so no admin or Homebrew is needed.

```sh
# Xcode 15+ is preinstalled on MacInCloud. Check:
xcodebuild -version

# Rust (installs into ~/.cargo, no sudo)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

git clone https://github.com/ustrat/seeqora.git
cd seeqora
```

## Build and run

```sh
git pull
bash scripts/build-ios.sh     # Rust core -> XCFramework + Swift bindings + Xcode project
open ios/Seeqora.xcodeproj
```

In Xcode, choose an iPhone simulator and press Run. The screen should show **Seeqora** and **Core 0.1.0**, which proves the Rust core is linked.

Run `scripts/build-ios.sh` again whenever `core/` or `ios/project.yml` changes. The `.xcodeproj` is generated, so don't edit it by hand. Put changes in `ios/project.yml`.

## Signing and TestFlight

1. Xcode → target **Seeqora** → Signing & Capabilities → choose your team. The bundle id is `com.ustrat.seeqora`; change it in `ios/project.yml` if it's not final.
2. Create the app record in App Store Connect with the same bundle id.
3. Product → Archive → Distribute App → App Store Connect → Upload.

To avoid hand-signing, add an App Store Connect API key to GitHub secrets later and let CI archive and upload. That way each weekly build reaches TestFlight without a MacInCloud session.
