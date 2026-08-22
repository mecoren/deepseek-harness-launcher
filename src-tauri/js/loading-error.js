// Shown if `dsh web` never becomes ready. Replaces the infinite spinner so the
// user understands what went wrong. `__REASON__` is replaced at runtime by the
// launcher with a JSON-encoded detail string (may be empty).
(function(){
  var sp = document.getElementById('spinner');
  if (sp) sp.style.display = 'none';
  var el = document.getElementById('msg');
  if (el) {
    el.dataset.locked = '1';
    el.innerHTML = '启动 DeepSeek Harness 失败 🐋<br><br>'
      + __REASON__
      + '<br><br>可能原因：<br>'
      + '1. 离线运行包（runtime-host/node.exe + @deepseek-ai/dsh）未打包进安装包；<br>'
      + '2. 当前网络无法访问 npm，无法以 npx 方式下载 @deepseek-ai/dsh；<br>'
      + '3. 系统缺少 WebView2 运行时。<br><br>'
      + '请确认构建时已包含完整 runtime-host，或检查网络后在托盘菜单重试。';
    el.style.color = '#ef4444';
  }
})();
