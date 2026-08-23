/**
 * debug.js — 错误监控 & 卡顿检测 & 联网消息追踪
 * 捕获运行时错误并显示在页面角落，日志可复制发送给开发者
 * 增强版：WebSocket 消息 / 游戏事件 / console 全捕获
 */
(function () {
    'use strict';

    // 安装包版本禁用调试面板：在 index.html 中设置 window.__DISABLE_DEBUG__ = true
    if (window.__DISABLE_DEBUG__) return;

    const errorLog = [];
    const MAX_LOG = 200;

    function esc(t) { var d = document.createElement('div'); d.textContent = t || ''; return d.innerHTML; }
    function ts() { return new Date().toLocaleTimeString() + '.' + String(new Date().getMilliseconds()).padStart(3,'0'); }

    function createPanel() {
        var panel = document.createElement('div');
        panel.id = 'debugPanel';
        panel.style.cssText = 'display:none;position:fixed;bottom:12px;right:12px;z-index:99999;font-family:Consolas,monospace;font-size:11px;line-height:1.4;width:520px;max-height:420px;border-radius:8px;overflow:hidden;background:rgba(20,20,28,0.96);color:#ff6b6b;border:1px solid rgba(255,107,107,0.35);box-shadow:0 8px 32px rgba(0,0,0,0.5);backdrop-filter:blur(4px);';

        var hdr = document.createElement('div');
        hdr.style.cssText = 'display:flex;justify-content:space-between;align-items:center;padding:8px 12px;background:rgba(233,69,96,0.2);border-bottom:1px solid rgba(255,107,107,0.15);cursor:pointer;user-select:none;';
        hdr.innerHTML = '<span style="font-weight:bold;color:#ff6b6b;">\u{1F6A8} 运行时日志</span><span style="display:flex;gap:8px;align-items:center;"><span id="debugCount" style="color:#aaa;font-size:10px;">0</span><span id="debugToggleIcon" style="color:#888;">▲</span><span id="debugCloseBtn" style="cursor:pointer;color:#888;font-size:14px;line-height:1;padding:0 2px;" title="关闭面板">✕</span></span>';

        var tabs = document.createElement('div');
        tabs.id = 'debugTabs';
        tabs.style.cssText = 'display:flex;gap:4px;padding:4px 10px;background:rgba(0,0,0,0.25);border-bottom:1px solid rgba(255,255,255,0.05);font-size:10px;';
        var tabNames = ['all','error','network','game','ai'];
        var tabColors = ['#aaa','#ff6b6b','#a29bfe','#55efc4','#fdcb6e'];
        tabs.innerHTML = tabNames.map(function(t,i){ return '<span class="debug-tab" data-tab="'+t+'" style="cursor:pointer;padding:2px 8px;border-radius:10px;background:rgba(255,255,255,0.06);color:'+tabColors[i]+';'+(t==='all'?'font-weight:bold;':'')+'">'+t+'</span>'; }).join('');

        var body = document.createElement('div');
        body.id = 'debugBody';
        body.style.cssText = 'max-height:320px;overflow-y:auto;padding:0;';

        var ft = document.createElement('div');
        ft.style.cssText = 'display:flex;justify-content:space-between;padding:6px 10px;background:rgba(0,0,0,0.3);border-top:1px solid rgba(255,107,107,0.1);font-size:10px;';
        ft.innerHTML = '<span style="color:#666;">拖动标题栏移动 · 点标签过滤</span><button id="debugCopyBtn" style="background:#c0392b;color:#fff;border:none;border-radius:4px;padding:3px 10px;cursor:pointer;font-size:10px;">\u{1F4CB} 复制日志</button>';

        panel.appendChild(hdr);
        panel.appendChild(tabs);
        panel.appendChild(body);
        panel.appendChild(ft);
        document.body.appendChild(panel);

        // Drag
        var isDrag = false, dx0, dy0, px0, py0;
        hdr.addEventListener('mousedown', function(e){
            isDrag = true; dx0 = e.clientX; dy0 = e.clientY;
            var r = panel.getBoundingClientRect();
            panel.style.left = r.left+'px'; panel.style.top = r.top+'px';
            panel.style.bottom = 'auto'; panel.style.right = 'auto';
            px0 = r.left; py0 = r.top;
            e.preventDefault();
        });
        document.addEventListener('mousemove', function(e){
            if(!isDrag) return;
            panel.style.left = (px0 + e.clientX - dx0)+'px';
            panel.style.top = (py0 + e.clientY - dy0)+'px';
        });
        document.addEventListener('mouseup', function(){ isDrag = false; });

        hdr.addEventListener('click', function(e){
            if(e.target.closest('#debugCopyBtn')||e.target.closest('button')) return;
            body.style.display = body.style.display === 'none' ? 'block' : 'none';
            document.getElementById('debugToggleIcon').textContent = body.style.display === 'none' ? '▼' : '▲';
        });

        tabs.addEventListener('click', function(e){
            var tab = e.target.closest('.debug-tab');
            if(!tab) return;
            document.querySelectorAll('.debug-tab').forEach(function(t){ t.style.fontWeight = ''; });
            tab.style.fontWeight = 'bold';
            var filter = tab.dataset.tab;
            body.querySelectorAll('.log-entry').forEach(function(el){
                var type = el.dataset.type || 'error';
                el.style.display = (filter === 'all' || type === filter) ? '' : 'none';
            });
        });

        document.getElementById('debugCloseBtn').addEventListener('click', function(e){ e.stopPropagation(); panel.style.display = 'none'; });

        document.getElementById('debugCopyBtn').addEventListener('click', function(){
            var activeTab = document.querySelector('.debug-tab[style*="bold"]');
            var filter = activeTab ? activeTab.dataset.tab : 'all';
            var text = errorLog.filter(function(e){ return filter === 'all' || e.type === filter; }).map(function(e){
                return '['+e.time+'] ['+e.type+'] '+e.msg+(e.src?'\n   at '+e.src+':'+e.line+':'+e.col:'')+(e.detail?'\n   '+e.detail:'')+(e.stack?'\n'+e.stack:'');
            }).join('\n\n---\n\n');
            if(!text){ alert('该分类暂无日志'); return; }
            if(navigator.clipboard){
                navigator.clipboard.writeText(text).then(function(){ showToast('✅ 日志已复制'); }).catch(function(){ fallbackCopy(text); });
            } else { fallbackCopy(text); }
            function fallbackCopy(t){
                var ta = document.createElement('textarea');
                ta.value = t; ta.style.cssText = 'position:fixed;left:-9999px;top:0;';
                document.body.appendChild(ta); ta.select();
                document.execCommand('copy'); document.body.removeChild(ta);
                showToast('✅ 日志已复制');
            }
        });

        // Fab button
        var fab = document.createElement('div');
        fab.id = 'debugFab';
        fab.textContent = '\u{1F41B}';
        fab.title = '打开调试面板';
        fab.style.cssText = 'position:fixed;top:60px;right:12px;z-index:99998;width:36px;height:36px;border-radius:50%;background:rgba(233,69,96,0.8);color:#fff;font-size:18px;display:flex;align-items:center;justify-content:center;cursor:pointer;box-shadow:0 2px 8px rgba(0,0,0,0.3);';
        fab.addEventListener('click', function(){ panel.style.display = panel.style.display === 'none' ? 'block' : 'none'; });
        document.body.appendChild(fab);
    }

    function showToast(msg){
        var t = document.createElement('div');
        t.textContent = msg;
        t.style.cssText = 'position:fixed;bottom:60px;right:20px;z-index:100000;background:rgba(0,0,0,0.85);color:#4caf50;padding:8px 16px;border-radius:6px;font-family:monospace;font-size:12px;transition:opacity 0.5s;';
        document.body.appendChild(t);
        setTimeout(function(){ t.style.opacity = '0'; setTimeout(function(){ t.remove(); },500); },2000);
    }
    window.showDebugToast = showToast;

    function addEntry(type, msg, src, line, col, stack, detail){
        var entry = { time: ts(), type: type||'error', msg: msg, src: src||'', line: line||0, col: col||0, stack: stack||'', detail: detail||'' };
        errorLog.push(entry);
        if(errorLog.length > MAX_LOG) errorLog.shift();
        var panel = document.getElementById('debugPanel'), body = document.getElementById('debugBody'), cnt = document.getElementById('debugCount');
        if(!panel||!body) return;
        // 不再自动弹出面板 — 用户点击右下角红点按钮手动打开
        if(cnt) cnt.textContent = errorLog.length;
        var colors = { error:'#ff6b6b', network:'#a29bfe', game:'#55efc4', ai:'#fdcb6e', console:'#aaa' };
        var color = colors[type]||'#aaa';
        var el = document.createElement('div');
        el.className = 'log-entry'; el.dataset.type = type;
        el.style.cssText = 'padding:5px 10px;border-bottom:1px solid rgba(255,255,255,0.04);font-size:10px;';
        el.innerHTML = '<div><span style="color:#666;">'+esc(entry.time)+'</span> <span style="color:'+color+';">['+type+']</span> <span style="color:#e0e0e0;">'+esc(entry.msg)+'</span></div>'+
            (entry.detail ? '<div style="color:#666;font-size:9px;margin-top:2px;word-break:break-all;">'+esc(entry.detail)+'</div>' : '')+
            (entry.src ? '<div style="color:#444;font-size:8px;">'+esc(entry.src)+':'+entry.line+'</div>' : '')+
            (entry.stack ? '<pre style="font-size:8px;color:#444;margin:2px 0 0;white-space:pre-wrap;max-height:40px;overflow:hidden;">'+esc(entry.stack.split('\n').slice(0,3).join('\n'))+'</pre>' : '');
        body.appendChild(el);
        body.scrollTop = body.scrollHeight;
    }

    window.copySingleError = function(idx){
        var e = errorLog[idx]; if(!e) return;
        var text = '['+e.time+'] ['+e.type+'] '+e.msg+(e.src?'\n   at '+e.src+':'+e.line+':'+e.col:'')+(e.detail?'\n   '+e.detail:'')+(e.stack?'\n'+e.stack:'');
        if(navigator.clipboard){ navigator.clipboard.writeText(text); }
        else {
            var ta = document.createElement('textarea');
            ta.value = text; ta.style.cssText = 'position:fixed;left:-9999px;';
            document.body.appendChild(ta); ta.select();
            document.execCommand('copy'); document.body.removeChild(ta);
        }
        showToast('\u{1F4CB} 已复制');
    };

    window.getDebugLog = function(){
        return errorLog.map(function(e){ return '['+e.time+'] ['+e.type+'] '+e.msg+(e.src?'\n   at '+e.src+':'+e.line+':'+e.col:'')+(e.detail?'\n   '+e.detail:'')+(e.stack?'\n'+e.stack:''); }).join('\n\n---\n\n');
    };

    // 1. Global error / rejection
    window.onerror = function(msg, src, line, col, err){ addEntry('error', String(msg), src, line, col, err&&err.stack?err.stack:''); return false; };
    window.addEventListener('unhandledrejection', function(e){
        var r = e.reason; addEntry('error', 'Promise: '+(r&&r.message?r.message:String(r||'unknown')), '',0,0, r&&r.stack?r.stack:'');
    });

    // 2. WebSocket monitor
    (function(){
        var origWS = window.WebSocket;
        window.WebSocket = function(url){
            var ws = new origWS(url);
            ws.addEventListener('message', function(event){
                try {
                    var data = JSON.parse(event.data);
                    var d = data.type||'?';
                    if(data.phase) d += ' phase='+data.phase;
                    if(data.lastHandResult) d += ' [SHOWDOWN]';
                    if(data.players) d += ' count='+data.players.length;
                    if(data.isYourTurn!==undefined) d += ' yourTurn='+data.isYourTurn;
                    if(data.error) d += ' ERR='+data.message;
                    addEntry('network', '← '+d, 'WS',0,0,'', data.error?data.message:(data.yourCards?'has cards':''));
                } catch(e){}
            });
            var origSend = ws.send;
            ws.send = function(data){
                try {
                    var p = JSON.parse(data);
                    addEntry('network', '→ '+(p.type||'raw'), 'WS',0,0,'', p.action?p.action+(p.amount?' $'+p.amount:''):'');
                } catch(e){}
                return origSend.call(this, data);
            };
            return ws;
        };
        window.WebSocket.prototype = origWS.prototype;
        window.WebSocket.CONNECTING = origWS.CONNECTING;
        window.WebSocket.OPEN = origWS.OPEN;
        window.WebSocket.CLOSING = origWS.CLOSING;
        window.WebSocket.CLOSED = origWS.CLOSED;
    })();

    // 3. Game state monitor
    (function(){
        var _orig = null;
        Object.defineProperty(window, 'onGameState', {
            set: function(fn){ _orig = fn; },
            get: function(){ return function(msg){
                if(msg && msg.phase){
                    var d = 'phase='+msg.phase+' pot=$'+(msg.pot||0);
                    if(msg.communityCards) d += ' community='+msg.communityCards.length;
                    if(msg.lastHandResult) d += ' [HAS_RESULT]';
                    addEntry('game', 'game_state: '+d, 'net',0,0,'', msg.lastHandResult?'winners='+(msg.lastHandResult.winners||[]).map(function(w){return w.name;}).join(','):'');
                }
                if(_orig) _orig(msg);
            };},
            configurable: true
        });
    })();

    // 4. Heartbeat
    (function(){
        var lastBeat = Date.now();
        setInterval(function(){ lastBeat = Date.now(); }, 200);
        setInterval(function(){
            var e = Date.now() - lastBeat;
            if(e > 3000) addEntry('error', '⚠️ 页面可能卡死 — 停滞 '+Math.round(e/1000)+'s', 'monitor');
        }, 1500);
    })();

    // 5. console.log capture
    (function(){
        var origLog = console.log;
        console.log = function(){
            var args = Array.prototype.slice.call(arguments);
            var text = args.map(function(a){
                try { return typeof a === 'object' ? JSON.stringify(a) : String(a); } catch(e){ return String(a); }
            }).join(' ');
            if(text.length > 200) text = text.slice(0,200)+'...';
            addEntry('console', text, 'console');
            origLog.apply(console, arguments);
        };
    })();

    // 6. Init
    if(document.readyState === 'loading') document.addEventListener('DOMContentLoaded', createPanel);
    else createPanel();
    console.log('[Debug] 增强版监控已启动');
})();
