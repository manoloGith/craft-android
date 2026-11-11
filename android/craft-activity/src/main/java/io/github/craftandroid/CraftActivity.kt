package io.github.craftandroid

import android.app.ActivityManager
import android.app.AlertDialog
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.content.pm.ShortcutInfo
import android.content.pm.ShortcutManager
import android.content.res.Configuration
import android.graphics.drawable.Icon
import android.net.Uri
import android.os.Bundle
import android.os.Process
import android.print.PrintAttributes
import android.print.PrintManager
import android.util.Log
import android.view.DragEvent
import android.view.InputDevice
import android.view.PointerIcon
import android.view.KeyCharacterMap
import android.view.KeyEvent
import android.view.MotionEvent
import android.widget.Toast
import androidx.activity.OnBackPressedCallback
import androidx.core.view.WindowInsetsCompat
import com.google.androidgamesdk.GameActivity
import com.google.androidgamesdk.gametextinput.State
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.concurrent.Executors

/**
 * The activity of a Crafting App on Android (craft-android). Subclass it as the app's
 * `MainActivity`; the manifest names the Rust library (`android.app.lib_name`), whose
 * `android_main` calls `craft_android::android::start`.
 *
 * The whole window is drawn by the library (egui through android-activity's GameActivity glue:
 * GameActivity rather than NativeActivity for its IME support, and because eframe's accesskit
 * needs it). This class is the library's way to what only Android offers:
 *
 * - the mouse and hardware keyboards, read here and handed over (winit reports a mouse as a
 *   finger and gets no characters for keys), the on-screen keyboard and the system's insets;
 * - the lifecycle: Back closes the window through the app (which may ask about unsaved changes),
 *   a destroyed activity ends the process and a reused process is restarted (winit's event loop
 *   can be created once per process);
 * - the Storage Access Framework (DocumentStore), the clipboard, printing, "Open with", "Share",
 *   drag and drop, the cursor, the task's title, links;
 * - diagnostics for testers without adb (Diag, a report at the next start, Ctrl+Shift+D), shown
 *   in debuggable builds only unless [diagnosticsEnabled] says otherwise.
 *
 * Rust calls [bridge] over JNI; answers that come later go back through the native methods.
 * Everything crosses as JSON text and byte arrays (`protocol.rs`).
 */
open class CraftActivity : GameActivity() {
    private lateinit var documents: DocumentStore
    private lateinit var clipboard: ClipboardBridge
    private val io = Executors.newSingleThreadExecutor()

    /** Pickers on screen: request code → (Rust's request id, kind). */
    private val pending = HashMap<Int, Pair<Long, Int>>()
    private var nextRequest = 0

