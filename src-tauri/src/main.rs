// DeepSeek Harness Launcher — Tauri 2 desktop shell.
//
// Behaviour (mirrors the upstream desktop app):
//  * On startup spawns `dsh web` (bundled offline CLI if present, else `npx`)
//    on a system-assigned loopback port (`--port 0`).
//  * Reads the canonical `dsh web: <url>` readiness line from stdout and
//    navigates the main webview to it once ready (after a loading screen).
//  * Lives in the system tray: closing the window only hides it; the app
//    quits only via the tray "Quit" menu.
//  * On exit it actively kills the `dsh web` process tree (taskkill /T /F).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dsh;

use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager, WindowEvent,
};

/// 品牌鲸鱼图标（与 icons/icon.png 一致），编译期嵌入，通过 get_whale_icon_url
/// 命令以 data:image/png;base64,... 形式提供给标题栏 JS。
const WHALE_ICON_PNG: &[u8] = include_bytes!("../icons/icon.png");

/// Injected into every loaded page: a custom title bar that matches the DeepSeek
/// sidebar (`#f9fafb` in light, `#1b1b1c` in dark mode), plus a 1px same-color inset
/// border so the side frame and the bar share one color. The bar is draggable; the
/// right-side window buttons are Windows-11-style 46x40px SVG caption buttons
/// (crisp at any DPI), muted-foreground icon color, bg-muted hover,
/// red (#E81123) close hover, a centered title, and a maximize icon (□↔❐) that
/// follows the window's maximized state (queried via a resize listener). The
/// buttons talk to the `window_action` command over `__TAURI_INTERNALS__.invoke`
/// (the always-injected IPC bridge), so they do NOT depend on `withGlobalTauri`.
/// `close` triggers a CloseRequested, which our handler intercepts to hide the
/// window (the tray owns the app lifetime). Theme is followed live via matchMedia
/// + MutationObserver.
const TITLEBAR_INJECT_JS: &str = r#"
(function () {
  // Theme tokens, recomputed on every applyTheme() so hover/colors stay correct
  // after a dark/light switch. Colors mirror wait-home's index.css tokens:
  // muted-foreground / foreground / muted (hover bg).
  var THEME = { bg:'#f9fafb', border:'#e5e7eb', muted:'#737373', fg:'#1a1a1a',
                hover:'#f4f4f5', closeHover:'#E81123', closeFg:'#ffffff' };

  // Windows-11-style caption icons as inline SVG (stroke=currentColor so hover
  // colors keep working). 12px vector glyphs on 46x40px buttons look much
  // crisper than the old 13px text glyphs (− □ ×), especially on HiDPI.
  var ICON_MIN = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M1.5 6h9" stroke="currentColor" stroke-width="1"/></svg>';
  var ICON_MAX = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="2" y="2" width="8" height="8" rx="0.75" fill="none" stroke="currentColor" stroke-width="1"/></svg>';
  var ICON_RESTORE = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M3 7.5V3a1 1 0 0 1 1-1h3.5a1 1 0 0 1 1 1v1.5" fill="none" stroke="currentColor" stroke-width="1"/><rect x="4.5" y="4.5" width="6" height="6" rx="0.75" fill="none" stroke="currentColor" stroke-width="1"/></svg>';
  var ICON_CLOSE = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 2.5l7 7M9.5 2.5l-7 7" stroke="currentColor" stroke-width="1" stroke-linecap="round"/></svg>';

  // Windows-11-style caption icons as inline SVG (stroke=currentColor so hover

  function parseRgb(s) {
    var m = s && s.match(/rgba?\(([^)]+)\)/);
    if (!m) return null;
    var p = m[1].split(',').map(function (x) { return parseFloat(x); });
    return [p[0] || 0, p[1] || 0, p[2] || 0];
  }
  function luminance(c) {
    function f(x) { x /= 255; return x <= 0.03928 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4); }
    return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]);
  }
  function pageIsDark() {
    var de = document.documentElement;
    if (de && (de.classList.contains('dark') || (de.getAttribute('data-theme') || '').toLowerCase() === 'dark')) return true;
    var bg = getComputedStyle(document.body).backgroundColor;
    var rgb = parseRgb(bg);
    if (rgb) return luminance(rgb) < 0.5;
    try { return !!(window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches); }
    catch (e) { return false; }
  }
  function refreshTheme() {
    var dark = pageIsDark();
    THEME.bg = dark ? '#1b1b1c' : '#f9fafb';
    THEME.border = dark ? 'rgba(255,255,255,0.08)' : '#e5e7eb';
    THEME.muted = dark ? '#a1a1aa' : '#737373';
    THEME.fg = dark ? '#f4f4f5' : '#1a1a1a';
    THEME.hover = dark ? '#2b2c33' : '#f4f4f5';
  }

  function applyTheme() {
    refreshTheme();
    var bar = document.getElementById('__dsh_titlebar');
    if (!bar) return;
    bar.style.background = THEME.bg;
    bar.style.borderBottom = '1px solid ' + THEME.border;
    var btns = bar.querySelectorAll('[data-winbtn]');
    for (var i = 0; i < btns.length; i++) {
      if (btns[i].getAttribute('data-close') !== '1') btns[i].style.color = THEME.muted;
    }
    var t = bar.querySelector('[data-title]');
    if (t) t.style.color = THEME.muted;
    // 品牌区域：鲸鱼图标（<img>）用 filter 反色适配暗色；文字随 fg 变色
    var brand = document.getElementById('__dsh_title_center');
    if (brand) {
      brand.style.color = THEME.fg;
      var wi = document.getElementById('__dsh_whale_img');
      if (wi) wi.style.filter = pageIsDark() ? 'invert(1) hue-rotate(180deg)' : 'none';
    }
      var menu = document.getElementById('__dsh_menu');
      if (menu) {
        menu.style.background = THEME.bg;
        menu.style.border = '1px solid ' + THEME.border;
        menu.style.color = THEME.fg;
      }
      var updateItem = document.getElementById('__dsh_update_item');
      if (updateItem) updateItem.style.color = THEME.fg;
    var st = document.getElementById('__dsh_titlebar_style');
    if (st) st.textContent = 'html{height:calc(100vh - 40px) !important;margin-top:40px !important;box-sizing:border-box;'
      + 'background:' + THEME.bg + ' !important;box-shadow:inset 0 0 0 1px ' + THEME.bg + ' !important;}'
      + 'body{height:100% !important;margin:0 !important;}';
  }

  // Window controls go through a custom Rust command over the always-injected
  // IPC bridge (`__TAURI_INTERNALS__.invoke`), which is present regardless of
  // `withGlobalTauri`. Fall back to the `window.__TAURI__` global if present.
  function invoke(cmd, args) {
    return new Promise(function (resolve, reject) {
      try {
        var t = window.__TAURI_INTERNALS__;
        if (t && t.invoke) { t.invoke(cmd, args || {}).then(resolve, reject); return; }
        var g = window.__TAURI__;
        if (g && g.core && g.core.invoke) { g.core.invoke(cmd, args || {}).then(resolve, reject); return; }
        reject(new Error('Tauri IPC unavailable'));
      } catch (e) { reject(e); }
    });
  }
  function setMaxIcon(m) {
    var b = document.querySelector('[data-kind="max"]');
    if (b) b.innerHTML = m ? ICON_RESTORE : ICON_MAX;
  }
  // Toggle the maximize glyph □ ↔ ❐ (mirrors wait-home's Square/Copy switch).
  function syncMaxIcon() {
    invoke('window_action', { action: 'query_maximized' })
      .then(function (m) { setMaxIcon(m === true); })
      .catch(function () {});
  }

  function mkBtn(iconHtml, tip, onClick, isClose, kind) {
    var b = document.createElement('div');
    b.innerHTML = iconHtml;
    b.title = tip;
    b.setAttribute('data-winbtn', '1');
    if (kind) b.setAttribute('data-kind', kind);
    if (isClose) b.setAttribute('data-close', '1');
    b.style.cssText = 'width:46px;height:40px;display:flex;align-items:center;justify-content:center;'
      + 'cursor:pointer;color:' + THEME.muted + ';'
      + 'transition:background-color 150ms ease,color 150ms ease;'
      + '-webkit-app-region:no-drag;';
    b.onmouseenter = function () {
      if (isClose) { b.style.background = THEME.closeHover; b.style.color = THEME.closeFg; }
      else { b.style.background = THEME.hover; b.style.color = THEME.fg; }
    };
    b.onmouseleave = function () { b.style.background = 'transparent'; b.style.color = THEME.muted; };
    b.onclick = onClick;
    return b;
  }

    function toggleMenu() {
      var menu = document.getElementById('__dsh_menu');
      if (!menu) return;
      var show = menu.style.display === 'none' || !menu.style.display;
      var brand = document.getElementById('__dsh_title_center');
      // 下拉菜单跟随品牌文字位置（点击 DeepSeek Harness 展开）
      if (brand) menu.style.left = brand.offsetLeft + 'px';
      menu.style.display = show ? 'block' : 'none';
      if (brand) brand.setAttribute('aria-expanded', show ? 'true' : 'false');
    }

    function closeMenu() {
      var menu = document.getElementById('__dsh_menu');
      if (menu) menu.style.display = 'none';
      var brand = document.getElementById('__dsh_title_center');
      if (brand) brand.setAttribute('aria-expanded', 'false');
    }

    // 居中提示：内联实现 shadcn Sonner toast 风格（不引入外部依赖、不依赖网络），顶部居中 top:52px。
    function closeCenterAlert() {
      var el = document.getElementById('__dsh_center_alert');
      if (!el) return;
      var card = el.firstElementChild;
      if (card && card.style.animation) {
        card.style.animation = 'dshToastOut 0.2s ease forwards';
        setTimeout(function () { if (el && el.parentNode) el.remove(); }, 220);
      } else {
        el.remove();
      }
    }
    // 内联实现（Sonner toast 观感，顶部居中，不依赖外部组件/网络）
    function fallbackAlert(title, desc, variant) {
      var ts = document.getElementById('__dsh_toast_style');
      if (!ts) {
        ts = document.createElement('style');
        ts.id = '__dsh_toast_style';
        ts.textContent = '@keyframes dshToastIn{from{opacity:0;transform:translateY(-12px) scale(.98)}'
          + 'to{opacity:1;transform:translateY(0) scale(1)}}'
          + '@keyframes dshToastOut{from{opacity:1;transform:translateY(0) scale(1)}'
          + 'to{opacity:0;transform:translateY(-12px) scale(.98)}}';
        document.head.appendChild(ts);
      }
      closeCenterAlert();
      var overlay = document.createElement('div');
      overlay.id = '__dsh_center_alert';
      overlay.style.cssText = 'position:fixed;top:52px;left:50%;transform:translateX(-50%);'
        + 'z-index:2147483647;max-width:356px;width:calc(100% - 32px);pointer-events:none;';
      var destructive = variant === 'destructive';
      var success = variant === 'success';
      var iconColor = destructive ? (pageIsDark() ? '#f87171' : '#ef4444')
        : success ? (pageIsDark() ? '#4ade80' : '#22c55e') : THEME.fg;
      var toast = document.createElement('div');
      toast.setAttribute('role', 'status');
      toast.style.cssText = 'width:100%;box-sizing:border-box;display:flex;align-items:flex-start;gap:12px;'
        + 'padding:16px;background:' + THEME.bg + ';border:1px solid ' + THEME.border + ';border-radius:8px;'
        + 'box-shadow:0 6px 20px rgba(0,0,0,0.18);'
        + 'font-family:system-ui,-apple-system,sans-serif;pointer-events:auto;'
        + 'animation:dshToastIn 0.32s cubic-bezier(0.21,1.02,0.73,1) both;';
      var icon = document.createElement('div');
      icon.style.cssText = 'flex-shrink:0;width:20px;height:20px;margin-top:1px;color:' + iconColor + ';';
      icon.innerHTML = destructive
        ? '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>'
        : '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="m9 12 2 2 4-4"/></svg>';
      var content = document.createElement('div');
      content.style.cssText = 'flex:1;min-width:0;';
      var t = document.createElement('div');
      t.style.cssText = 'font-size:14px;font-weight:500;line-height:1.3;color:' + THEME.fg + ';';
      t.textContent = title;
      var d = document.createElement('div');
      d.style.cssText = 'margin-top:2px;font-size:13px;line-height:1.45;color:' + THEME.muted + ';';
      d.textContent = desc;
      content.appendChild(t); content.appendChild(d);
      toast.appendChild(icon); toast.appendChild(content);
      overlay.appendChild(toast);
      document.body.appendChild(overlay);
      setTimeout(closeCenterAlert, 3200);
    }
    function showCenterAlert(title, desc, variant) {
      fallbackAlert(title, desc, variant);
    }

    // ---- 关于 (About) dialog：仿 shadcn Dialog + Card 观感 ----
    // shadcn 设计令牌：rounded-lg(12px) / border / bg-background / text-foreground
    // / text-muted-foreground / ring / overlay(black/60)。这里用与 wait-home
    // index.css 一致的 token 复刻，不引入外部组件/网络。
    function openAboutDialog() {
      if (document.getElementById('__dsh_about')) return;

      var overlay = document.createElement('div');
      overlay.id = '__dsh_about';
      overlay.setAttribute('role', 'dialog');
      overlay.setAttribute('aria-modal', 'true');
      overlay.style.cssText = 'position:fixed;inset:0;z-index:2147483647;'
        + 'display:flex;align-items:center;justify-content:center;'
        + 'background:rgba(0,0,0,0.6);'
        + 'font-family:system-ui,-apple-system,"Segoe UI",sans-serif;'
        + 'opacity:0;transition:opacity 160ms ease;'
        + '-webkit-app-region:no-drag;';

      var card = document.createElement('div');
      card.style.cssText = 'width:380px;max-width:calc(100vw - 32px);box-sizing:border-box;'
        + 'background:' + THEME.bg + ';border:1px solid ' + THEME.border + ';border-radius:12px;'
        + 'box-shadow:0 16px 48px rgba(0,0,0,0.28);'
        + 'overflow:hidden;transform:translateY(8px) scale(0.98);'
        + 'transition:transform 180ms cubic-bezier(0.21,1.02,0.73,1);';

      // 顶部品牌条：鲸鱼图标 + 标题（shadcn CardHeader 风格，左对齐）
      var header = document.createElement('div');
      header.style.cssText = 'display:flex;align-items:center;gap:12px;padding:20px 20px 12px;';
      var whale = document.createElement('img');
      whale.style.cssText = 'width:36px;height:36px;object-fit:contain;flex-shrink:0;border-radius:8px;';
      invoke('get_whale_icon_url', {})
        .then(function (url) { if (url) whale.src = url; })
        .catch(function () {});
      var titleWrap = document.createElement('div');
      titleWrap.style.cssText = 'min-width:0;';
      var h = document.createElement('div');
      h.textContent = 'DeepSeek Harness';
      h.style.cssText = 'font-size:16px;font-weight:600;line-height:1.2;color:' + THEME.fg + ';';
      var sub = document.createElement('div');
      sub.textContent = '桌面启动器';
      sub.style.cssText = 'margin-top:2px;font-size:12px;color:' + THEME.muted + ';';
      titleWrap.appendChild(h); titleWrap.appendChild(sub);
      header.appendChild(whale); header.appendChild(titleWrap);
      card.appendChild(header);

      // 版本信息卡（shadcn Card 风格，muted 背景 + 圆角 + 分隔行）
      var body = document.createElement('div');
      body.style.cssText = 'padding:4px 20px 16px;display:flex;flex-direction:column;gap:10px;';

      function row(label, value) {
        var r = document.createElement('div');
        r.style.cssText = 'display:flex;align-items:center;justify-content:space-between;gap:12px;'
          + 'padding:10px 12px;background:' + (pageIsDark() ? 'rgba(255,255,255,0.04)' : '#f4f4f5') + ';'
          + 'border:1px solid ' + THEME.border + ';border-radius:8px;';
        var l = document.createElement('span');
        l.textContent = label;
        l.style.cssText = 'font-size:13px;color:' + THEME.muted + ';';
        var v = document.createElement('span');
        v.textContent = value;
        v.style.cssText = 'font-size:13px;font-weight:600;color:' + THEME.fg + ';'
          + 'font-variant-numeric:tabular-nums;';
        r.appendChild(l); r.appendChild(v);
        return r;
      }
      var launcherRow = row('启动器版本', '…');
      var dshRow = row('DeepSeek Harness', '…');
      body.appendChild(launcherRow);
      body.appendChild(dshRow);
      card.appendChild(body);

      // 底部：关闭按钮（shadcn Button 风格，primary 实心）
      var footer = document.createElement('div');
      footer.style.cssText = 'padding:0 20px 20px;display:flex;justify-content:flex-end;';
      var closeBtn = document.createElement('button');
      closeBtn.type = 'button';
      closeBtn.textContent = '知道了';
      closeBtn.style.cssText = 'height:36px;padding:0 16px;border:none;border-radius:8px;cursor:pointer;'
        + 'font-size:14px;font-weight:500;background:' + THEME.fg + ';color:' + THEME.bg + ';'
        + 'transition:opacity 150ms ease,-webkit-app-region:no-drag;';
      closeBtn.onmouseenter = function () { closeBtn.style.opacity = '0.85'; };
      closeBtn.onmouseleave = function () { closeBtn.style.opacity = '1'; };
      closeBtn.onclick = closeAbout;
      footer.appendChild(closeBtn);
      card.appendChild(footer);

      overlay.appendChild(card);
      document.body.appendChild(overlay);

      // 动画入场
      requestAnimationFrame(function () {
        overlay.style.opacity = '1';
        card.style.transform = 'translateY(0) scale(1)';
      });

      // 点击遮罩 / Esc 关闭
      overlay.addEventListener('click', function (e) { if (e.target === overlay) closeAbout(); });
      function onKey(e) { if (e.key === 'Escape') closeAbout(); }
      document.addEventListener('keydown', onKey);
      overlay._onKey = onKey;

      // 拉取真实版本号
      invoke('get_about_info', {})
        .then(function (info) {
          if (!info) return;
          launcherRow.lastChild.textContent = 'v' + (info.launcher_version || '?');
          var d = info.dsh_version;
          dshRow.lastChild.textContent = d ? ('v' + d) : 'npx（未打包离线包）';
          dshRow.lastChild.style.color = d ? THEME.fg : '#f59e0b';
        })
        .catch(function (e) {
          launcherRow.lastChild.textContent = 'v' + (window.__DSH_LAUNCHER_VER__ || '?');
          dshRow.lastChild.textContent = '获取失败';
          dshRow.lastChild.style.color = '#ef4444';
        });
    }

    function closeAbout() {
      var overlay = document.getElementById('__dsh_about');
      if (!overlay) return;
      if (overlay._onKey) document.removeEventListener('keydown', overlay._onKey);
      overlay.style.opacity = '0';
      var card = overlay.firstElementChild;
      if (card) card.style.transform = 'translateY(8px) scale(0.98)';
      setTimeout(function () { if (overlay && overlay.parentNode) overlay.remove(); }, 180);
    }

    // ---- 更新检查与升级流程 ----
    // 检查出有新版 → 弹确认对话框；确认后开始升级，升级过程通过
    // `dsh_update_progress` 事件驱动进度条；完成后弹重启确认对话框。
    var __dsh_update_info = null;
    var __dsh_progress = null;

    function listenDshEvent(handler) {
      return new Promise(function (resolve, reject) {
        try {
          var t = window.__TAURI__;
          if (t && t.event && t.event.listen) {
            t.event.listen('dsh_update_progress', function (e) { handler(e.payload); })
              .then(function (un) { if (un) window.__dsh_unlisten_update = un; resolve(); }, reject);
            return;
          }
        } catch (e) {}
        reject(new Error('Tauri 事件 API 不可用'));
      });
    }

    function ensureDialogStyles() {
      if (document.getElementById('__dsh_dialog_style')) return;
      var st = document.createElement('style');
      st.id = '__dsh_dialog_style';
      st.textContent = '@keyframes dshProgIndeterminate{0%{transform:translateX(-100%)}100%{transform:translateX(300%)}}';
      document.head.appendChild(st);
    }

    function mkDialogOverlay(id) {
      var overlay = document.createElement('div');
      overlay.id = id;
      overlay.setAttribute('role', 'dialog');
      overlay.setAttribute('aria-modal', 'true');
      overlay.style.cssText = 'position:fixed;inset:0;z-index:2147483647;'
        + 'display:flex;align-items:center;justify-content:center;'
        + 'background:rgba(0,0,0,0.6);'
        + 'font-family:system-ui,-apple-system,"Segoe UI",sans-serif;'
        + 'opacity:0;transition:opacity 160ms ease;'
        + '-webkit-app-region:no-drag;';
      return overlay;
    }

    function mkDialogCard() {
      var card = document.createElement('div');
      card.style.cssText = 'width:400px;max-width:calc(100vw - 32px);box-sizing:border-box;'
        + 'background:' + THEME.bg + ';border:1px solid ' + THEME.border + ';border-radius:12px;'
        + 'box-shadow:0 16px 48px rgba(0,0,0,0.28);'
        + 'overflow:hidden;transform:translateY(8px) scale(0.98);'
        + 'transition:transform 180ms cubic-bezier(0.21,1.02,0.73,1);';
      return card;
    }

    function mkDialogHeader(title, sub) {
      var header = document.createElement('div');
      header.style.cssText = 'display:flex;align-items:center;gap:12px;padding:20px 20px 12px;';
      var whale = document.createElement('img');
      whale.style.cssText = 'width:36px;height:36px;object-fit:contain;flex-shrink:0;border-radius:8px;';
      invoke('get_whale_icon_url', {})
        .then(function (url) { if (url) whale.src = url; })
        .catch(function () {});
      var titleWrap = document.createElement('div');
      titleWrap.style.cssText = 'min-width:0;';
      var h = document.createElement('div');
      h.textContent = title;
      h.style.cssText = 'font-size:16px;font-weight:600;line-height:1.2;color:' + THEME.fg + ';';
      var s = document.createElement('div');
      s.textContent = sub;
      s.style.cssText = 'margin-top:2px;font-size:12px;color:' + THEME.muted + ';';
      titleWrap.appendChild(h); titleWrap.appendChild(s);
      header.appendChild(whale); header.appendChild(titleWrap);
      return header;
    }

    function dialogVersionRow(label, value) {
      var r = document.createElement('div');
      r.style.cssText = 'display:flex;align-items:center;justify-content:space-between;gap:12px;'
        + 'padding:10px 12px;background:' + (pageIsDark() ? 'rgba(255,255,255,0.04)' : '#f4f4f5') + ';'
        + 'border:1px solid ' + THEME.border + ';border-radius:8px;';
      var l = document.createElement('span');
      l.textContent = label;
      l.style.cssText = 'font-size:13px;color:' + THEME.muted + ';';
      var v = document.createElement('span');
      v.textContent = value;
      v.style.cssText = 'font-size:13px;font-weight:600;color:' + THEME.fg + ';font-variant-numeric:tabular-nums;';
      r.appendChild(l); r.appendChild(v);
      return r;
    }

    function dialogBtn(label, primary) {
      var b = document.createElement('button');
      b.type = 'button';
      b.textContent = label;
      b.style.cssText = 'height:36px;padding:0 16px;border:none;border-radius:8px;cursor:pointer;'
        + 'font-size:14px;font-weight:500;transition:opacity 150ms ease;'
        + (primary
          ? 'background:' + THEME.fg + ';color:' + THEME.bg + ';'
          : 'background:transparent;color:' + THEME.muted + ';border:1px solid ' + THEME.border + ';');
      b.onmouseenter = function () { b.style.opacity = '0.8'; };
      b.onmouseleave = function () { b.style.opacity = '1'; };
      return b;
    }

    function closeDialog(overlay) {
      if (!overlay) return;
      if (overlay._onKey) document.removeEventListener('keydown', overlay._onKey);
      overlay.style.opacity = '0';
      var card = overlay.firstElementChild;
      if (card) card.style.transform = 'translateY(8px) scale(0.98)';
      setTimeout(function () { if (overlay && overlay.parentNode) overlay.remove(); }, 180);
    }

    function animateIn(overlay, card, dismissable) {
      requestAnimationFrame(function () {
        overlay.style.opacity = '1';
        card.style.transform = 'translateY(0) scale(1)';
      });
      // 进度对话框不允许点击遮罩 / Esc 关闭，避免升级过程中误关
      if (dismissable === false) return;
      overlay.addEventListener('click', function (e) { if (e.target === overlay) closeDialog(overlay); });
      function onKey(e) { if (e.key === 'Escape') closeDialog(overlay); }
      document.addEventListener('keydown', onKey);
      overlay._onKey = onKey;
    }

    function checkDshUpdate() {
      var item = document.getElementById('__dsh_update_item');
      if (!item) return;
      if (item.getAttribute('data-updating') === '1') return;
      item.setAttribute('data-updating', '1');
      var oldText = item.textContent;
      item.textContent = '正在检查 DeepSeek Harness 更新';
      item.style.opacity = '0.7';
      invoke('check_dsh_update', {})
        .then(function (info) {
          item.textContent = oldText;
          item.style.opacity = '1';
          item.removeAttribute('data-updating');
          if (!info || !info.outdated) {
            showCenterAlert('已是最新',
              info && info.latest ? ('DeepSeek Harness 当前已是最新版本（v' + info.latest + '）')
                                 : 'DeepSeek Harness 已是最新版本',
              'success');
            closeMenu();
            return;
          }
          closeMenu();
          openUpdateConfirmDialog(info);
        })
        .catch(function (e) {
          item.textContent = oldText;
          item.style.opacity = '1';
          item.removeAttribute('data-updating');
          showCenterAlert('检查更新失败', e && e.message ? e.message : String(e), 'destructive');
        });
    }

    function openUpdateConfirmDialog(info) {
      if (document.getElementById('__dsh_update_confirm')) return;
      ensureDialogStyles();
      __dsh_update_info = info;
      var overlay = mkDialogOverlay('__dsh_update_confirm');
      var card = mkDialogCard();
      card.appendChild(mkDialogHeader('发现新版本', 'DeepSeek Harness 有新版本可用'));
      var body = document.createElement('div');
      body.style.cssText = 'padding:4px 20px 16px;display:flex;flex-direction:column;gap:10px;';
      body.appendChild(dialogVersionRow('当前版本', info.current ? ('v' + info.current) : '未知'));
      body.appendChild(dialogVersionRow('最新版本', 'v' + info.latest));
      var desc = document.createElement('div');
      desc.style.cssText = 'font-size:13px;line-height:1.5;color:' + THEME.muted + ';padding:0 2px;';
      desc.textContent = '升级过程中将停止当前服务，完成后需重启。是否立即升级？';
      body.appendChild(desc);
      card.appendChild(body);
      var footer = document.createElement('div');
      footer.style.cssText = 'padding:0 20px 20px;display:flex;justify-content:flex-end;gap:10px;';
      var cancelBtn = dialogBtn('稍后', false);
      cancelBtn.onclick = function () { closeDialog(overlay); };
      var goBtn = dialogBtn('立即升级', true);
      goBtn.onclick = function () { startDshUpdate(); };
      footer.appendChild(cancelBtn); footer.appendChild(goBtn);
      card.appendChild(footer);
      overlay.appendChild(card);
      document.body.appendChild(overlay);
      animateIn(overlay, card);
    }

    function startDshUpdate() {
      closeDialog(document.getElementById('__dsh_update_confirm'));
      openUpdateProgressDialog('正在升级', '正在准备升级…', false);
      listenDshEvent(function (p) {
        if (!p) return;
        if (p.phase === 'done') setProgressValue(100, p.message || '升级完成', false);
        else if (p.phase === 'error') setProgressError(p.message);
        else setProgressValue(p.percent, p.message, p.phase === 'installing');
      })
        .then(function () { return invoke('update_dsh', {}); })
        .catch(function () { return invoke('update_dsh', {}); })
        .then(function (result) {
          setProgressValue(100, result || '升级完成', false);
          setTimeout(function () {
            closeProgressDialog();
            var info = __dsh_update_info;
            openRestartConfirmDialog(info && info.latest ? ('v' + info.latest) : '');
          }, 700);
        })
        .catch(function (e) {
          setProgressError(e && e.message ? e.message : String(e));
          setTimeout(function () {
            closeProgressDialog();
            showCenterAlert('更新失败', e && e.message ? e.message : String(e), 'destructive');
          }, 1400);
        });
    }

    function openUpdateProgressDialog(title, message, indeterminate) {
      if (document.getElementById('__dsh_update_progress')) return;
      ensureDialogStyles();
      var overlay = mkDialogOverlay('__dsh_update_progress');
      var card = mkDialogCard();
      card.appendChild(mkDialogHeader(title, '请稍候，不要关闭窗口'));
      var body = document.createElement('div');
      body.style.cssText = 'padding:4px 20px 20px;display:flex;flex-direction:column;gap:12px;';
      var msg = document.createElement('div');
      msg.style.cssText = 'font-size:13px;line-height:1.5;color:' + THEME.muted + ';min-height:18px;';
      msg.textContent = message || '请稍候…';
      var track = document.createElement('div');
      track.style.cssText = 'height:6px;background:' + (pageIsDark() ? 'rgba(255,255,255,0.1)' : '#e5e7eb') + ';border-radius:3px;overflow:hidden;';
      var bar = document.createElement('div');
      bar.style.cssText = 'height:100%;width:0%;border-radius:3px;background:' + THEME.fg + ';transition:width 0.3s ease;';
      track.appendChild(bar);
      body.appendChild(msg); body.appendChild(track);
      card.appendChild(body);
      overlay.appendChild(card);
      document.body.appendChild(overlay);
      animateIn(overlay, card, false);
      __dsh_progress = { overlay: overlay, card: card, msg: msg, bar: bar };
      if (indeterminate) setProgressIndeterminate(message);
      else setProgressValue(10, message, false);
      return overlay;
    }

    function setProgressValue(percent, message, indeterminate) {
      if (!__dsh_progress) return;
      if (message) __dsh_progress.msg.textContent = message;
      if (indeterminate) { setProgressIndeterminate(message); return; }
      var bar = __dsh_progress.bar;
      bar.style.animation = 'none';
      bar.style.background = THEME.fg;
      bar.style.width = Math.min(100, Math.max(0, percent)) + '%';
    }

    function setProgressIndeterminate(message) {
      if (!__dsh_progress) return;
      if (message) __dsh_progress.msg.textContent = message;
      var bar = __dsh_progress.bar;
      bar.style.background = THEME.fg;
      bar.style.width = '33%';
      bar.style.animation = 'dshProgIndeterminate 1.2s ease-in-out infinite';
    }

    function setProgressError(message) {
      if (!__dsh_progress) return;
      if (message) __dsh_progress.msg.textContent = message;
      __dsh_progress.msg.style.color = '#ef4444';
      var bar = __dsh_progress.bar;
      bar.style.animation = 'none';
      bar.style.width = '100%';
      bar.style.background = '#ef4444';
    }

    function closeProgressDialog() {
      if (__dsh_progress && __dsh_progress.overlay) closeDialog(__dsh_progress.overlay);
      __dsh_progress = null;
    }

    function openRestartConfirmDialog(versionLabel) {
      if (document.getElementById('__dsh_restart_confirm')) return;
      ensureDialogStyles();
      var overlay = mkDialogOverlay('__dsh_restart_confirm');
      var card = mkDialogCard();
      card.appendChild(mkDialogHeader('升级完成', 'DeepSeek Harness 已更新' + (versionLabel ? ('到 ' + versionLabel) : '')));
      var body = document.createElement('div');
      body.style.cssText = 'padding:4px 20px 16px;';
      var p = document.createElement('div');
      p.style.cssText = 'font-size:13px;line-height:1.6;color:' + THEME.muted + ';';
      p.textContent = '需要重启 DeepSeek Harness 服务才能生效。是否立即重启？';
      body.appendChild(p);
      card.appendChild(body);
      var footer = document.createElement('div');
      footer.style.cssText = 'padding:0 20px 20px;display:flex;justify-content:flex-end;gap:10px;';
      var laterBtn = dialogBtn('稍后', false);
      laterBtn.onclick = function () {
        closeDialog(overlay);
        showCenterAlert('等待重启', '服务已停止，可通过菜单「重启 DeepSeek Harness」随时重启。', '');
      };
      var restartBtn = dialogBtn('立即重启', true);
      restartBtn.onclick = function () { restartDshNow(); };
      footer.appendChild(laterBtn); footer.appendChild(restartBtn);
      card.appendChild(footer);
      overlay.appendChild(card);
      document.body.appendChild(overlay);
      animateIn(overlay, card);
    }

    function restartDshNow() {
      closeDialog(document.getElementById('__dsh_restart_confirm'));
      openUpdateProgressDialog('正在重启', '正在重启 DeepSeek Harness…', true);
      invoke('restart_dsh', {})
        .then(function () {
          setTimeout(function () {
            closeProgressDialog();
            showCenterAlert('重启完成', 'DeepSeek Harness 已重新启动', 'success');
          }, 500);
        })
        .catch(function (e) {
          setProgressError(e && e.message ? e.message : String(e));
          setTimeout(function () {
            closeProgressDialog();
            showCenterAlert('重启失败', e && e.message ? e.message : String(e), 'destructive');
          }, 1400);
        });
    }


  function build() {
    if (document.getElementById('__dsh_titlebar')) { applyTheme(); return; }
    var bar = document.createElement('div');
    bar.id = '__dsh_titlebar';
    bar.style.cssText = 'position:fixed;top:0;left:0;right:0;height:40px;z-index:2147483647;'
      + '-webkit-app-region:drag;display:flex;align-items:center;'
      + 'font:13px/1 system-ui,-apple-system,sans-serif;'
      + 'user-select:none;box-sizing:border-box;';

    // 标题栏左侧：品牌区（🐋 DeepSeek Harness，点击展开菜单）+ 右侧窗口按钮

      // 菜单面板（仿 shadcn menubar dropdown，点击品牌文字展开）
      var menu = document.createElement('div');
      menu.id = '__dsh_menu';
        menu.setAttribute('role', 'menu');
      menu.style.cssText = 'position:absolute;left:8px;top:44px;min-width:200px;padding:4px;'
        + 'background:' + THEME.bg + ';border:1px solid ' + THEME.border + ';border-radius:8px;'
        + 'box-shadow:0 8px 24px rgba(0,0,0,0.12);z-index:2147483647;display:none;'
        + 'font-size:14px;color:' + THEME.fg + ';-webkit-app-region:no-drag;';
      var aboutItem = document.createElement('button');
      aboutItem.id = '__dsh_about_item';
        aboutItem.type = 'button';
        aboutItem.setAttribute('role', 'menuitem');
      aboutItem.textContent = '关于';
      aboutItem.style.cssText = 'display:flex;align-items:center;width:100%;box-sizing:border-box;padding:8px 10px;border:none;background:transparent;border-radius:6px;cursor:pointer;color:' + THEME.fg + ';font:inherit;-webkit-app-region:no-drag;'
        + 'transition:background-color 150ms ease,color 150ms ease;';
      aboutItem.onmouseenter = function () { aboutItem.style.background = THEME.hover; aboutItem.style.color = THEME.fg; };
      aboutItem.onmouseleave = function () { aboutItem.style.background = 'transparent'; aboutItem.style.color = THEME.fg; };
      aboutItem.onclick = function (e) { e.stopPropagation(); closeMenu(); openAboutDialog(); };

      var updateItem = document.createElement('button');
      updateItem.id = '__dsh_update_item';
        updateItem.type = 'button';
        updateItem.setAttribute('role', 'menuitem');
      updateItem.textContent = '检查 DeepSeek Harness 更新';
      updateItem.style.cssText = 'display:flex;align-items:center;width:100%;box-sizing:border-box;padding:8px 10px;border:none;background:transparent;border-radius:6px;cursor:pointer;color:' + THEME.fg + ';font:inherit;-webkit-app-region:no-drag;'
        + 'transition:background-color 150ms ease,color 150ms ease;';
      updateItem.onmouseenter = function () { updateItem.style.background = THEME.hover; updateItem.style.color = THEME.fg; };
      updateItem.onmouseleave = function () { updateItem.style.background = 'transparent'; updateItem.style.color = THEME.fg; };
      updateItem.onclick = function (e) { e.stopPropagation(); checkDshUpdate(); };
      menu.appendChild(updateItem);

      var restartItem = document.createElement('button');
      restartItem.id = '__dsh_restart_item';
        restartItem.type = 'button';
        restartItem.setAttribute('role', 'menuitem');
      restartItem.textContent = '重启 DeepSeek Harness';
      restartItem.style.cssText = 'display:flex;align-items:center;width:100%;box-sizing:border-box;padding:8px 10px;border:none;background:transparent;border-radius:6px;cursor:pointer;color:' + THEME.fg + ';font:inherit;-webkit-app-region:no-drag;'
        + 'transition:background-color 150ms ease,color 150ms ease;';
      restartItem.onmouseenter = function () { restartItem.style.background = THEME.hover; restartItem.style.color = THEME.fg; };
      restartItem.onmouseleave = function () { restartItem.style.background = 'transparent'; restartItem.style.color = THEME.fg; };
      restartItem.onclick = function (e) { e.stopPropagation(); closeMenu(); restartDshNow(); };
      menu.appendChild(restartItem);

      // 关于放在更新下面
      menu.appendChild(aboutItem);
      bar.appendChild(menu);

      // 品牌文字：鲸鱼图标（icon.png，通过 Rust 命令获取 data URI）+ 文字；点击展开菜单
      var center = document.createElement('div');
      center.id = '__dsh_title_center';
      center.title = '菜单';
      center.setAttribute('role', 'button');
      center.setAttribute('aria-haspopup', 'true');
      center.setAttribute('aria-expanded', 'false');
      center.style.cssText = 'display:flex;align-items:center;gap:8px;margin-left:10px;'
        + 'pointer-events:auto;cursor:pointer;color:' + THEME.fg + ';'
        + '-webkit-app-region:no-drag;'
        + 'font:600 14px/1 system-ui,-apple-system,sans-serif;white-space:nowrap;'
        + 'border-radius:6px;transition:background-color 150ms ease;';
      center.onmouseenter = function () { center.style.background = THEME.hover; };
      center.onmouseleave = function () { center.style.background = 'transparent'; };
      center.onclick = function (e) { e.stopPropagation(); toggleMenu(); };

      var whaleImg = document.createElement('img');
      whaleImg.id = '__dsh_whale_img';
      whaleImg.alt = 'DeepSeek';
      whaleImg.style.cssText = 'width:20px;height:20px;object-fit:contain;flex-shrink:0;';
      // 通过 Rust 命令获取鲸鱼图标的 data:image/png;base64,... URL
      invoke('get_whale_icon_url', {})
        .then(function (url) { if (url) whaleImg.src = url; })
        .catch(function () {});
      center.appendChild(whaleImg);
      center.appendChild(document.createElement('span'));
      center.lastChild.textContent = 'DeepSeek Harness';
      // 顺序：品牌区（🐋 DeepSeek Harness，可点击）→ 窗口按钮（最右，靠 margin-left:auto 撑开）
      bar.appendChild(center);

    
    
    
    
      
    

    
    
    
    
      
    

    var wrap = document.createElement('div');
    wrap.style.cssText = 'margin-left:auto;display:flex;-webkit-app-region:no-drag;';
    wrap.appendChild(mkBtn(ICON_MIN, '最小化', function () { invoke('window_action', { action: 'minimize' }).catch(function (e) { console.error('[titlebar] minimize failed', e); }); }, false, 'min'));
    wrap.appendChild(mkBtn(ICON_MAX, '最大化', function () {
      invoke('window_action', { action: 'toggle_maximize' })
        .then(function (m) { if (typeof m === 'boolean') setMaxIcon(m); })
        .catch(function (e) { console.error('[titlebar] maximize failed', e); });
    }, false, 'max'));
    wrap.appendChild(mkBtn(ICON_CLOSE, '关闭', function () {
      invoke('window_action', { action: 'close' })
        .catch(function () { try { window.close(); } catch (e) {} });
    }, true, 'close'));
    bar.appendChild(wrap);
    document.body.appendChild(bar);

      // 点击品牌文字 / 菜单外部时关闭下拉
      document.addEventListener('click', function (e) {
        var brand = document.getElementById('__dsh_title_center');
        var menu = document.getElementById('__dsh_menu');
        if (menu && brand && !brand.contains(e.target) && !menu.contains(e.target)) closeMenu();
      });


    var st = document.createElement('style');
    st.id = '__dsh_titlebar_style';
    document.head.appendChild(st);
    applyTheme();

    // 品牌文字为静态内容（鲸鱼 SVG + DeepSeek Harness），无需延迟重试提取。

    // Sync the maximize glyph on window resize (handles double-click/other
    // maximize paths, mirroring wait-home's onResized listener).
    window.addEventListener('resize', function () { syncMaxIcon(); });
    syncMaxIcon();

    // Follow theme changes live: OS preference + in-app class/style toggles.
    try {
      var mq = window.matchMedia('(prefers-color-scheme: dark)');
      var onMq = function () { applyTheme(); };
      if (mq.addEventListener) mq.addEventListener('change', onMq); else if (mq.addListener) mq.addListener(onMq);
    } catch (e) {}
    try {
      var mo = new MutationObserver(function () { applyTheme(); });
      mo.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'style', 'data-theme'] });
      mo.observe(document.body, { attributes: true, attributeFilter: ['class', 'style', 'data-theme'] });
    } catch (e) {}
  }

  // Wait until the page has committed a <body> before building (Rust retries on a
  // timer to cover the navigate-commit + SPA render race).
  if (document.body) build();
})();
"#;

