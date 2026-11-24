# ImmersiveMap Core

[![License](https://img.shields.io/github/license/artembobkin/ImmersiveMapCore?style=flat-square)](LICENSE)

ImmersiveMap Core is the Rust code the ports of [ImmersiveMap](https://github.com/artembobkin/ImmersiveMap), the Swift and Metal map engine for Apple platforms, have in common. It holds the parts of the engine that know nothing of a host: no browser, no GPU API, no IO. A host fetches the bytes and draws the result, and everything between is here.

It is used by [ImmersiveMap Web](https://github.com/artembobkin/ImmersiveMapWeb), where it is compiled to WebAssembly, and is the base of the Android engine.

## Crates

| Crate | What it does |
| --- | --- |
| `pmtiles` | The PMTiles v3 format: the header, the directories, the Hilbert tile id, the walk of an archive's directories, a writer. |
| `mvt` | A zero-copy Mapbox Vector Tile decoder. |
| `map-style` | The tile schema, the map style with its themes, and the adaptation of labels (language, romanization, glyph coverage). |
| `tile-parse` | The whole parse of a map tile into the arena the GPU takes: ground, buildings, trees, lines, roads, labels. |
| `tile-flattening` | The ground flattener: stacked painted rings in, one layer of triangles that do not overlap out. |
| `tile-prep` | A small MVT to vertex and index buffer preparer. |
| `text-layout` | The layout of label text over the bundled font atlases' metrics. |
| `ground-relief` | The decode of terrain tiles (Terrarium and Mapbox, WebP or PNG) to half-float heights. |
| `layer-mesh` | The layers of an app's own data: GeoJSON to flat shapes, shapes to a mesh, the mesh file. |
| `model-tile` | The model tile's format, read and written. |
| `map-frame` | The per-frame logic: the tile placement planner, the label collision solve, the pin collision layout, the tile grid's frame. |
| `map-worker` | What a host's worker runs: the tile parse as the settings configure it, the decode of a model tile. |

## Building and testing

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --target wasm32-unknown-unknown
```

Some tests read real tiles from a `fixtures` directory at the root of the workspace and skip themselves without it. ImmersiveMap Web downloads them (`npm run fixtures` there), and a symbolic link to its `fixtures` directory is enough. `FIXTURES` names another directory.

### On an Android device

```sh
rustup target add aarch64-linux-android
scripts/test-android.sh
```

The script builds the tests for `aarch64-linux-android` with the NDK's clang (`ANDROID_NDK_HOME`, else the newest NDK in `$ANDROID_HOME/ndk`), pushes them and the fixtures to the device `adb` sees, and runs each one there with `FIXTURES` naming the pushed fixtures. It fails if a test fails, or if a fixture test skips although the fixtures were pushed.

## Using it from a host

The crates are not on crates.io. A host names them as git dependencies:

```toml
[dependencies]
tile-parse = { git = "https://github.com/artembobkin/ImmersiveMapCore" }
```

To work on the core and a host together, point the host at a local checkout with a `[patch]` section in its `Cargo.toml`:

```toml
[patch."https://github.com/artembobkin/ImmersiveMapCore"]
tile-parse = { path = "../ImmersiveMapCore/crates/tile-parse" }
```

## License

MIT, see [LICENSE](LICENSE).
