package io.github.craftandroid

import android.util.Log
import java.io.File

/**
 * A small diagnostics log (`files/diag.log`) for testers without adb: the activity's lifecycle,
 * the first mouse events of each session and the library's milestones (it appends to the same
 * file). It holds no document content, only events and sizes.
 *
 * Each session starts with a `session start` line and ends with `session end` when the app closes
 * normally, so a session that ended otherwise (a crash, a hang the system ended) is found at the
 * next start and its log offered to the user.
 */
object Diag {
    private const val TAG = "craft-android"
    private const val KEEP_LINES = 400
    private const val START = "session start"
    private const val END = "session end"

    private var file: File? = null

    /** The previous session's lines, if it did not end normally. */
    var unclean: String? = null
        private set

    fun init(dir: File) {
        val f = File(dir, "diag.log")
        file = f
        val lines = runCatching { f.readLines() }.getOrDefault(emptyList())
        val lastStart = lines.indexOfLast { it.contains(START) }
        if (lastStart >= 0 && lines.drop(lastStart).none { it.contains(END) }) {
            unclean = lines.drop(lastStart).joinToString("\n")
        }
        // Keep the file short.
        if (lines.size > KEEP_LINES) runCatching { f.writeText(lines.takeLast(KEEP_LINES / 2).joinToString("\n", postfix = "\n")) }
    }

    @Synchronized
    fun log(line: String) {
        Log.i(TAG, line)
        runCatching { file?.appendText("${System.currentTimeMillis()} kt: $line\n") }
    }

    fun sessionStart(details: String) = log("$START $details")

    fun sessionEnd(reason: String) = log("$END ($reason)")

    /** The whole log, for the report. */
    fun text(): String = runCatching { file?.readText() }.getOrNull().orEmpty()
}