/// Shown when we fall back to `npx` (no bundled offline runtime-host). The
/// first-run download of `@deepseek-ai/dsh` can take a while, so tell the user
/// it is not frozen.
const LOADING_NPX_JS: &str = r#"
(function(){
  var el = document.getElementById('msg');
  if (el) {
    el.dataset.locked = '1';
    el.innerHTML = '正在通过 npx 下载并启动 @deepseek-ai/dsh …'
      + '<br><small>（首次运行较慢，请保持网络连接）</small>';
  }
})();
"#;

/// Shown if `dsh web` never becomes ready within the timeout. Replaces the
/// infinite spinner so the user understands what went wrong.
const LOADING_ERROR_JS: &str = r#"
(function(){
  var sp = document.getElementById('spinner');
  if (sp) sp.style.display = 'none';
  var el = document.getElementById('msg');
  if (el) {
    el.dataset.locked = '1';
    el.innerHTML = '启动 DeepSeek Harness 失败 🐋<br><br>'
      + '未能在限定时间内启动本地服务，可能原因：<br>'
      + '1. 离线运行包（runtime-host/node.exe + @deepseek-ai/dsh）未打包进安装包；<br>'
      + '2. 当前网络无法访问 npm，无法以 npx 方式下载 @deepseek-ai/dsh；<br>'
      + '3. 系统缺少 WebView2 运行时。<br><br>'
      + '请确认构建时已包含完整 runtime-host，或检查网络后重试。';
    el.style.color = '#ef4444';
  }
})();
"#;