    override fun onCreate(savedInstanceState: Bundle?) {
        val reused = processUsed
        processUsed = true
        Diag.init(filesDir)
        Diag.sessionStart(
            "pid=${Process.myPid()} reused=$reused restored=${savedInstanceState != null} " +
                "multiWindow=$isInMultiWindowMode dex=${isDexMode(resources.configuration)} ${describe(resources.configuration)} " +
                "android=${android.os.Build.VERSION.SDK_INT} device=${android.os.Build.MODEL}",
        )
        recordJavaCrashes()
        offerPrograms()
        super.onCreate(savedInstanceState) // loads the library and starts android_main
        if (reused) {
            // This process has already run the library (its window can't be created twice):
            // start again in a fresh one.
            Diag.sessionEnd("process reused; restarting in a fresh one")
            restartInFreshProcess()
            return
        }
        takeMouseInput()
        acceptDrops()
        if (diagnosticsEnabled()) offerDiagnosticsShortcut() else withdrawDiagnosticsShortcut()
        current = this
        documents = DocumentStore(applicationContext).also { it.start() }
        clipboard = ClipboardBridge(applicationContext)
        nativeRegister(this)
        // Back closes the window the way its close button does on a desktop: the app asks about
        // unsaved changes first, and asks Android to finish only if the window may close.
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() = nativeEvent("close", "{}")
        })
        if (savedInstanceState == null) handleIntent(intent)
        if (diagnosticsEnabled()) showReport(onlyIfSomethingWentWrong = !isDiagnosticsLaunch(intent))
    }

    // --- Mouse ---------------------------------------------------------------------------------

    /** Mouse events logged this session (the first few, to see what a device sends). */
    private var mouseLogged = 0
    /** Buttons down, as the library was last told. */
    private var mouseButtons = 0

    /**
     * Mouse events go to the library directly (nativePointer) instead of through GameActivity and
     * winit, which report a mouse as a finger: hover, both buttons and the wheel then work as on a
     * desktop. Touch still goes to GameActivity.
     */
    private fun takeMouseInput() {
        val surface = mSurfaceView ?: return
        surface.setOnTouchListener { _, e -> if (isMouse(e)) onMouse(e) else processMotionEvent(e) }
        surface.setOnGenericMotionListener { _, e -> if (isMouse(e)) onMouse(e) else processMotionEvent(e) }
    }

    private fun isMouse(e: MotionEvent): Boolean =
        e.isFromSource(InputDevice.SOURCE_MOUSE) || e.isFromSource(InputDevice.SOURCE_TOUCHPAD) ||
            (e.pointerCount > 0 && e.getToolType(0) == MotionEvent.TOOL_TYPE_MOUSE)

    private fun onMouse(e: MotionEvent): Boolean {
        val action = e.actionMasked
        val bits = e.buttonState and 0x1f
        // Some pointers leave the button bits empty: then DOWN/UP are the primary button and
        // BUTTON_PRESS/RELEASE name theirs.
        val buttons = when {
            bits != 0 -> bits
            action == MotionEvent.ACTION_DOWN -> 1
            action == MotionEvent.ACTION_MOVE -> mouseButtons and 1
            action == MotionEvent.ACTION_BUTTON_PRESS -> mouseButtons or e.actionButton
            action == MotionEvent.ACTION_BUTTON_RELEASE -> mouseButtons and e.actionButton.inv()
            else -> 0
        } and 0x1f
        val scroll = action == MotionEvent.ACTION_SCROLL
        val h = if (scroll) e.getAxisValue(MotionEvent.AXIS_HSCROLL) else 0f
        val v = if (scroll) e.getAxisValue(MotionEvent.AXIS_VSCROLL) else 0f
        val gone = action == MotionEvent.ACTION_HOVER_EXIT && buttons == 0
        if (mouseLogged < MOUSE_LOG_LIMIT && (action != MotionEvent.ACTION_HOVER_MOVE || mouseLogged < 3)) {
            mouseLogged++
            Diag.log(
                "mouse ${MotionEvent.actionToString(action)} src=0x${Integer.toHexString(e.source)} " +
                    "tool=${e.getToolType(0)} state=${e.buttonState} actionButton=${e.actionButton} " +
                    "-> buttons=$buttons wheel=($h,$v) at (${e.x.toInt()},${e.y.toInt()})",
            )
        }
        mouseButtons = if (gone) 0 else buttons
        nativePointer(gone, e.x, e.y, buttons, h, v, e.metaState)
        return true
    }

    // --- Phone and tablet ---------------------------------------------------------------------

    /** Set while this class empties the keyboard's buffer itself. */
    private var resettingIme = false

    /**
     * The on-screen keyboard's buffer changed (GameTextInput). winit drops these, so the library
     * gets them (see `ime.rs`); once nothing is being composed the buffer is emptied, so the
     * keyboard never reaches back into words already typed.
     */
    override fun stateChanged(newState: State, dismissed: Boolean) {
        if (resettingIme) return
        val text = newState.text ?: ""
        nativeIme(text, newState.composingRegionStart, newState.composingRegionEnd, false)
        val composing = newState.composingRegionStart in 0 until newState.composingRegionEnd
        if (!composing && text.isNotEmpty()) {
            resettingIme = true
            try {
                setTextInputState(State("", 0, 0, -1, -1))
            } finally {
                resettingIme = false
            }
            nativeIme("", -1, -1, true)
        }
    }

    /**
     * What the system covers (status and navigation bars, a display cutout, the keyboard), as the
     * interface's safe area: Android 15 draws apps edge to edge.
     */
    override fun onApplyWindowInsets(v: android.view.View, insets: WindowInsetsCompat): WindowInsetsCompat {
        val result = super.onApplyWindowInsets(v, insets)
        val covered = insets.getInsets(
            WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime(),
        )
        nativeInsets(covered.left, covered.top, covered.right, covered.bottom)
        return result
    }

    // --- Desktop window --------------------------------------------------------------------------

    /** The mouse cursor the app asks for (an Android `PointerIcon` type; see `pointer.rs`). */
    private fun setCursor(type: Int) = runOnUiThread {
        mSurfaceView?.pointerIcon = PointerIcon.getSystemIcon(this, type)
    }

    /** The app's window title: the task's label in DeX's taskbar. */
    private fun setWindowTitle(title: String) = runOnUiThread {
        this.title = title
        @Suppress("DEPRECATION") // TaskDescription.Builder is API 33; this works on every version
        setTaskDescription(ActivityManager.TaskDescription(title))
    }

    /**
     * PDFs dragged onto the window (from My Files in DeX, another app in split screen) open in
     * tabs, as files dropped on the desktop app do.
     */
    private fun acceptDrops() {
        val surface = mSurfaceView ?: return
        surface.setOnDragListener { _, event ->
            when (event.action) {
                DragEvent.ACTION_DRAG_STARTED -> event.clipDescription?.let { d ->
                    droppableMimeTypes().any { d.hasMimeType(it) } || d.hasMimeType("application/octet-stream")
                } ?: false
                DragEvent.ACTION_DROP -> {
                    val permissions = requestDragAndDropPermissions(event)
                    val clip = event.clipData
                    val uris = if (clip == null) emptyList() else (0 until clip.itemCount).mapNotNull { clip.getItemAt(it).uri }
                    Diag.log("drop: ${uris.size} item(s)")
                    io.execute {
                        val paths = uris.mapNotNull { documents.open(it, writable = false) }
                        permissions?.release()
                        if (paths.size < uris.size) toast(getString(R.string.craft_open_failed))
                        if (paths.isNotEmpty()) nativeEvent("open", JSONObject().put("paths", JSONArray(paths)).toString())
                    }
                    true
                }
                else -> true
            }
        }
    }

    /** Android's print dialog for a PDF the app composed (see `print.rs`). */
    private fun print(file: File, title: String) = runOnUiThread {
        val manager = getSystemService(PRINT_SERVICE) as PrintManager
        try {
            manager.print(title, PdfPrintAdapter(file, "$title.pdf"), PrintAttributes.Builder().build())
        } catch (e: Exception) {
            Diag.log("print failed: $e")
            toast(getString(R.string.craft_print_failed))
            file.delete()
        }
    }

    /** A link (in a document, Help ▸ …) in the browser or the app that handles it. */
    private fun openUrl(url: String) = runOnUiThread {
        try {
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        } catch (e: Exception) {
            Diag.log("no app for a link: ${e.javaClass.simpleName}")
            toast(getString(R.string.craft_no_app_for_link))
        }
    }

    /**
     * Programs the app runs (an OCR engine…), shipped as lib/<abi>/<file> in the APK: Android runs
     * programs only from the native-library folder (useLegacyPackaging extracts them there). Each
     * entry is environment variable → file name; the variable is set to the program's path, before
     * the library starts, when the file is there. Override to name yours.
     */
    protected open fun nativePrograms(): Map<String, String> = emptyMap()

    private fun offerPrograms() {
        for ((variable, name) in nativePrograms()) {
            val program = File(applicationInfo.nativeLibraryDir, name)
            if (program.isFile) {
                android.system.Os.setenv(variable, program.absolutePath, true)
                Diag.log("program $name: ${program.length()} bytes, executable=${program.canExecute()}")
            }
        }
    }

    /**
     * MIME types of what may be dropped onto the window (from My Files in DeX, another app in
     * split screen); the documents open in the app. Override with the app's formats.
     */
    protected open fun droppableMimeTypes(): List<String> = listOf("*/*")

    /** The app's name, for reports and labels. */
    protected val appName: String get() = applicationInfo.loadLabel(packageManager).toString()

    /** Samsung DeX (desktop mode), read from One UI's configuration field; false elsewhere. */
    private fun isDexMode(c: Configuration): Boolean = runCatching {
        val cls = c.javaClass
        val enabled = cls.getField("SEM_DESKTOP_MODE_ENABLED").getInt(cls)
        cls.getField("semDesktopModeEnabled").getInt(c) == enabled
    }.getOrDefault(false)

    // --- Diagnostics ---------------------------------------------------------------------------

    private fun describe(c: Configuration) =
        "screen=${c.screenWidthDp}x${c.screenHeightDp}dp density=${c.densityDpi} " +
            "uiMode=0x${Integer.toHexString(c.uiMode)} display=${display?.displayId}"

    override fun onStart() { super.onStart(); Diag.log("onStart") }
    override fun onResume() { super.onResume(); Diag.log("onResume") }
    override fun onPause() { Diag.log("onPause"); super.onPause() }
    override fun onStop() { Diag.log("onStop"); super.onStop() }

    override fun onConfigurationChanged(newConfig: Configuration) {
        Diag.log("onConfigurationChanged dex=${isDexMode(newConfig)} ${describe(newConfig)}")
        super.onConfigurationChanged(newConfig)
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        Diag.log("onWindowFocusChanged $hasFocus")
        super.onWindowFocusChanged(hasFocus)
    }

    private fun isDiagnosticsLaunch(i: Intent?) =
        i?.action == ACTION_DIAGNOSTICS || i?.component?.className?.endsWith(".Diagnostics") == true

    /**
     * Whether testers get the diagnostics: the "Diagnostics" item in the icon's menu, Ctrl+Shift+D
     * and the report at start after a crash. By default only in debuggable builds (debug APKs):
     * a release shows none of it. The log (`Diag`) and crash records are kept either way. Override
     * to choose otherwise.
     */
    protected open fun diagnosticsEnabled(): Boolean =
        (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0

    /** A release installed over a build that offered the shortcut: take it away. */
    private fun withdrawDiagnosticsShortcut() {
        runCatching { getSystemService(ShortcutManager::class.java)?.removeAllDynamicShortcuts() }
            .onFailure { Diag.log("diagnostics shortcut: $it") }
    }

    /** "Diagnostics" in the menu of the app's icon (long press, or right click in DeX). */
    private fun offerDiagnosticsShortcut() {
        runCatching {
            val shortcut = ShortcutInfo.Builder(this, "diagnostics")
                .setShortLabel(getString(R.string.craft_diagnostics_short))
                .setLongLabel(getString(R.string.craft_diagnostics_label, appName))
                .setIcon(Icon.createWithResource(this, applicationInfo.icon))
                .setIntent(Intent(this, javaClass).setAction(ACTION_DIAGNOSTICS))
                .build()
            getSystemService(ShortcutManager::class.java)?.dynamicShortcuts = listOf(shortcut)
        }.onFailure { Diag.log("diagnostics shortcut: $it") }
    }

    /** Ctrl+Shift+D shows the diagnostics report (when enabled); physical keyboards go to the library. */
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (diagnosticsEnabled() && event.action == KeyEvent.ACTION_DOWN && event.keyCode == KeyEvent.KEYCODE_D &&
            event.isCtrlPressed && event.isShiftPressed
        ) {
            showReport(onlyIfSomethingWentWrong = false)
            return true
        }
        if (isHardwareKey(event) && onKey(event)) return true
        return super.dispatchKeyEvent(event)
    }

    // --- Keyboard ------------------------------------------------------------------------------

    /** A dead key (´ ` ^ ¨ ~) waiting for the next key, as its combining accent. */
    private var pendingAccent = 0
    private var keysLogged = 0

    /** A key of a physical keyboard (not the on-screen one, not Back, volume or power). */
    private fun isHardwareKey(e: KeyEvent): Boolean {
        val device = e.device ?: return false
        if (device.isVirtual || !e.isFromSource(InputDevice.SOURCE_KEYBOARD)) return false
        return when (e.keyCode) {
            KeyEvent.KEYCODE_BACK, KeyEvent.KEYCODE_VOLUME_UP, KeyEvent.KEYCODE_VOLUME_DOWN,
            KeyEvent.KEYCODE_VOLUME_MUTE, KeyEvent.KEYCODE_POWER, KeyEvent.KEYCODE_HOME,
            KeyEvent.KEYCODE_APP_SWITCH, KeyEvent.KEYCODE_MENU -> false
            else -> true
        }
    }

    /**
     * Hand a hardware key to the library: winit gets no character for letters, digits and symbols
     * on Android, so typing and shortcuts like Ctrl+O did nothing. Android's KeyEvent knows the
     * layout: the character with Shift and AltGr, and dead keys (´ then a gives á).
     */
    @Suppress("DEPRECATION") // ACTION_MULTIPLE: still sent by some input devices
    private fun onKey(e: KeyEvent): Boolean {
        val pressed = when (e.action) {
            KeyEvent.ACTION_DOWN -> true
            KeyEvent.ACTION_UP -> false
            KeyEvent.ACTION_MULTIPLE -> {
                // Several characters at once (some input devices): type them.
                e.characters?.let { nativeKey(KeyEvent.KEYCODE_UNKNOWN, 0, it, true, false, e.metaState) }
                return true
            }
            else -> return false
        }
        var text = ""
        if (pressed) {
            val c = e.getUnicodeChar(e.metaState)
            when {
                c and KeyCharacterMap.COMBINING_ACCENT != 0 -> pendingAccent = c and KeyCharacterMap.COMBINING_ACCENT_MASK
                c != 0 && pendingAccent != 0 -> {
                    val combined = KeyCharacterMap.getDeadChar(pendingAccent, c)
                    text = if (combined != 0) String(Character.toChars(combined)) else String(Character.toChars(pendingAccent)) + String(Character.toChars(c))
                    pendingAccent = 0
                }
                c != 0 -> text = String(Character.toChars(c))
            }
        }
        val base = e.getUnicodeChar(0).let { if (it and KeyCharacterMap.COMBINING_ACCENT != 0) 0 else it }
        if (keysLogged < KEY_LOG_LIMIT && pressed) {
            keysLogged++
            Diag.log("key code=${e.keyCode} base=$base meta=0x${Integer.toHexString(e.metaState)} typed=${if (text.isEmpty()) "-" else "yes"}")
        }
        nativeKey(e.keyCode, base, text, pressed, e.repeatCount > 0, e.metaState)
        return true
    }

    private fun restartInFreshProcess() {
        startActivity(
            Intent(this, RestartActivity::class.java)
                .putExtra(RestartActivity.EXTRA_PID, Process.myPid())
                .putExtra(RestartActivity.EXTRA_INTENT, Intent(intent)),
        )
        finish()
    }

    /** A Java crash is written where a Rust panic is (the library's `crash.txt`), then reported. */
    private fun recordJavaCrashes() {
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        val file = File(filesDir, CRASH_FILE)
        Thread.setDefaultUncaughtExceptionHandler { thread, e ->
            runCatching { file.writeText("$appName (Java)\n${Log.getStackTraceString(e)}") }
            previous?.uncaughtException(thread, e)
        }
    }

    /**
     * The report for the user to pass on: what went wrong last time (a panic, an exception, or a
     * session that ended without closing normally) and the diagnostics log. Shown at start when
     * something went wrong, and always from the debug build's "Diagnostics" icon.
     */
    private fun showReport(onlyIfSomethingWentWrong: Boolean) {
        val crashFile = File(filesDir, CRASH_FILE)
        val crash = if (crashFile.isFile) runCatching { crashFile.readText() }.getOrDefault("") else ""
        crashFile.delete()
        val unclean = Diag.unclean
        if (onlyIfSomethingWentWrong && crash.isBlank() && unclean == null) return
        val report = buildString {
            if (crash.isNotBlank()) append(crash).append("\n\n")
            if (unclean != null) append("-- previous session (did not end normally) --\n").append(unclean).append("\n\n")
            append("-- log --\n").append(Diag.text().takeLast(12_000))
        }.take(30_000)
        AlertDialog.Builder(this)
            .setTitle(if (crash.isBlank() && unclean == null) R.string.craft_report_title else R.string.craft_crash_title)
            .setMessage(report)
            .setPositiveButton(R.string.craft_crash_copy) { _, _ ->
                (getSystemService(CLIPBOARD_SERVICE) as ClipboardManager)
                    .setPrimaryClip(ClipData.newPlainText("$appName report", report))
            }
            .setNegativeButton(R.string.craft_crash_close, null)
            .show()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        Diag.log("onNewIntent ${intent.action}")
        setIntent(intent)
        if (isDiagnosticsLaunch(intent)) {
            if (diagnosticsEnabled()) showReport(onlyIfSomethingWentWrong = false)
        } else {
            handleIntent(intent)
        }
    }

    override fun onDestroy() {
        Diag.log("onDestroy finishing=$isFinishing changingConfigurations=$isChangingConfigurations")
        if (current === this) current = null
        super.onDestroy()
        Diag.log("onDestroy: the library has stopped")
        if (!::documents.isInitialized) {
            Diag.sessionEnd("activity destroyed before it started")
            Process.killProcess(Process.myPid())
            return
        }
        // The library's window and event loop live for the whole process and can't be created a
        // second time in it, so a destroyed activity ends the process: the next start is a fresh
        // one. Unsaved work is in the app's crash recovery (Shell autosaves when the window loses
        // focus) and is offered back then.
        documents.flush(timeoutMs = 4_000)
        Diag.sessionEnd("activity destroyed; ending the process")
        Process.killProcess(Process.myPid())
    }

    /**
     * "Open with" and "Share" from other apps: open the documents in new tabs. Any other action
     * that carries documents (an app's own, such as a file manager's `…action.OPEN` declared in
     * the manifest) opens them too: its data URI and the URIs of its clip data.
     */
    private fun handleIntent(intent: Intent?) {
        val uris = when (intent?.action) {
            null, Intent.ACTION_MAIN, ACTION_DIAGNOSTICS -> emptyList()
            Intent.ACTION_VIEW, Intent.ACTION_EDIT -> listOfNotNull(intent.data)
            Intent.ACTION_SEND -> listOfNotNull(intent.streamExtra())
            Intent.ACTION_SEND_MULTIPLE -> intent.streamExtras()
            else -> intent.documentUris()
        }
        if (uris.isEmpty()) return
        val writable = intent!!.flags and Intent.FLAG_GRANT_WRITE_URI_PERMISSION != 0
        io.execute {
            val paths = uris.mapNotNull { documents.open(it, writable) }
            if (paths.size < uris.size) toast(getString(R.string.craft_open_failed))
            if (paths.isNotEmpty()) nativeEvent("open", JSONObject().put("paths", JSONArray(paths)).toString())
        }
    }

    /**
     * Called by Rust (any thread) with an operation, its JSON arguments and optional bytes; returns
     * bytes or null. Never throws: a failure is logged and answered with null.
     */
    @Suppress("unused") // called over JNI
    fun bridge(op: String, json: String, data: ByteArray?): ByteArray? = try {
        val args = if (json.isEmpty()) JSONObject() else JSONObject(json)
        when (op) {
            "pick" -> { pick(args); null }
            "clip_get" -> clipboard.get().toString().toByteArray()
            "clip_image" -> clipboard.image()
            "clip_text" -> { clipboard.setText(args.getString("text")); null }
            "clip_html" -> { clipboard.setHtml(args.getString("html"), args.optString("text")); null }
            "clip_set_image" -> { clipboard.setImage(args.getInt("width"), args.getInt("height"), data ?: ByteArray(0)); null }
            "clip_files" -> { clipboard.setFiles(args.getJSONArray("paths")); null }
            "toast" -> { toast(args.getString("text")); null }
            "cursor" -> { setCursor(args.getInt("type")); null }
            "print" -> { print(File(args.getString("path")), args.getString("title")); null }
            "open_url" -> { openUrl(args.getString("url")); null }
            "title" -> { setWindowTitle(args.getString("title")); null }
            "finish" -> { runOnUiThread { finishAndRemoveTask() }; null }
            "restart" -> { runOnUiThread { restartInFreshProcess() }; null }
            else -> { Log.w(TAG, "bridge: unknown operation $op"); null }
        }
    } catch (e: Exception) {
        Log.w(TAG, "bridge: $op failed", e)
        null
    }

    /** Show the system picker for a request from the rfd shim (see `Kind` in its Rust code). */
    private fun pick(args: JSONObject) {
        val id = args.getLong("id")
        val kind = args.getInt("kind")
        val intent = when (kind) {
            KIND_FOLDER, KIND_FOLDERS -> Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
            KIND_SAVE -> Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                val name = args.optString("file_name").ifEmpty { "document.pdf" }
                type = mimeTypes(args).firstOrNull() ?: DocumentStore.mimeFor(name)
                putExtra(Intent.EXTRA_TITLE, name)
            }
            else -> Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                val mimes = mimeTypes(args)
                type = mimes.singleOrNull() ?: "*/*"
                if (mimes.size > 1) putExtra(Intent.EXTRA_MIME_TYPES, mimes.toTypedArray())
                putExtra(Intent.EXTRA_ALLOW_MULTIPLE, kind == KIND_OPEN_FILES)
            }
        }
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
        runOnUiThread {
            val code = REQUEST_BASE + (nextRequest++ and 0xfff)
            pending[code] = Pair(id, kind)
            try {
                @Suppress("DEPRECATION")
                startActivityForResult(intent, code)
            } catch (e: Exception) {
                Log.w(TAG, "no picker for $intent", e)
                pending.remove(code)
                answer(id, emptyList())
            }
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        val (id, kind) = pending.remove(requestCode) ?: return
        val uris = if (resultCode == RESULT_OK && data != null) {
            val clip = data.clipData
            if (clip != null) (0 until clip.itemCount).map { clip.getItemAt(it).uri } else listOfNotNull(data.data)
        } else {
            emptyList()
        }
        io.execute {
            val paths = when (kind) {
                KIND_FOLDER, KIND_FOLDERS -> uris.map { documents.folder(it) }
                KIND_SAVE -> uris.map { documents.created(it) }
                else -> uris.mapNotNull { documents.open(it, writable = true) }
            }
            if (paths.size < uris.size) toast(getString(R.string.craft_open_failed))
            answer(id, paths)
        }
    }

    private fun answer(id: Long, paths: List<String>) =
        nativeEvent("picked", JSONObject().put("id", id).put("paths", JSONArray(paths)).toString())

    /** The MIME types of the dialog's filters (`[[name, [ext, …]], …]`). */
    private fun mimeTypes(args: JSONObject): List<String> {
        val filters = args.optJSONArray("filters") ?: return emptyList()
        val out = LinkedHashSet<String>()
        for (i in 0 until filters.length()) {
            val exts = filters.optJSONArray(i)?.optJSONArray(1) ?: continue
            for (j in 0 until exts.length()) {
                val ext = exts.getString(j)
                if (ext == "*") return emptyList()
                out.add(DocumentStore.mimeFor("x.$ext"))
            }
        }
        return out.filter { it != "application/octet-stream" }
    }

    /** This activity, for the library (stored as a global reference on the Rust side). */
    private external fun nativeRegister(activity: CraftActivity)

    /** An event for the library: `picked` `{id, paths}`, `open` `{paths}` or `close` `{}`. */
    private external fun nativeEvent(op: String, json: String)

    /** The on-screen keyboard's buffer (see `ime.rs`); `reset`: this class emptied it. */
    private external fun nativeIme(text: String, start: Int, end: Int, reset: Boolean)

    /** What the system covers, in pixels (see `insets.rs`). */
    private external fun nativeInsets(left: Int, top: Int, right: Int, bottom: Int)

    /** One key of a hardware keyboard (see `keyboard.rs`). */
    private external fun nativeKey(keycode: Int, base: Int, text: String, pressed: Boolean, repeat: Boolean, meta: Int)

    /** One mouse event, in the surface's pixels (see `pointer.rs`). */
    private external fun nativePointer(gone: Boolean, x: Float, y: Float, buttons: Int, hscroll: Float, vscroll: Float, meta: Int)

    companion object {
        private const val TAG = "craft-android"
        private const val REQUEST_BASE = 0x4000
        private const val CRASH_FILE = "crash.txt"
        private const val MOUSE_LOG_LIMIT = 60
        /** Keys logged per session: codes only, never what is typed. */
        private const val KEY_LOG_LIMIT = 20
        const val ACTION_DIAGNOSTICS = "io.github.craftandroid.DIAGNOSTICS"

        /** This process has created a CraftActivity (and so started the library) before. */
        @Volatile private var processUsed = false

        // The rfd shim's `Kind`, in its order.
        const val KIND_OPEN_FILE = 0
        const val KIND_OPEN_FILES = 1
        const val KIND_FOLDER = 2
        const val KIND_FOLDERS = 3
        const val KIND_SAVE = 4

        @Volatile private var current: CraftActivity? = null

        fun toast(text: String) {
            val activity = current ?: return
            activity.runOnUiThread { Toast.makeText(activity, text, Toast.LENGTH_LONG).show() }
        }
    }
}

@Suppress("DEPRECATION")
private fun Intent.streamExtra(): Uri? = getParcelableExtra(Intent.EXTRA_STREAM)

@Suppress("DEPRECATION")
private fun Intent.streamExtras(): List<Uri> = getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM).orEmpty()

/** The documents an intent of another action carries: its data, its clip data, its streams. */
private fun Intent.documentUris(): List<Uri> {
    val uris = mutableListOf<Uri>()
    data?.let(uris::add)
    clipData?.let { clip -> for (i in 0 until clip.itemCount) clip.getItemAt(i).uri?.let(uris::add) }
    streamExtra()?.let(uris::add)
    uris += streamExtras()
    return uris.distinct()
}
