# craft-android

La capa Android compartida para llevar las apps de [storytold](https://github.com/storytold)
(PdfCraft, PhotoCraft, WordCraft, GridCraft…: núcleo Rust + interfaz egui) a Android con su
interfaz de escritorio, pensada para **Samsung DeX** y las ventanas de escritorio de Android, y
usable también en móvil y tableta.

Sale de [DexPdfCraft](https://github.com/manoloGith/DexPdfCraft), donde se probó en DeX y en el
móvil, y reúne todo lo que costó que funcionara: ratón y rueda, teclado físico completo, teclado en
pantalla, cerrar preguntando por los cambios, volver a abrir sin que se cierre sola, documentos
del Storage Access Framework con escritura de vuelta, portapapeles, imprimir, «Abrir con» y
diagnóstico sin adb.

Una app portada queda en:

```kotlin
class MainActivity : CraftActivity()
```

```rust
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    let Some(started) = craft_android::android::start(&app, &Config { log_tag: "miapp", ..Default::default() }) else { return };
    // … backends de rfd/arboard (unas líneas), y:
    eframe::run_native("MiApp", craft_android::android::native_options(&app, options),
        Box::new(|cc| Ok(Box::new(craft_android::Shell::new(MiApp::new(cc)).on_focus_lost(|a| a.autosave())))));
    craft_android::android::finish();
}
```

## Contenido

| Ruta | Qué es |
|---|---|
| `crates/craft-android` | Crate Rust: `Shell<A: eframe::App>` (entrada de la actividad, Back = cerrar, autoguardado al perder el foco, cursor, título, enlaces), `android::start` (entorno `HOME`/XDG/`LANG`, pánicos, diagnóstico, assets, ejes de la rueda), puente JNI, selectores SAF, portapapeles, impresión, «Abrir con» |
| `crates/rfd-android-shim`, `crates/arboard-android-shim` | Sustitutos de `rfd` y `arboard` con un backend instalable (Android) que en el resto de sistemas reexportan las crates reales |
| `winit/` | winit de storytold (rev `a517b38`) con los parches W-DEX1 (ratón), W-DEX2 (Destroy) y W-DEX3 (eventos antes de la ventana): [`winit/PATCHES.md`](winit/PATCHES.md) |
| `android/craft-activity` | Biblioteca Android (Kotlin): `CraftActivity : GameActivity` con documentos SAF (`DocumentStore`), portapapeles, impresión, ratón y teclado, teclado en pantalla, insets, arrastrar y soltar, ciclo de vida (`RestartActivity`) y diagnóstico (`Diag`) |

Cómo integrarla en una app: [docs/integrate.md](docs/integrate.md).

## Compilar y probar

```bash
cargo test --workspace
```

```bash
cargo ndk -t arm64-v8a clippy -p craft-android -- -D warnings
```

```bash
cd android && ./gradlew :craft-activity:assembleRelease
```

Requisitos: Rust (`rust-toolchain.toml`), `cargo-ndk` 4.1.2, NDK 29.0.14206865, JDK 17, SDK 36.

## Licencia

MIT o Apache-2.0, a elegir (`LICENSE-MIT`, `LICENSE-APACHE`). `winit/` conserva su licencia
Apache-2.0 (`winit/LICENSE`).