/// Shared app state: PID of the running `dsh web` server (if any).
struct AppState {
    dsh_pid: Mutex<Option<u32>>,
}

#[derive(serde::Serialize)]
#[allow(dead_code)]
struct UpdateCheckResult {
    current: Option<String>,
    latest: String,
    outdated: bool,
    message: String,
}

/// Progress events emitted on `dsh_update_progress` while `update_dsh` runs, so
/// the injected UI can drive its progress bar by phase.
#[derive(Clone, serde::Serialize)]
struct UpdateProgress {
    phase: String,
    percent: u8,
    message: String,
}

/// Restart the local `dsh web` server and point the main window at the new URL.
fn restart_dsh_service(app: &tauri::AppHandle) -> Result<(), String> {
    let resource_dir = app.path().resource_dir().ok();
    match dsh::launch_and_wait(resource_dir) {
        Some((url, pid)) => {
            let state = app.state::<AppState>();
            *state.dsh_pid.lock().unwrap() = Some(pid);
            let window = app
                .get_webview_window("main")
                .ok_or_else(|| "主窗口不存在".to_string())?;
            let parsed = url::Url::parse(&url).map_err(|e| format!("无效地址 {url}: {e}"))?;
            window
                .navigate(parsed)
                .map_err(|e| format!("导航失败: {e}"))?;
            Ok(())
        }
        None => Err("重启 dsh server 失败".to_string()),
    }
}

