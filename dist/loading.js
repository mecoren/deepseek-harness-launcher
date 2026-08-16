document.body.innerHTML = `
<div style="
  display:flex;align-items:center;justify-content:center;
  height:100vh;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;
  color:#333;background:#fafafa;margin:0;
">
  <div style="text-align:center">
    <div style="
      width:40px;height:40px;margin:0 auto 16px;
      border:3px solid #e2e8f0;border-top-color:#1a1a1a;border-radius:50%;
      animation:spin 0.8s linear infinite;
    "></div>
    <p id="msg" style="font-size:14px;color:#666">Starting DeepSeek Harness&hellip;</p>
  </div>
</div>
<style>@keyframes spin{to{transform:rotate(360deg)}}</style>
`;
(function(){
  const msgs=[
    "Starting DeepSeek Harness\u2026",
    "Launching local server\u2026", 
    "Loading AI workspace\u2026",
    "Almost ready\u2026"
  ];
  let i=0;
  setInterval(function(){i=(i+1)%msgs.length;document.getElementById('msg').textContent=msgs[i]},3000);
})();
