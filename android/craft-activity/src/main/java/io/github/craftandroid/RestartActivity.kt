package io.github.craftandroid

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.os.Process
import java.io.File

/**
 * Starts the app again in a fresh process (the ProcessPhoenix pattern). It runs in its own process
 * (`:restart` in the manifest): it waits until the old one has ended, then starts the app's activity again,
 * which gets a new process, and ends its own.
 *
 * Needed because the library's window and event loop can be created only once per process, so an
 * activity that Android starts in a process the app has already run in can't draw.
 */
class RestartActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val oldPid = intent.getIntExtra(EXTRA_PID, -1)
        Thread {
            if (oldPid > 0) {
                Process.killProcess(oldPid)
                val proc = File("/proc/$oldPid")
                val deadline = System.currentTimeMillis() + 3_000
                while (proc.exists() && System.currentTimeMillis() < deadline) Thread.sleep(20)
            }
            @Suppress("DEPRECATION")
            val target = intent.getParcelableExtra<Intent>(EXTRA_INTENT)?.takeIf { it.component != null }
                ?: packageManager.getLaunchIntentForPackage(packageName)
            if (target != null) {
                target.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
                startActivity(target)
            }
            finish()
            Process.killProcess(Process.myPid())
        }.start()
    }

    companion object {
        const val EXTRA_PID = "pid"
        const val EXTRA_INTENT = "intent"
    }
}