/// Custom title-bar window controls, callable from the injected script via
/// `__TAURI_INTERNALS__.invoke('window_action', { action })`. Returns the new
/// maximized state for toggle/query so the □/❐ glyph stays in sync.
#[tauri::command]
fn window_action(app: tauri::AppHandle, action: String) -> Result<Option<bool>, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    match action.as_str() {
        "minimize" => {
            window.minimize().map_err(|e| e.to_string())?;
            Ok(None)
        }
        "toggle_maximize" => {
            if window.is_maximized().map_err(|e| e.to_string())? {
                window.unmaximize().map_err(|e| e.to_string())?;
            } else {
                window.maximize().map_err(|e| e.to_string())?;
            }
            let maximized = window.is_maximized().map_err(|e| e.to_string())?;
            Ok(Some(maximized))
        }
        "query_maximized" => {
            let maximized = window.is_maximized().map_err(|e| e.to_string())?;
            Ok(Some(maximized))
        }
        "close" => {
            window.close().map_err(|e| e.to_string())?;
            Ok(None)
        }
        other => Err(format!("unknown action: {other}")),
    }
}

/// Check the latest published `@deepseek-ai/dsh` version against the local one.
#[tauri::command]
async fn check_dsh_update(app: tauri::AppHandle) -> Result<UpdateCheckResult, String> {
    let resource_dir = app.path().resource_dir().ok();
    tauri::async_runtime::spawn_blocking(move || {
        let info = dsh::check_update(resource_dir)?;
        let current = info.current.clone();
        let latest = info.latest.clone();
        let message = if info.outdated {
            format!("v{} → v{}", current.as_deref().unwrap_or("?"), latest)
        } else {
            format!("已是最新 v{}", latest)
        };
        Ok(UpdateCheckResult {
            current: info.current,
            latest: info.latest,
            outdated: info.outdated,
            message,
        })
    })
    .await
    .map_err(|e| format!("检查更新任务失败: {e}"))?
}

