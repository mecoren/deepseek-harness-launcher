// Injected when we fall back to `npx` (no bundled offline runtime-host). The
// first-run download of `@deepseek-ai/dsh` can take a while, so tell the user
// it is not frozen.
(function(){
  var el = document.getElementById('msg');
  if (el) {
    el.dataset.locked = '1';
    el.innerHTML = '正在通过 npx 下载并启动 @deepseek-ai/dsh …'
      + '<br><small>（首次运行较慢，请保持网络连接）</small>';
  }
})();
