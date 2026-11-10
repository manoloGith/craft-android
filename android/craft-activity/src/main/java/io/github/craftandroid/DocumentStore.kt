package io.github.craftandroid

import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.FileObserver
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.util.Log
import org.json.JSONObject
import java.io.File
import java.security.MessageDigest
import java.util.concurrent.Executors

/**
 * Documents from the Storage Access Framework, as local files the app can open and save.
 *
 * The Crafting Apps work on paths. A document the user picks (or opens with the app) is copied into a
 * folder of its own under the app's files, named after the document, and the app is given that
 * path. The folder is watched: when the app saves the file (it writes `.<name>…tmp` and renames
 * it into place), the new bytes are written back to the document. Save As and output folders work
 * the same way: the path the app gets is a mirror of the document or folder the user chose.
 *
 * Which local path stands for which document is kept in shared preferences, with the persistable
 * permission the picker grants, so saving still reaches the document after a restart (Recent
 * files).
 */
class DocumentStore(private val context: Context) {
    private val resolver: ContentResolver = context.contentResolver
    private val prefs = context.getSharedPreferences("documents", Context.MODE_PRIVATE)
    private val io = Executors.newSingleThreadExecutor()
    private val observers = HashMap<String, FileObserver>()

    /** Each mirror as it was last copied from or to its document (modified time, length): a
     *  write that leaves it so (the copy made when opening) is not written back. */
    private val synced = HashMap<String, Pair<Long, Long>>()

    private val documentsDir = File(context.filesDir, "documents")
    private val foldersDir = File(context.filesDir, "folders")

    /** Watch every mirror recorded by earlier runs. */
    fun start() {
        for ((path, value) in prefs.all) {
            val entry = (value as? String)?.let { runCatching { JSONObject(it) }.getOrNull() } ?: continue
            val dir = if (entry.optBoolean("tree")) File(path) else File(path).parentFile ?: continue
            if (dir.isDirectory) watch(dir)
        }
    }

    /**
     * Finish writing saves back before the process ends: give the file observers a moment to
     * report a save that has just happened, then wait (up to [timeoutMs]) for the writes queued.
     */
    fun flush(timeoutMs: Long) {
        Thread.sleep(OBSERVER_GRACE_MS)
        io.shutdown()
        if (!io.awaitTermination(timeoutMs, java.util.concurrent.TimeUnit.MILLISECONDS)) {
            Log.w(TAG, "saves were still being written back when the app closed")
        }
    }

    /** Copy [uri] to its local mirror and return the mirror's path (on the caller's thread). */
    fun open(uri: Uri, writable: Boolean): String? {
        keepPermission(uri, writable)
        val name = displayName(uri) ?: "document.pdf"
        val dir = File(documentsDir, key(uri)).apply { mkdirs() }
        val file = File(dir, safeName(name))
        val path = file.absolutePath
        try {
            resolver.openInputStream(uri)?.use { input ->
                file.outputStream().use { input.copyTo(it) }
            } ?: return null
        } catch (e: Exception) {
            Log.w(TAG, "could not read $uri", e)
            return null
        }
        markSynced(file)
        record(path, uri, tree = false, writable = writable)
        watch(dir)
        return path
    }

    /** A new document the user created with Save As: its mirror's path (the file is not made yet). */
    fun created(uri: Uri): String {
        keepPermission(uri, writable = true)
        val name = displayName(uri) ?: "document.pdf"
        val dir = File(documentsDir, key(uri)).apply { mkdirs() }
        val file = File(dir, safeName(name))
        record(file.absolutePath, uri, tree = false, writable = true)
        watch(dir)
        return file.absolutePath
    }

    /** A folder the user chose (for exported or processed files): its mirror's path. */
    fun folder(tree: Uri): String {
        keepPermission(tree, writable = true)
        val dir = File(foldersDir, key(tree)).apply { mkdirs() }
        record(dir.absolutePath, tree, tree = true, writable = true)
        watch(dir)
        return dir.absolutePath
    }

    /** The document a local mirror stands for, if any. */
    fun uriFor(path: String): Uri? =
        prefs.getString(path, null)?.let { runCatching { Uri.parse(JSONObject(it).getString("uri")) }.getOrNull() }