/// Update the local `@deepseek-ai/dsh` package used by this project.
///
/// Stops the running `dsh web` first (avoid EBUSY on Windows), installs the new
/// package, and reports progress over the `dsh_update_progress` event. It does
/// NOT restart the server — the UI asks the user first (the server stays down
/// until they pick "restart now" or use the menu item later).
#[tauri::command]
async fn update_dsh(app: tauri::AppHandle) -> Result<String, String> {
    let resource_dir = app.path().resource_dir().ok();
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let emit = |phase: &str, percent: u8, message: &str| {
            let _ = app2.emit(
                "dsh_update_progress",
                UpdateProgress {
                    phase: phase.to_string(),
                    percent,
                    message: message.to_string(),
                },
            );
        };

        let info = dsh::check_update(resource_dir.clone())?;
        if !info.outdated {
            return Ok(format!("已是最新 v{}", info.latest));
        }

        let old = info.current.clone().unwrap_or_else(|| "?".to_string());
        let new = info.latest.clone();

        // Stop existing dsh web before touching node_modules (avoid EBUSY).
        emit("stopping", 15, "正在停止 DeepSeek Harness 服务…");
        let state = app2.state::<AppState>();
        let old_pid = state.dsh_pid.lock().unwrap().take();
        if let Some(pid) = old_pid {
            dsh::kill_by_pid(pid);
        }

        // Install the new package. The install itself is a single blocking step;
        // the UI switches to an indeterminate bar for this phase.
        emit("installing", 40, &format!("正在下载并安装 v{new} …"));
        match dsh::install_update(resource_dir) {
            Ok(()) => {
                emit("done", 100, "安装完成，请重启服务生效");
                Ok(format!("v{old} → v{new} 更新完成"))
            }
            Err(e) => {
                emit("error", 0, &format!("更新失败：{e}"));
                Err(e)
            }
        }
    })
    .await
    .map_err(|e| format!("更新任务失败: {e}"))?
}

