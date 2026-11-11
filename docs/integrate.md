# Integrar craft-android en una app

Ejemplo completo: [DexPdfCraft](https://github.com/manoloGith/DexPdfCraft) (`apps/dexpdfcraft`).

## 1. Repositorio

Añade craft-android como submódulo en la raíz del repositorio de la app:

```bash
git submodule add https://github.com/manoloGith/craft-android craft-android
```

En el `Cargo.toml` raíz:

```toml
[workspace]
exclude = ["upstream", "craft-android"]

[patch.crates-io]
# … los [patch.crates-io] de upstream, copiados (Cargo no los hereda) …
winit = { path = "craft-android/winit" }
```

La rev de egui/winit de upstream debe ser la de craft-android (egui `6d518fc`, winit `a517b38`).
Si upstream la mueve, actualiza craft-android primero (los parches de `winit/PATCHES.md`).

## 2. Crate de la app

```toml
[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
craft-android = { path = "../../craft-android/crates/craft-android" }

[target.'cfg(target_os = "android")'.dependencies]
eframe = { version = "0.37", default-features = false, features = ["android-game-activity"] }
# Si la interfaz usa rfd/arboard: los shims, con el nombre de la crate real
rfd = { package = "rfd-android-shim", path = "../../craft-android/crates/rfd-android-shim" }
arboard = { package = "arboard-android-shim", path = "../../craft-android/crates/arboard-android-shim" }
```

La crate de interfaz de upstream también debe depender de los shims en todos los objetivos (Cargo
no admite otra fuente por objetivo para el mismo nombre): es el único cambio del fork.

### `android_main`

```rust
use craft_android::android::{self as platform, AndroidApp};

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    let config = platform::Config { log_tag: "miapp", asset_dirs: vec![("models", "MIAPP_MODELS")] };
    // None: segunda llamada en el mismo proceso; CraftActivity relanza en uno nuevo.
    let Some(started) = platform::start(&app, &config) else { return };
    rfd::set_backend(Box::new(SafPickers));        // ver DexPdfCraft: android/mod.rs
    arboard::set_backend(Box::new(SystemClipboard));
    let options = platform::native_options(&app, eframe::NativeOptions::default());
    let _ = eframe::run_native("MiApp", options, Box::new(|cc| {
        platform::events::set_context(cc.egui_ctx.clone());
        Ok(Box::new(craft_android::Shell::new(MiApp::new(cc))
            .on_focus_lost(|a| a.autosave_now())
            .title(|a| a.window_title.clone())))
    }));
    platform::finish();
}
```

- **`Shell`** debe envolver a la app siempre (también en escritorio, donde no hace nada): sin él no
  llegan ratón, teclado, teclado en pantalla ni insets.
- **Copiar, cortar y pegar:** `Shell` convierte Ctrl+C, X y V de la actividad en `Event::Copy`,
  `Cut` y `Paste` con el texto del portapapeles del sistema, y lleva a ese portapapeles el texto que
  copia egui (`OutputCommand::CopyText`), como hace egui-winit en escritorio. La app no tiene que
  hacer nada; si su interfaz copia HTML o imágenes, que lo haga con `android::clipboard`.
- **Back** se convierte en la petición de cierre de egui: si la app la cancela para preguntar por
  cambios sin guardar, el diálogo aparece como en el escritorio.
- **«Abrir con»**: `platform::events::opened()` devuelve las rutas locales nuevas; conviértelas en
  el evento de apertura de la app en cada frame.
- **Imprimir**: `platform::print::print_pdf(bytes, título, carpeta)` abre el diálogo del sistema.
- **Diagnóstico** (Ctrl+Mayús+D, «Diagnostics» en el menú del icono, informe al arrancar tras un
  fallo): solo en compilaciones depurables (APK de debug). Un release no muestra nada de eso, aunque
  sigue guardando el registro. Para cambiarlo, sobrescribe `diagnosticsEnabled()` en tu `MainActivity`.
- **Pestañas de documentos** (`craft_android::tabs`): para apps que en Android enseñan varios
  documentos en una sola ventana. `TabStrip::new(&tabs).show_panel(ui)` dibuja una tira de
  pestañas con el nombre, un punto si hay cambios sin guardar, una × y un botón +. Solo informa
  de lo pulsado (`TabAction::Activate`, `Close`, `New`), y la app lo hace con sus propios comandos:
  `Close` debe pasar por el Cerrar de la app, que pregunta si guardar. Se llama justo después de
  la cinta o la barra de herramientas, así que el fork de la app necesita un enganche ahí. Ejemplo:
  DexWordCraft (`Services::below_ribbon` en su fork).

## 3. Gradle

`settings.gradle.kts`:

```kotlin
include(":craft-activity")
project(":craft-activity").projectDir = file("<ruta>/craft-android/android/craft-activity")
```

`build.gradle.kts` raíz: `id("com.android.library") version "9.2.1" apply false`.
App: `implementation(project(":craft-activity"))` y `packaging { jniLibs { useLegacyPackaging = true } }`
si la app ejecuta programas nativos.

```kotlin
class MainActivity : CraftActivity() {
    override fun nativePrograms() = mapOf("MIAPP_TOOL" to "libtool.so")   // opcional
    override fun droppableMimeTypes() = listOf("image/png", "image/jpeg")
}
```

## 4. Manifiesto de la app

La biblioteca ya declara `RestartActivity` (proceso `:restart`) y el `FileProvider`
(`${applicationId}.files`). La app declara su actividad:

- `android:configChanges` con todos los cambios (`orientation|screenSize|screenLayout|smallestScreenSize|density|keyboard|keyboardHidden|navigation|uiMode|fontScale|locale|layoutDirection|colorMode|touchscreen|mcc|mnc|fontWeightAdjustment`): ninguno puede recrear la actividad.
- `launchMode="singleTask"`, `windowSoftInputMode="adjustResize"`, `resizeableActivity="true"`.
- `<meta-data android:name="android.app.lib_name" android:value="<lib>" />`.
- `<layout android:defaultWidth="1280dp" android:defaultHeight="800dp" …/>` para DeX.
- Meta-data de Samsung: `com.samsung.android.keepalive.density` y
  `com.samsung.android.multidisplay.keep_process_alive` = `true`.
- Intent filters `VIEW`/`EDIT`/`SEND`/`SEND_MULTIPLE` con los tipos MIME de la app.
- Opcional (debug): un `activity-alias` cuyo nombre acabe en `.Diagnostics` abre el informe.

## 5. Por qué cada pieza

El porqué de cada decisión (síntoma → causa → arreglo) está en el
[PORTING-PROMPT de DexPdfCraft](https://github.com/manoloGith/DexPdfCraft/blob/main/docs/PORTING-PROMPT.md).
