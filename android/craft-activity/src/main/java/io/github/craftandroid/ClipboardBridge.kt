package io.github.craftandroid

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import androidx.core.content.FileProvider
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.nio.ByteBuffer

/**
 * The system clipboard for the app (craft_android::android::clipboard): text, HTML with its plain
 * alternative, images (RGBA8, shared as a PNG through the app's FileProvider) and file lists.
 */
class ClipboardBridge(private val context: Context) {
    private val label = context.applicationInfo.loadLabel(context.packageManager).toString()
    private val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    private val authority = "${context.packageName}.files"

    /** What is on the clipboard: `{"kind": "none" | "text" | "html" | "image", "text", "html"}`. */
    fun get(): JSONObject {
        val out = JSONObject().put("kind", "none")
        val clip = clipboard.primaryClip ?: return out
        val item = clip.takeIf { it.itemCount > 0 }?.getItemAt(0) ?: return out
        val mime = clip.description.takeIf { it.mimeTypeCount > 0 }?.getMimeType(0) ?: ""
        if (item.uri != null && (mime.startsWith("image/") || context.contentResolver.getType(item.uri)?.startsWith("image/") == true)) {
            return out.put("kind", "image")
        }
        item.htmlText?.let { return out.put("kind", "html").put("html", it).put("text", item.text?.toString() ?: "") }
        val text = item.coerceToText(context)?.toString() ?: return out
        return out.put("kind", "text").put("text", text)
    }

    /** The clipboard's image as width and height (big-endian ints) followed by RGBA8 pixels. */
    fun image(): ByteArray? {
        val uri = clipboard.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.uri ?: return null
        val bitmap = context.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it) } ?: return null
        val w = bitmap.width
        val h = bitmap.height
        if (w <= 0 || h <= 0 || w.toLong() * h > MAX_PIXELS) return null
        val argb = IntArray(w * h)
        bitmap.getPixels(argb, 0, w, 0, 0, w, h)
        val out = ByteBuffer.allocate(8 + argb.size * 4).putInt(w).putInt(h)
        for (p in argb) {
            out.put((p shr 16).toByte()).put((p shr 8).toByte()).put(p.toByte()).put((p ushr 24).toByte())
        }
        return out.array()
    }

    fun setText(text: String) = clipboard.setPrimaryClip(ClipData.newPlainText(label, text))

    fun setHtml(html: String, text: String) = clipboard.setPrimaryClip(ClipData.newHtmlText(label, text, html))

    /** RGBA8 pixels, shared as a PNG file. */
    fun setImage(width: Int, height: Int, rgba: ByteArray) {
        require(width > 0 && height > 0 && rgba.size.toLong() == width.toLong() * height * 4) { "malformed image" }
        val argb = IntArray(width * height) { i ->
            val o = i * 4
            ((rgba[o + 3].toInt() and 0xff) shl 24) or ((rgba[o].toInt() and 0xff) shl 16) or
                ((rgba[o + 1].toInt() and 0xff) shl 8) or (rgba[o + 2].toInt() and 0xff)
        }
        val bitmap = Bitmap.createBitmap(argb, width, height, Bitmap.Config.ARGB_8888)
        val dir = File(context.cacheDir, "clipboard").apply { mkdirs() }
        dir.listFiles()?.forEach { it.delete() } // only the latest copy is kept
        val file = File(dir, "image-${System.currentTimeMillis()}.png")
        file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        val uri = FileProvider.getUriForFile(context, authority, file)
        clipboard.setPrimaryClip(ClipData.newUri(context.contentResolver, "$label image", uri))
    }

    /** Files from the app's storage, shared through its FileProvider. */
    fun setFiles(paths: JSONArray) {
        val uris = (0 until paths.length()).mapNotNull { i ->
            runCatching { FileProvider.getUriForFile(context, authority, File(paths.getString(i))) }.getOrNull()
        }
        if (uris.isEmpty()) return
        val clip = ClipData.newUri(context.contentResolver, "$label files", uris.first())
        uris.drop(1).forEach { clip.addItem(ClipData.Item(it)) }
        clipboard.setPrimaryClip(clip)
    }

    companion object {
        /** The largest image read from the clipboard (64 MiB of RGBA). */
        private const val MAX_PIXELS = 64L * 1024 * 1024 / 4
    }
}