/// Restart the local `dsh web` server. Called from the UI after an update
/// (user picked "restart now") or from the menu item as a manual recovery.
#[tauri::command]
async fn restart_dsh(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || restart_dsh_service(&app))
        .await
        .map_err(|e| format!("重启任务失败: {e}"))?
}

/// Return the DeepSeek whale brand icon as a `data:image/png;base64,...` URL so
/// the injected title-bar script can render it as an `<img>` (with CSS filter
/// for dark-mode inversion).  The PNG is embedded at compile time via
/// `include_bytes!` from `icons/icon.png`.
#[tauri::command]
fn get_whale_icon_url() -> Result<String, String> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(WHALE_ICON_PNG);
    Ok(format!("data:image/png;base64,{b64}"))
}

/// Information shown in the About dialog: the launcher's own version (from
/// `Cargo.toml`) and the version of the `@deepseek-ai/dsh` CLI bundled for this
/// project (offline runtime-host). `dsh_version` is `None` when the offline
/// package is absent and we fall back to `npx` at runtime.
#[derive(serde::Serialize)]
struct AboutInfo {
    launcher_version: String,
    dsh_version: Option<String>,
}

#[tauri::command]
fn get_about_info(app: tauri::AppHandle) -> Result<AboutInfo, String> {
    let resource_dir = app.path().resource_dir().ok();
    Ok(AboutInfo {
        launcher_version: env!("CARGO_PKG_VERSION").to_string(),
        dsh_version: dsh::current_dsh_version(resource_dir),
    })
}

