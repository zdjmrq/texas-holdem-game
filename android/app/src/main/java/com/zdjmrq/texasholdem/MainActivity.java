package com.zdjmrq.texasholdem;

import android.app.Activity;
import android.app.AlertDialog;
import android.os.Bundle;
import android.view.WindowInsets;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import androidx.webkit.WebViewAssetLoader;

public class MainActivity extends Activity {
    private WebView webView;
    private static final String HOST = "appassets.androidplatform.net";

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        webView = new WebView(this);
        webView.setBackgroundColor(0xff0d0d1a);
        setContentView(webView);
        // Keep controls away from display cutouts and system gesture areas.
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            webView.setOnApplyWindowInsetsListener((view, insets) -> {
                android.graphics.Insets safe = insets.getInsets(
                    WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
                view.setPadding(safe.left, safe.top, safe.right, safe.bottom);
                return insets;
            });
        }
        WebSettings settings = webView.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setMediaPlaybackRequiresUserGesture(false);
        settings.setUserAgentString(settings.getUserAgentString() + " TexasHoldemAndroid/1.5.0");
        // The HTTP origin is served exclusively from APK assets; it also permits LAN ws:// rooms.
        WebViewAssetLoader loader = new WebViewAssetLoader.Builder()
            .setHttpAllowed(true)
            .addPathHandler("/assets/", new WebViewAssetLoader.AssetsPathHandler(this)).build();
        webView.setWebViewClient(new WebViewClient() {
            @Override public WebResourceResponse shouldInterceptRequest(WebView view, WebResourceRequest request) {
                if (HOST.equals(request.getUrl().getHost())) {
                    WebResourceResponse response = loader.shouldInterceptRequest(request.getUrl());
                    return response != null ? response : new WebResourceResponse("text/plain", "UTF-8", 404,
                        "Not Found", java.util.Collections.emptyMap(), new java.io.ByteArrayInputStream(new byte[0]));
                }
                return super.shouldInterceptRequest(view, request);
            }
            @Override public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                return !HOST.equals(request.getUrl().getHost())
                    || !"http".equals(request.getUrl().getScheme())
                    || !request.getUrl().getPath().startsWith("/assets/");
            }
        });
        webView.setWebChromeClient(new WebChromeClient());
        webView.loadUrl("http://" + HOST + "/assets/index.html");
        if (android.os.Build.VERSION.SDK_INT >= 33) {
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(0, this::confirmExit);
        }
    }

    private void confirmExit() {
        new AlertDialog.Builder(this).setTitle("退出游戏？")
            .setMessage("当前牌局不会保存。")
            .setNegativeButton("继续游戏", null)
            .setPositiveButton("退出", (dialog, which) -> finish()).show();
    }
    // Android 13+ uses the dispatcher registered in onCreate; this is the API 26-32 fallback.
    @android.annotation.SuppressLint("GestureBackNavigation")
    @Override public void onBackPressed() { confirmExit(); }
    @Override protected void onPause() {
        webView.onPause();
        webView.pauseTimers();
        super.onPause();
    }
    @Override protected void onResume() {
        super.onResume();
        webView.onResume();
        webView.resumeTimers();
    }
    @Override protected void onDestroy() {
        webView.destroy();
        super.onDestroy();
    }
}
