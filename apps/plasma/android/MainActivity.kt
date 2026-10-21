package dev.dioxus.main

import android.content.Intent
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import java.io.File

typealias BuildConfig = com.camacho.plasmahub.BuildConfig

class MainActivity : WryActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        handleShareIntent(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleShareIntent(intent)
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_OPEN_MEDIA) {
            return
        }
        val path = if (resultCode == RESULT_OK) {
            data?.data?.let { copyUriToCache(it) }
        } else {
            null
        }
        nativeOnMediaPicked(path)
    }

    fun requestOpenMedia() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "*/*"
            putExtra(
                Intent.EXTRA_MIME_TYPES,
                arrayOf("image/*", "video/*", "audio/*", "application/pdf", "application/zip"),
            )
        }
        startActivityForResult(intent, REQUEST_OPEN_MEDIA)
    }

    fun openPath(path: String) {
        val file = File(path)
        if (!file.exists()) {
            return
        }
        val mime = MimeTypeMap.getSingleton()
            .getMimeTypeFromExtension(file.extension.lowercase())
            ?: "application/octet-stream"
        MediaScannerConnection.scanFile(
            applicationContext,
            arrayOf(path),
            arrayOf(mime),
        ) { _, uri ->
            if (uri != null) {
                val view = Intent(Intent.ACTION_VIEW).apply {
                    setDataAndType(uri, mime)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                runCatching { startActivity(view) }
            }
        }
    }

    private fun handleShareIntent(intent: Intent?) {
        if (intent == null) {
            return
        }
        val action = intent.action
        if (action != Intent.ACTION_SEND &&
            action != Intent.ACTION_SEND_MULTIPLE &&
            action != Intent.ACTION_VIEW
        ) {
            return
        }
        val text = intent.getStringExtra(Intent.EXTRA_TEXT)
            ?: intent.clipData?.getItemAt(0)?.text?.toString()
        val url = extractUrl(text) ?: extractUrl(intent.dataString)
        if (url != null) {
            nativeOnShareUrl(url)
        }
    }

    private fun extractUrl(value: String?): String? {
        if (value == null) {
            return null
        }
        val match = Regex("""https?://\S+""").find(value)?.value ?: return null
        return match.trimEnd(',', '.', ')', ']', '}', ';', '!', '?', '"', '\'')
    }

    private fun copyUriToCache(uri: Uri): String? {
        val displayName = contentResolver.query(
            uri,
            arrayOf(OpenableColumns.DISPLAY_NAME),
            null,
            null,
            null,
        )?.use { cursor ->
            if (cursor.moveToFirst()) {
                val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (index >= 0) cursor.getString(index) else null
            } else {
                null
            }
        }
        val rawName = displayName ?: "media-${System.currentTimeMillis()}"
        val safeName = rawName
            .replace(Regex("[^A-Za-z0-9._-]"), "_")
            .take(120)
            .ifEmpty { "media-${System.currentTimeMillis()}" }
        val dot = safeName.lastIndexOf('.')
        val base = if (dot > 0) safeName.substring(0, dot) else safeName
        val extension = if (dot > 0) safeName.substring(dot) else ""

        val directory = File(cacheDir, "picked").apply { mkdirs() }
        val target = File(directory, "$base-${System.currentTimeMillis()}$extension")
        return runCatching {
            contentResolver.openInputStream(uri)?.use { input ->
                target.outputStream().use { output -> input.copyTo(output) }
            } ?: return null
            target.absolutePath
        }.getOrNull()
    }

    companion object {
        private const val REQUEST_OPEN_MEDIA = 0x5048

        init {
            System.loadLibrary("main")
        }
    }

    external fun nativeOnMediaPicked(path: String?)
    external fun nativeOnShareUrl(url: String)
}
