# HX Launcher

**Minecraft Java Edition** launcher built with **Tauri 2**, **React / TypeScript**, and a **Rust** backend. The interface is French, with isolated instances and CurseForge ZIP import installation.

## Getting started

Requirements: Node.js 22.12+ (or 24+), a recent stable Rust toolchain, [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), and Java to run Minecraft.

```sh
npm install
npm run tauri dev
```

The Vite server uses `127.0.0.1:15420`. `npm run dev` only displays the interface in a browser; native commands require Tauri.

```sh
npm run build                          # TypeScript + frontend
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build                    # Production bundle for the current machine
npm run tauri build -- --debug --bundles app  # Development macOS .app
```

## Implemented features

- Microsoft sign-in through a code displayed in the system browser.
- **`oauth2` 5** handles the device flow, backoff polling, expiration, and Microsoft token refresh.
- **`minecraft-msa-auth` 0.5** handles Microsoft → Xbox Live → Minecraft Services exchanges.
- Minecraft Java profile loading; no access or refresh token is sent to the frontend.
- Refresh tokens are stored in the system keychain with `keyring`; refresh on startup and before each launch, logout, and login cancellation are supported.
- Mojang stable versions from Minecraft 1.19 onward; instance creation and Java/RAM settings.
- Client, library, and asset downloads from Mojang manifests, SHA-1 verification, shared cache, and 12 concurrent asset downloads.
- JVM/game argument construction, platform rules, Java launch, and process tracking. Legacy native formats incompatible with ARM are rejected explicitly.
- Local **CurseForge ZIP** inspection: manifest v1, Minecraft version, primary modloader, `projectID` / `fileID` references, and override count. Path traversal, symbolic links, and oversized archives are rejected.

## Microsoft setup

The authentication library does not remove the need to register your application with Microsoft.

1. In **Microsoft Entra → App registrations**, create an application that accepts **personal Microsoft accounts**.
2. In **Authentication → Advanced settings**, enable **Allow public client flows**. Device flow requires no client secret or redirect URL.
3. Set `MICROSOFT_CLIENT_ID` in the root `.env` file, then rebuild the launcher.
4. Click **Sign in with Microsoft**, open Microsoft, and enter the code.
5. The account must own Minecraft Java and have created a player profile.

Access to Minecraft Services for a new application may require Mojang approval. A rejection at this stage cannot be fixed by changing libraries or borrowing another launcher's Client ID. Validate the complete sign-in flow with your own application and account before distribution.