fn main() {
    tauri::Builder::default()
        // Single instance: launching the exe again must surface the existing
        // window (which may be hidden in the tray), not open a second app.
        // The callback runs in the *first* instance when a second one starts.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .setup(|app| {
            // --- system tray ---
            // Use a DPI-matched whale PNG instead of the full-size app icon:
            // Windows renders the tray at 16/20/24/28/32px, and downscaling the
            // 256px default window icon there looks blurry. Pick the exact
            // native size for the monitor the main window is on.
            let scale = app
                .get_webview_window("main")
                .and_then(|w| w.current_monitor().ok().flatten())
                .or_else(|| app.primary_monitor().ok().flatten())
                .map(|m| m.scale_factor())
                .unwrap_or(1.0);
            let tray_icon = if scale >= 2.0 {
                tauri::image::Image::from_bytes(include_bytes!("../icons/tray-32.png"))
            } else if scale >= 1.75 {
                tauri::image::Image::from_bytes(include_bytes!("../icons/tray-28.png"))
            } else if scale >= 1.5 {
                tauri::image::Image::from_bytes(include_bytes!("../icons/tray-24.png"))
            } else if scale >= 1.25 {
                tauri::image::Image::from_bytes(include_bytes!("../icons/tray-20.png"))
            } else {
                tauri::image::Image::from_bytes(include_bytes!("../icons/tray-16.png"))
            }
            .expect("tray icon png");

            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&show_item, &PredefinedMenuItem::separator(app)?, &quit_item],
            )?;

            let _tray = TrayIconBuilder::new()
                .icon(tray_icon)
                .tooltip("DeepSeek Harness")
                .menu(&menu)
                // Left click pops the window straight up (no menu on left
                // click); the menu stays on right click.
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.unminimize();
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                })
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    _ => {}
                })
                .build(app)?;

            // Custom title bar is injected from the dsh thread after navigation
            // commits (see below). `on_page_load` only exists on WebviewWindowBuilder
            // in Tauri 2.11.x, not on a live WebviewWindow, so we poll instead.

            // --- spawn dsh web in a background thread, navigate when ready ---
            // The managed state must be fetched *inside* the thread via the
            // cloned AppHandle: a borrowed `&AppState` is not `'static` and
            // cannot cross the `std::thread::spawn` boundary.
            let handle = app.handle().clone();
            let resource_dir = app.path().resource_dir().ok();
            // If no bundled offline runtime-host exists we fall back to `npx`,
            // whose first-run download can take a while — tell the user so the
            // loading page does not look frozen.
            let mode = dsh::launch_mode(resource_dir.clone());
            if mode == "npx" {
                if let Some(w) = handle.get_webview_window("main") {
                    let _ = w.eval(LOADING_NPX_JS);
                }
            }
            std::thread::spawn(move || {
                let state = handle.state::<AppState>();
                match dsh::launch_and_wait(resource_dir) {
                    Some((url, pid)) => {
                        *state.dsh_pid.lock().unwrap() = Some(pid);
                        match url::Url::parse(&url) {
                            Ok(parsed) => {
                                if let Some(w) = handle.get_webview_window("main") {
                                    if let Err(e) = w.navigate(parsed) {
                                        eprintln!("[dsh] navigate failed: {e}");
                                    } else {
                                        // Keep the custom title-bar injected across the initial
                                        // load and later SPA refreshes. The script is
                                        // idempotent: if the bar already exists it returns.
                                        let w = w.clone();
                                        std::thread::spawn(move || {
                                            let js = TITLEBAR_INJECT_JS.to_string();
                                            loop {
                                                let _ = w.eval(js.as_str());
                                                { /* keep retrying across refreshes */ }
                                                std::thread::sleep(
                                                    std::time::Duration::from_millis(500),
                                                );
                                            }
                                        });
                                    }
                                }
                            }
                            Err(e) => eprintln!("[dsh] invalid harness url `{url}`: {e}"),
                        }
                    }
                    None => {
                        eprintln!("[dsh] failed to launch harness");
                        // Surface a clear error instead of leaving the app
                        // stuck on the loading spinner forever.
                        if let Some(w) = handle.get_webview_window("main") {
                            let _ = w.eval(LOADING_ERROR_JS);
                        }
                    }
                }
            });

            Ok(())
        })
        .manage(AppState {
            dsh_pid: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            window_action,
            check_dsh_update,
            update_dsh,
            restart_dsh,
            get_whale_icon_url,
            get_about_info
        ])
        .on_window_event(|window, event| {
            // Closing the window hides it (tray owns the lifetime), it does not quit.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running DeepSeek Harness launcher")
        .run(|app, event| {
            // Actively stop the dsh web server when the app exits.
            if matches!(
                event,
                tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
            ) {
                // Chain the state lookup into the `.take()` so `State` and its
                // `MutexGuard` are both statement-scoped temporaries (dropped in
                // the correct order). A named `let state` would outlive the guard.
                let pid = app.state::<AppState>().dsh_pid.lock().unwrap().take();
                if let Some(pid) = pid {
                    dsh::kill_by_pid(pid);
                }
            }
        });
}
