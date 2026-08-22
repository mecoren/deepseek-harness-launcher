// Injected into every loaded page: a custom title bar that matches the DeepSeek
// sidebar (`#f9fafb` in light, `#1b1b1c` in dark mode), plus a 1px same-color inset
// border so the side frame and the bar share one color. The bar is draggable; the
// right-side window buttons are Windows-11-style 46x40px SVG caption buttons
// (crisp at any DPI), muted-foreground icon color, bg-muted hover,
// red (#E81123) close hover, a centered title, and a maximize icon that follows
// the window's maximized state (queried via a resize listener). The buttons talk
// to the `window_action` command over `__TAURI_INTERNALS__.invoke` (the
// always-injected IPC bridge), so they do NOT depend on `withGlobalTauri`.
// `close` triggers a CloseRequested, which our handler intercepts to hide the
// window (the tray owns the app lifetime). Theme is followed live via matchMedia
// + MutationObserver.
//
// The script is idempotent: if the bar already exists it only re-applies theme.
(function () {
  // ---- theme -------------------------------------------------------------
  // Theme tokens, recomputed on every applyTheme() so hover/colors stay correct
  // after a dark/light switch. Colors mirror shadcn zinc tokens:
  // muted-foreground / foreground / muted (hover bg).
  var THEME = { bg:'#f9fafb', border:'#e5e7eb', muted:'#737373', fg:'#1a1a1a',
                hover:'#f4f4f5', closeHover:'#E81123', closeFg:'#ffffff' };

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

  // Coalesce bursts of theme-change signals (MutationObserver fires once per
  // attribute change on a busy SPA) into one repaint per animation frame.
  var themeScheduled = false;
  function scheduleApplyTheme() {
    if (themeScheduled) return;
    themeScheduled = true;
    requestAnimationFrame(function () { themeScheduled = false; applyTheme(); });
  }

  // ---- IPC -----------------------------------------------------------------
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

  // ---- caption buttons -----------------------------------------------------
  // Windows-11-style caption icons as inline SVG (stroke=currentColor so hover
  // colors keep working). 12px vector glyphs on 46x40px buttons look much
  // crisper than small text glyphs, especially on HiDPI.
  var ICON_MIN = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M1.5 6h9" stroke="currentColor" stroke-width="1"/></svg>';
  var ICON_MAX = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="2" y="2" width="8" height="8" rx="0.75" fill="none" stroke="currentColor" stroke-width="1"/></svg>';
  var ICON_RESTORE = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M3 7.5V3a1 1 0 0 1 1-1h3.5a1 1 0 0 1 1 1v1.5" fill="none" stroke="currentColor" stroke-width="1"/><rect x="4.5" y="4.5" width="6" height="6" rx="0.75" fill="none" stroke="currentColor" stroke-width="1"/></svg>';
  var ICON_CLOSE = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 2.5l7 7M9.5 2.5l-7 7" stroke="currentColor" stroke-width="1" stroke-linecap="round"/></svg>';

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

  function setMaxIcon(m) {
    var b = document.querySelector('[data-kind="max"]');
    if (b) b.innerHTML = m ? ICON_RESTORE : ICON_MAX;
  }
  // Toggle the maximize glyph between the two states.
  function syncMaxIcon() {
    invoke('window_action', { action: 'query_maximized' })
      .then(function (m) { setMaxIcon(m === true); })
      .catch(function () {});
  }

  // ---- toast（顶部居中，shadcn Sonner 观感，内联实现，无外部依赖） ----------
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
  function showCenterAlert(title, desc, variant) {
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

  // ---- dialog toolkit（仿 shadcn Dialog + Card，内联实现） ------------------
  // 设计令牌：rounded-lg(12px) / border / bg-background / text-foreground
  // / text-muted-foreground / ring / overlay(black/60)。不引入外部组件/网络。
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

  function mkDialogCard(widthPx) {
    var card = document.createElement('div');
    card.style.cssText = 'width:' + (widthPx || 400) + 'px;max-width:calc(100vw - 32px);box-sizing:border-box;'
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

  // ---- 关于 (About) 对话框 ---------------------------------------------------
  function openAboutDialog() {
    if (document.getElementById('__dsh_about')) return;
    ensureDialogStyles();

    var overlay = mkDialogOverlay('__dsh_about');
    var card = mkDialogCard(380);
    card.appendChild(mkDialogHeader('DeepSeek Harness', '桌面启动器'));

    var body = document.createElement('div');
    body.style.cssText = 'padding:4px 20px 16px;display:flex;flex-direction:column;gap:10px;';
    var launcherRow = dialogVersionRow('启动器版本', '…');
    var dshRow = dialogVersionRow('DeepSeek Harness', '…');
    body.appendChild(launcherRow);
    body.appendChild(dshRow);
    card.appendChild(body);

    var footer = document.createElement('div');
    footer.style.cssText = 'padding:0 20px 20px;display:flex;justify-content:flex-end;';
    var closeBtn = dialogBtn('知道了', true);
    closeBtn.onclick = function () { closeDialog(document.getElementById('__dsh_about')); };
    footer.appendChild(closeBtn);
    card.appendChild(footer);

    overlay.appendChild(card);
    document.body.appendChild(overlay);
    animateIn(overlay, card);

    // 拉取真实版本号
    invoke('get_about_info', {})
      .then(function (info) {
        if (!info) return;
        launcherRow.lastChild.textContent = 'v' + (info.launcher_version || '?');
        var d = info.dsh_version;
        dshRow.lastChild.textContent = d ? ('v' + d) : 'npx（未打包离线包）';
        dshRow.lastChild.style.color = d ? THEME.fg : '#f59e0b';
      })
      .catch(function () {
        launcherRow.lastChild.textContent = 'v' + (window.__DSH_LAUNCHER_VER__ || '?');
        dshRow.lastChild.textContent = '获取失败';
        dshRow.lastChild.style.color = '#ef4444';
      });
  }

  // ---- 更新检查与升级流程 ----------------------------------------------------
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
    // 先挂事件监听再触发升级；监听不可用不应阻止升级本身，
    // 升级失败也绝不能触发第二次 update_dsh 执行。
    var listenP = listenDshEvent(function (p) {
      if (!p) return;
      if (p.phase === 'done') setProgressValue(100, p.message || '升级完成', false);
      else if (p.phase === 'error') setProgressError(p.message);
      else setProgressValue(p.percent, p.message, p.phase === 'installing');
    });
    Promise.resolve(listenP)
      .catch(function () {})
      .then(function () { return invoke('update_dsh', {}); })
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

  // ---- 下拉菜单 --------------------------------------------------------------
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

  // 菜单项统一工厂：三个条目共用同一套样式与 hover 行为。
  function mkMenuItem(id, label, onClick) {
    var item = document.createElement('button');
    item.id = id;
    item.type = 'button';
    item.setAttribute('role', 'menuitem');
    item.textContent = label;
    item.style.cssText = 'display:flex;align-items:center;width:100%;box-sizing:border-box;padding:8px 10px;border:none;background:transparent;border-radius:6px;cursor:pointer;color:' + THEME.fg + ';font:inherit;-webkit-app-region:no-drag;'
      + 'transition:background-color 150ms ease,color 150ms ease;';
    item.onmouseenter = function () { item.style.background = THEME.hover; item.style.color = THEME.fg; };
    item.onmouseleave = function () { item.style.background = 'transparent'; item.style.color = THEME.fg; };
    item.onclick = onClick;
    return item;
  }

  // ---- 标题栏构建 ------------------------------------------------------------
  function build() {
    if (document.getElementById('__dsh_titlebar')) { applyTheme(); return; }
    var bar = document.createElement('div');
    bar.id = '__dsh_titlebar';
    bar.style.cssText = 'position:fixed;top:0;left:0;right:0;height:40px;z-index:2147483647;'
      + '-webkit-app-region:drag;display:flex;align-items:center;'
      + 'font:13px/1 system-ui,-apple-system,sans-serif;'
      + 'user-select:none;box-sizing:border-box;';

    // 菜单面板（仿 shadcn menubar dropdown，点击品牌文字展开）
    var menu = document.createElement('div');
    menu.id = '__dsh_menu';
    menu.setAttribute('role', 'menu');
    menu.style.cssText = 'position:absolute;left:8px;top:44px;min-width:200px;padding:4px;'
      + 'background:' + THEME.bg + ';border:1px solid ' + THEME.border + ';border-radius:8px;'
      + 'box-shadow:0 8px 24px rgba(0,0,0,0.12);z-index:2147483647;display:none;'
      + 'font-size:14px;color:' + THEME.fg + ';-webkit-app-region:no-drag;';

    var updateItem = mkMenuItem('__dsh_update_item', '检查 DeepSeek Harness 更新',
      function (e) { e.stopPropagation(); checkDshUpdate(); });
    menu.appendChild(updateItem);

    var restartItem = mkMenuItem('__dsh_restart_item', '重启 DeepSeek Harness',
      function (e) { e.stopPropagation(); closeMenu(); restartDshNow(); });
    menu.appendChild(restartItem);

    var aboutItem = mkMenuItem('__dsh_about_item', '关于',
      function (e) { e.stopPropagation(); closeMenu(); openAboutDialog(); });
    // 关于放在更新下面
    menu.appendChild(aboutItem);
    bar.appendChild(menu);

    // 品牌区：鲸鱼图标（icon.png，通过 Rust 命令获取 data URI）+ 文字；点击展开菜单
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
    // 顺序：品牌区（可点击）→ 窗口按钮（最右，靠 margin-left:auto 撑开）
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

    // Sync the maximize glyph on window resize (handles double-click/other
    // maximize paths).
    window.addEventListener('resize', function () { syncMaxIcon(); });
    syncMaxIcon();

    // Follow theme changes live: OS preference + in-app class/style toggles.
    try {
      var mq = window.matchMedia('(prefers-color-scheme: dark)');
      var onMq = function () { scheduleApplyTheme(); };
      if (mq.addEventListener) mq.addEventListener('change', onMq); else if (mq.addListener) mq.addListener(onMq);
    } catch (e) {}
    try {
      var mo = new MutationObserver(function () { scheduleApplyTheme(); });
      mo.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'style', 'data-theme'] });
      mo.observe(document.body, { attributes: true, attributeFilter: ['class', 'style', 'data-theme'] });
    } catch (e) {}
  }

  // Wait until the page has committed a <body> before building (Rust retries on
  // a timer to cover the navigate-commit + SPA render race).
  if (document.body) build();
})();