    private fun stamp(file: File) = Pair(file.lastModified(), file.length())

    private fun markSynced(file: File) = synchronized(synced) { synced[file.absolutePath] = stamp(file) }

    private fun isSynced(file: File) = synchronized(synced) { synced[file.absolutePath] == stamp(file) }

    private fun record(path: String, uri: Uri, tree: Boolean, writable: Boolean) {
        val entry = JSONObject().put("uri", uri.toString()).put("tree", tree).put("writable", writable)
        prefs.edit().putString(path, entry.toString()).apply()
    }

    private fun keepPermission(uri: Uri, writable: Boolean) {
        val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or (if (writable) Intent.FLAG_GRANT_WRITE_URI_PERMISSION else 0)
        try {
            resolver.takePersistableUriPermission(uri, flags)
        } catch (_: SecurityException) {
            // Not persistable (a one-off grant from another app's VIEW intent): fine for this run.
        }
    }

    private fun watch(dir: File) {
        val key = dir.absolutePath
        synchronized(observers) {
            if (observers.containsKey(key)) return
            val observer = object : FileObserver(dir, CLOSE_WRITE or MOVED_TO) {
                override fun onEvent(event: Int, name: String?) {
                    if (name == null || name.startsWith(".")) return // the app's staging files
                    val file = File(dir, name)
                    io.execute { writeBack(file) }
                }
            }
            observer.startWatching()
            observers[key] = observer
        }
    }

    /** Write a mirror that the app saved back to its document. */
    private fun writeBack(file: File) {
        if (!file.isFile || isSynced(file)) return
        try {
            val target = documentFor(file) ?: return
            resolver.openOutputStream(target, "wt")?.use { out -> file.inputStream().use { it.copyTo(out) } }
                ?: throw IllegalStateException("no output stream")
            markSynced(file)
            Log.i(TAG, "saved ${file.name} to $target")
        } catch (e: Exception) {
            Log.w(TAG, "could not save ${file.name} back", e)
            CraftActivity.toast(context.getString(R.string.craft_save_back_failed, file.name))
        }
    }

    /** The document [file] is written to: its own, or a new child of the folder it is in. */
    private fun documentFor(file: File): Uri? {
        prefs.getString(file.absolutePath, null)?.let { return Uri.parse(JSONObject(it).getString("uri")) }
        val dirEntry = prefs.getString(file.parentFile?.absolutePath ?: return null, null) ?: return null
        val tree = Uri.parse(JSONObject(dirEntry).getString("uri"))
        val parent = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
        val existing = childNamed(tree, file.name)
        val child = existing ?: DocumentsContract.createDocument(resolver, parent, mimeFor(file.name), file.name)
        child?.let { record(file.absolutePath, it, tree = false, writable = true) }
        return child
    }

    private fun childNamed(tree: Uri, name: String): Uri? {
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
        val columns = arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME)
        resolver.query(children, columns, null, null, null)?.use { c ->
            while (c.moveToNext()) {
                if (c.getString(1) == name) return DocumentsContract.buildDocumentUriUsingTree(tree, c.getString(0))
            }
        }
        return null
    }

    private fun displayName(uri: Uri): String? =
        runCatching {
            resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) c.getString(0) else null
            }
        }.getOrNull() ?: uri.lastPathSegment?.substringAfterLast('/')

    companion object {
        private const val TAG = "craft-android"

        /** How long a file observer may take to report a write (inotify is near instant). */
        private const val OBSERVER_GRACE_MS = 300L

        /** A stable folder name for a document: the same document opens at the same path. */
        fun key(uri: Uri): String =
            MessageDigest.getInstance("SHA-256").digest(uri.toString().toByteArray()).take(12).joinToString("") { "%02x".format(it) }

        /** A file name that is safe on the app's file system. */
        fun safeName(name: String): String {
            val cleaned = name.replace(Regex("[/\\\\\u0000]"), "_").trim().trimStart('.')
            return cleaned.ifEmpty { "document.pdf" }.take(200)
        }

        fun mimeFor(name: String): String {
            val ext = name.substringAfterLast('.', "").lowercase()
            return android.webkit.MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext) ?: "application/octet-stream"
        }
    }
}