When sign-in is rejected, HX distinguishes Xbox Live, Xbox XSTS, and Minecraft Services. An HTTP 403 alone does not prove an app registration issue. If Minecraft returns `Invalid app registration`, use the [official AppID Review form](https://aka.ms/mce-reviewappid). It asks for the Client ID, Tenant ID, a contact, an application page, and a justification for API access. The form mentions a weekly review with no guaranteed approval time. Authentication responses and tokens are excluded from diagnostics.

Sources: [Microsoft device flow](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code), [oauth2-rs](https://github.com/ramosbugs/oauth2-rs), [minecraft-msa-auth](https://github.com/minecraft-rs/minecraft-msa-auth).

## Java

Java is not installed automatically yet. Set `java` or the absolute path to your executable in the settings, then click **Verify**. Before downloading the game, the backend compares your Java version with the minimum required by the Minecraft manifest. Use a Java runtime built for your computer's architecture.

Examples: Java 17 for Minecraft 1.20.1; Java 21 for 1.20.5 / 1.21. Later versions follow their manifest.

## Data

The folder selected in settings contains both instances and shared game files:

```text
<selected folder>/
├── instances/<name>/           # Worlds, mods, configs and game logs
└── runtime/
    ├── minecraft/              # Vanilla versions, libraries, assets and Java
    └── modded-runtime/         # Modded versions, libraries, assets and Java
```

On startup, the launcher automatically upgrades the previously selected instances folder to this layout. Without a selected folder, it creates this layout in the application data directory. Migration copies existing instances and shared game files, updates saved icon paths, then removes the old copies after saving the new setting when cleanup succeeds. Non-secret settings remain in the Tauri application data directory as `state.json`.

Persistent tokens stay in the system keychain and never in `state.json`. On Windows, large sessions are split across several Credential Manager entries to stay within its per-entry size limit. The launcher does not log the Java command line, which contains the session token. Game logs remain local.

## Launcher configuration

Users do not enter technical identifiers in the settings. Fill in the root `.env` file (see `.env.example`) before running `npm run tauri dev` or building. After each change, restart `npm run tauri dev`: the Rust build embeds the values when the application starts.

```dotenv
MICROSOFT_CLIENT_ID=your-application-id
CURSEFORGE_API_KEY='your-api-key'
```

Keep the single quotes around the CurseForge key: otherwise `.env` expands its `$` characters and corrupts the key. Rebuild and restart the launcher after changing `.env`.

Process environment variables take priority. The Rust build loads `.env` and embeds both values in the binary; they never pass through React. Changing `.env` requires a rebuild. The file is ignored by Git. A key embedded in a distributed application can still be extracted; for a truly private production key, CurseForge calls should go through a backend service.

## CurseForge import

**Modpacks → Choose a ZIP → Install.** The import follows the exact versions from the manifest, downloads files through the official API, and verifies their SHA-1. Mods go to `mods`, resource packs to `resourcepacks`, and shaders to `shaderpacks`. Configurations are extracted into a temporary directory; the instance appears on the home screen only after the full installation succeeds.

Fabric, Quilt, Forge, and NeoForge are installed through `mc-launcher-core` 0.1.2 and official sources. Forge/NeoForge installers use the configured Java runtime, write to `runtime/modded-runtime/loader-install.log` in the selected folder, and have a 15-minute maximum duration. The shared runtime and game directories are separated. A Minecraft license and Microsoft sign-in are required to play, not to install.

Missing keys, author-blocked downloads, missing hashes, and duplicate files stop the import with an explicit error. No referenced mod is silently skipped, including optional files. On failure, temporary files are cleaned up and no incomplete instance is published. Already-downloaded Minecraft resources remain cached for a retry. Mod resume and installation cancellation are not available yet.

## Structure

- `src/App.tsx`: library, account, import, settings, and activity.
- `src/Player.tsx`: animated 3D character with `skinview3d`, classic/slim models, and WebGL lifecycle handling.
- `public/skins/steve.png`: Steve texture extracted from the official Mojang 1.21.1 client (Minecraft asset owned by Mojang/Microsoft).
- `src-tauri/src/auth.rs`: authentication libraries and keychain integration.
- `src-tauri/src/minecraft.rs`: manifests, downloads, rules, and launching.
- `src-tauri/src/packs.rs`: archive checks, CurseForge API, and instance publication.
- `src-tauri/src/modded.rs`: modloader installation and modpack launching.
- `src-tauri/src/lib.rs`: Tauri commands and persistence.

## Validation and limitations

The frontend build and 11 Rust tests cover safe paths, launch rules/arguments, CurseForge manifests, file routing, and non-overwriting extraction. A separate network test checks official Mojang/Fabric metadata and launch command construction. Real sign-in, full downloads, and part of Minecraft require your account and are not covered by these tests. Windows/Linux builds require their own validation. The macOS development bundle is unsigned.

Not yet supported: automatic Java installation, snapshots and older versions, instance deletion/duplication, URL imports, repair, and installation cancellation.

## Pastel interface and skin

The window uses a custom drag bar and custom minimize/maximize/close controls. Its pink, blue, and sand background uses translucent panels. The native window is opaque to avoid composition glitches; panel transparency remains an internal interface effect.

The home screen uses `skinview3d`: idle animation, mouse rotation, rendering paused when the page is hidden, and reduced-motion support. Steve is available locally without a connection. After sign-in, the active Minecraft profile skin is downloaded by Rust only from `https://textures.minecraft.net`, without sending a token to that server; classic/slim variants are preserved. If loading fails, Steve remains visible with an explicit status. Showing a real account skin requires a connected account.

On macOS, `tauri.macos.conf.json` uses an opaque native window with a hidden overlay title bar so macOS clips the rounded corners. System buttons are hidden in `setup` in favor of the launcher's controls. No outer CSS mask or native transparency is required.
