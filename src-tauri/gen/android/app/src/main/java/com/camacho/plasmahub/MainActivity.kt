package com.camacho.plasmahub

import android.content.Intent
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import org.json.JSONObject

class MainActivity : TauriActivity() {
  private var webView: WebView? = null
  private var pendingSharedUrl: String? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    handleSharedIntent(intent)
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    this.webView = webView
    flushPendingSharedUrl()
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    setIntent(intent)
    handleSharedIntent(intent)
  }

  private fun handleSharedIntent(intent: Intent?) {
    val sharedText = when (intent?.action) {
      Intent.ACTION_SEND -> intent.getStringExtra(Intent.EXTRA_TEXT)
      Intent.ACTION_SEND_MULTIPLE -> intent.getStringExtra(Intent.EXTRA_TEXT)
      Intent.ACTION_VIEW -> intent.dataString
      else -> null
    }
    val sharedUrl = extractSupportedUrl(sharedText ?: return) ?: return
    pendingSharedUrl = sharedUrl
    flushPendingSharedUrl()
  }

  private fun extractSupportedUrl(text: String): String? {
    val urlRegex = Regex("""https?://[^\s"'<>]+""")
    return urlRegex.findAll(text)
      .map { it.value.trimEnd('.', ',', ')', ']', '}') }
      .firstOrNull { isSupportedUrl(it) }
  }

  private fun isSupportedUrl(url: String): Boolean {
    val normalized = url.lowercase()
    return normalized.contains("youtube.com/") ||
      normalized.contains("youtu.be/") ||
      normalized.contains("instagram.com/") ||
      normalized.contains("tiktok.com/") ||
      normalized.contains("twitter.com/") ||
      normalized.contains("x.com/")
  }

  private fun flushPendingSharedUrl() {
    val url = pendingSharedUrl ?: return
    val target = webView ?: return
    val jsonUrl = JSONObject.quote(url)
    val script = """
      (function () {
        var url = $jsonUrl;
        localStorage.setItem('plasma_pending_shared_download_url', url);
        window.dispatchEvent(new CustomEvent('plasma-android-share-url', { detail: { url: url } }));
      })();
    """.trimIndent()
    target.post {
      target.evaluateJavascript(script, null)
    }
    pendingSharedUrl = null
  }
}
