function $(id){return document.getElementById(id);}

function updateCpuLimitLabel(v){$('cpuLimitLabel').innerText = (window.i18n? i18n.t('limit.cpu',{v}) : `CPU Limit: ${v}%`);}
function updateRamLimitLabel(v){$('ramLimitLabel').innerText = (window.i18n? i18n.t('limit.ram',{v}) : `RAM Limit: ${v}%`);}
function updateGpuLimitLabel(v){$('gpuLimitLabel').innerText = (window.i18n? i18n.t('limit.gpu',{v}) : `GPU Limit: ${v}%`);}
function updateGpuMemoryLimitLabel(v){$('gpuMemoryLimitLabel').innerText = (window.i18n? i18n.t('limit.gpu_mem',{v}) : `GPU Memory Limit: ${v}%`);}
function updateDiskLimitLabel(v){$('diskLimitLabel').innerText = (window.i18n? i18n.t('limit.disk',{v}) : `Disk Limit: ${v}%`);}
function updateNetworkLimitLabel(v){$('networkLimitLabel').innerText = (window.i18n? i18n.t('limit.network',{v}) : `Network Limit: ${v} Mbps`);}

async function loadSettings(){
  try{
    const res = await fetch('/api/settings');
    const data = await res.json();
    if(!data.success){ throw new Error(data.error || 'load failed'); }
    const limits = data.limits || {};
    const detected = data.detected || {};
    const current = data.current || {};
    $('cpuLimit').value = limits.cpu_percent ?? 100; updateCpuLimitLabel($('cpuLimit').value);
    $('ramLimit').value = limits.memory_percent ?? 100; updateRamLimitLabel($('ramLimit').value);
    $('gpuLimit').value = limits.gpu_percent ?? 100; updateGpuLimitLabel($('gpuLimit').value);
    $('gpuMemoryLimit').value = limits.gpu_memory_percent ?? 100; updateGpuMemoryLimitLabel($('gpuMemoryLimit').value);
    $('diskLimit').value = limits.disk_percent ?? 100; updateDiskLimitLabel($('diskLimit').value);
    $('networkLimit').value = limits.network_mbps ?? 1000; updateNetworkLimitLabel($('networkLimit').value);

    // 填入效能概覽
    const setText = (id, v)=>{ const el=document.getElementById(id); if(el) el.innerText = v; };
    setText('detectedCpuScore', detected.cpu_score ?? '-');
    setText('detectedMemoryGB', detected.memory_gb ?? '-');
    setText('detectedGpuScore', detected.gpu_score ?? '-');
    setText('detectedGpuMemoryGB', detected.gpu_memory_gb ?? '-');
    setText('advCpu', current.advertised_cpu_score ?? '-');
    setText('advMem', current.advertised_memory_gb ?? '-');
    setText('advGpu', current.advertised_gpu_score ?? '-');
    setText('advGpuMem', current.advertised_gpu_memory_gb ?? '-');
  }catch(e){
    console.error('load settings failed', e);
    const el = document.getElementById('saveStatus');
    if(el){ el.innerText = (window.i18n? i18n.t('save.load_fail',{msg: e.message}) : `讀取設定失敗: ${e.message}`); el.className = 'status error'; }
  }
}

async function saveSettings(){
  const payload = {
    cpu_percent: parseInt($('cpuLimit').value, 10),
    memory_percent: parseInt($('ramLimit').value, 10),
    gpu_percent: parseInt($('gpuLimit').value, 10),
    gpu_memory_percent: parseInt($('gpuMemoryLimit').value, 10),
    disk_percent: parseInt($('diskLimit').value, 10),
    network_mbps: parseInt($('networkLimit').value, 10)
  };
  try{
    const res = await fetch('/api/settings', {
      method: 'POST',
      headers: {'Content-Type': 'application/json'},
      body: JSON.stringify(payload)
    });
    const data = await res.json();
    const el = document.getElementById('saveStatus');
    if(data.success){
      if(el){ el.innerText = (window.i18n? i18n.t('save.success') : '已保存設定，註冊或心跳時將採用上限。'); el.className = 'status ok'; }
    }else{
      if(el){ const msg = data.error || 'unknown error'; el.innerText = (window.i18n? i18n.t('save.fail',{msg}) : `保存失敗: ${msg}`); el.className = 'status error'; }
    }
  }catch(e){
    const el = document.getElementById('saveStatus');
    if(el){ el.innerText = (window.i18n? i18n.t('save.fail',{msg: e.message}) : `保存失敗: ${e.message}`); el.className = 'status error'; }
  }
}

document.addEventListener('DOMContentLoaded', loadSettings);

// 當語言切換時，重新渲染以多國語系顯示的動態標籤
document.addEventListener('i18n:languageChanged', function(){
  const $ = (id)=>document.getElementById(id);
  if($('cpuLimit')) updateCpuLimitLabel($('cpuLimit').value);
  if($('ramLimit')) updateRamLimitLabel($('ramLimit').value);
  if($('gpuLimit')) updateGpuLimitLabel($('gpuLimit').value);
  if($('gpuMemoryLimit')) updateGpuMemoryLimitLabel($('gpuMemoryLimit').value);
  if($('diskLimit')) updateDiskLimitLabel($('diskLimit').value);
  if($('networkLimit')) updateNetworkLimitLabel($('networkLimit').value);
});
