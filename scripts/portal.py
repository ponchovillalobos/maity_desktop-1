"""
Maity Desktop — Portal de Asamblea de Expertos + Dashboard de Mejora Continua

Lanzar:
    python scripts/portal.py
    # o
    uvicorn scripts.portal:app --port 8770 --reload

Endpoints:
    /              Portal unificado (tabs)
    /assembly      Asamblea: cards, scatter Impact/Effort, tabs por experto, roadmap
    /dashboard     Métricas: tests, warnings, builds, iteración actual
    /memory        Render de memory/*.md (improvement log, failed attempts, metrics, state)
    /api/findings  JSON crudo del assembly_data.json
    /health        OK
"""
from __future__ import annotations

import json
from pathlib import Path

import markdown as md
from fastapi import FastAPI, HTTPException
from fastapi.responses import HTMLResponse, JSONResponse, PlainTextResponse

ROOT = Path(__file__).resolve().parent.parent
ASSEMBLY_FILE = ROOT / "scripts" / "assembly_data.json"
MEMORY_DIR = ROOT / "memory"

app = FastAPI(title="Maity Desktop — Portal", version="1.0")


# ──────────────────────────── helpers ──────────────────────────── #
def load_assembly() -> dict:
    if not ASSEMBLY_FILE.exists():
        raise HTTPException(500, f"assembly_data.json not found at {ASSEMBLY_FILE}")
    with ASSEMBLY_FILE.open(encoding="utf-8") as f:
        return json.load(f)


SEVERITY_COLORS = {
    "critical": "#e74c3c",
    "high": "#e67e22",
    "medium": "#f1c40f",
    "low": "#95a5a6",
}


# ──────────────────────────── routes ──────────────────────────── #
@app.get("/health")
async def health():
    return {"ok": True, "assembly": ASSEMBLY_FILE.exists()}


@app.get("/api/findings")
async def api_findings():
    return JSONResponse(load_assembly())


@app.get("/", response_class=HTMLResponse)
async def portal():
    return f"""<!doctype html>
<html lang="es">
<head>
<meta charset="utf-8">
<title>Maity Desktop — Portal</title>
<style>
  :root {{ --bg:#0f1115; --fg:#e6e9ef; --accent:#27ae60; --muted:#7f8c8d; --card:#1a1d24; }}
  * {{ box-sizing:border-box; }}
  body {{ margin:0; font-family:-apple-system,Segoe UI,Roboto,sans-serif; background:var(--bg); color:var(--fg); }}
  header {{ padding:14px 22px; background:#161922; border-bottom:1px solid #2a2f3a; display:flex; align-items:center; gap:18px; }}
  header h1 {{ margin:0; font-size:18px; font-weight:600; }}
  header .tag {{ font-size:11px; padding:2px 8px; border-radius:10px; background:var(--accent); color:#fff; }}
  nav button {{ background:transparent; border:1px solid #2a2f3a; color:var(--fg); padding:6px 14px; margin-right:6px; border-radius:6px; cursor:pointer; font-size:13px; }}
  nav button:hover, nav button.active {{ background:#2a2f3a; }}
  iframe {{ width:100%; height:calc(100vh - 58px); border:0; background:var(--bg); }}
</style>
</head>
<body>
<header>
  <h1>🎙️ Maity Desktop</h1>
  <span class="tag">Asamblea v1.0</span>
  <nav>
    <button onclick="load('/assembly', this)" class="active">Asamblea</button>
    <button onclick="load('/dashboard', this)">Dashboard</button>
    <button onclick="load('/memory', this)">Memoria</button>
  </nav>
</header>
<iframe id="frame" src="/assembly"></iframe>
<script>
function load(url, btn) {{
  document.getElementById('frame').src = url;
  document.querySelectorAll('nav button').forEach(b => b.classList.remove('active'));
  btn.classList.add('active');
}}
</script>
</body>
</html>"""


@app.get("/assembly", response_class=HTMLResponse)
async def assembly():
    data = load_assembly()
    experts = data["experts"]
    project = data["project"]

    all_findings = []
    for exp_key, exp in experts.items():
        for f in exp["findings"]:
            all_findings.append({**f, "expert": exp_key, "expert_name": exp["name"], "icon": exp["icon"]})

    total = len(all_findings)
    done = sum(1 for f in all_findings if f["status"] == "done")
    pending = total - done
    critical = sum(1 for f in all_findings if f["severity"] == "critical")
    progress_pct = int((done / total) * 100) if total else 0

    # Tabs por experto
    tabs_html = ""
    panels_html = ""
    for i, (key, exp) in enumerate(experts.items()):
        active = " active" if i == 0 else ""
        tabs_html += f'<button class="tab{active}" onclick="showTab(\'{key}\', this)">{exp["icon"]} {exp["name"]} <span class="badge">{len(exp["findings"])}</span></button>\n'
        rows = ""
        for f in sorted(exp["findings"], key=lambda x: -(x["impact"] / max(x["effort"], 1))):
            sev_color = SEVERITY_COLORS.get(f["severity"], "#95a5a6")
            status_icon = "✅" if f["status"] == "done" else "⏳"
            rows += f"""
            <tr>
              <td><code>{f['id']}</code></td>
              <td>{status_icon} {f['title']}</td>
              <td><span class="sev" style="background:{sev_color}">{f['severity']}</span></td>
              <td>{f['impact']}</td>
              <td>{f['effort']}</td>
              <td>{f['phase']}</td>
            </tr>
            <tr class="detail"><td colspan="6">
              <p><strong>Problema:</strong> {f['description']}</p>
              <p><strong>Solución:</strong> {f['recommendation']}</p>
            </td></tr>"""
        panels_html += f"""
        <div class="panel" id="panel-{key}" style="display:{'block' if i == 0 else 'none'}">
          <p class="summary">{exp.get('summary', '')}</p>
          <table>
            <thead><tr><th>ID</th><th>Título</th><th>Sev</th><th>I</th><th>E</th><th>Phase</th></tr></thead>
            <tbody>{rows}</tbody>
          </table>
        </div>"""

    # Roadmap
    roadmap = data.get("roadmap", {})
    roadmap_html = ""
    for phase_key in ("v1.0", "v2.0", "v3.0"):
        if phase_key not in roadmap:
            continue
        phase = roadmap[phase_key]
        items = phase.get("items", [])
        items_html = ""
        for fid in items:
            f = next((x for x in all_findings if x["id"] == fid), None)
            if not f:
                continue
            sev_color = SEVERITY_COLORS.get(f["severity"], "#95a5a6")
            done_class = " done" if f["status"] == "done" else ""
            items_html += f'<li class="rm-item{done_class}"><span class="rm-id">{f["id"]}</span> {f["title"]} <span class="rm-sev" style="background:{sev_color}"></span></li>'
        roadmap_html += f"""
        <div class="rm-col">
          <h3>{phase_key} <small>{phase.get('label', '')}</small></h3>
          <ul>{items_html}</ul>
        </div>"""

    # Scatter chart data
    scatter_points = json.dumps([
        {"x": f["effort"], "y": f["impact"], "id": f["id"], "title": f["title"][:50],
         "color": SEVERITY_COLORS.get(f["severity"], "#95a5a6")}
        for f in all_findings
    ])

    return HTMLResponse(f"""<!doctype html>
<html lang="es">
<head>
<meta charset="utf-8">
<title>Asamblea — {project['name']}</title>
<script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.0/dist/chart.umd.min.js"></script>
<style>
  :root {{ --bg:#0f1115; --fg:#e6e9ef; --card:#1a1d24; --muted:#7f8c8d; --accent:#27ae60; --border:#2a2f3a; }}
  * {{ box-sizing:border-box; }}
  body {{ margin:0; padding:20px; font-family:-apple-system,Segoe UI,Roboto,sans-serif; background:var(--bg); color:var(--fg); }}
  h2, h3 {{ margin:0 0 8px; font-weight:600; }}
  .grid {{ display:grid; grid-template-columns:repeat(4, 1fr); gap:14px; margin-bottom:18px; }}
  .card {{ background:var(--card); border:1px solid var(--border); border-radius:10px; padding:18px; }}
  .card .num {{ font-size:32px; font-weight:700; }}
  .card .lbl {{ font-size:12px; color:var(--muted); text-transform:uppercase; letter-spacing:0.5px; }}
  .progress {{ background:var(--card); border:1px solid var(--border); border-radius:8px; height:24px; overflow:hidden; margin-bottom:18px; position:relative; }}
  .progress-bar {{ background:linear-gradient(90deg,#27ae60,#2ecc71); height:100%; transition:width 0.3s; }}
  .progress-text {{ position:absolute; inset:0; display:flex; align-items:center; justify-content:center; font-size:12px; font-weight:600; }}
  .uvp {{ background:var(--card); border-left:3px solid var(--accent); padding:12px 16px; border-radius:6px; margin-bottom:18px; font-size:13px; color:#bdc3c7; }}
  .tabs {{ display:flex; flex-wrap:wrap; gap:6px; margin-bottom:14px; }}
  .tab {{ background:transparent; border:1px solid var(--border); color:var(--fg); padding:8px 14px; border-radius:6px; cursor:pointer; font-size:13px; }}
  .tab:hover {{ background:#2a2f3a; }}
  .tab.active {{ background:var(--accent); border-color:var(--accent); color:#fff; }}
  .tab .badge {{ background:rgba(255,255,255,0.15); padding:1px 6px; border-radius:10px; font-size:11px; margin-left:4px; }}
  .panel {{ background:var(--card); border:1px solid var(--border); border-radius:10px; padding:18px; margin-bottom:18px; }}
  .summary {{ color:var(--muted); margin:0 0 14px; font-size:13px; font-style:italic; }}
  table {{ width:100%; border-collapse:collapse; font-size:13px; }}
  th {{ text-align:left; padding:8px; border-bottom:1px solid var(--border); color:var(--muted); font-weight:500; font-size:11px; text-transform:uppercase; }}
  td {{ padding:10px 8px; border-bottom:1px solid var(--border); }}
  tr.detail td {{ padding:8px 16px 16px; color:#bdc3c7; font-size:12px; background:rgba(255,255,255,0.02); }}
  tr.detail p {{ margin:4px 0; }}
  .sev {{ display:inline-block; padding:2px 8px; border-radius:10px; font-size:10px; font-weight:600; color:#fff; text-transform:uppercase; }}
  code {{ background:#0a0c10; padding:2px 6px; border-radius:4px; font-size:11px; color:#3498db; }}
  .roadmap {{ display:grid; grid-template-columns:1fr 1fr 1fr; gap:14px; margin-top:18px; }}
  .rm-col {{ background:var(--card); border:1px solid var(--border); border-radius:10px; padding:16px; }}
  .rm-col h3 small {{ color:var(--muted); font-size:11px; font-weight:400; display:block; margin-top:2px; }}
  .rm-col ul {{ list-style:none; padding:0; margin:12px 0 0; }}
  .rm-item {{ padding:8px 0; border-bottom:1px solid var(--border); font-size:12px; display:flex; align-items:center; gap:8px; }}
  .rm-item.done {{ opacity:0.5; text-decoration:line-through; }}
  .rm-id {{ background:#0a0c10; padding:2px 6px; border-radius:3px; font-size:10px; color:#3498db; font-family:monospace; }}
  .rm-sev {{ width:8px; height:8px; border-radius:50%; margin-left:auto; }}
  .chart-wrap {{ background:var(--card); border:1px solid var(--border); border-radius:10px; padding:18px; margin-bottom:18px; height:380px; }}
</style>
</head>
<body>

<h2>🎙️ Asamblea de Expertos — {project['name']} v{project['version']}</h2>
<div class="uvp">{project.get('unique_value', '')}</div>

<div class="grid">
  <div class="card"><div class="num">{total}</div><div class="lbl">Total Hallazgos</div></div>
  <div class="card"><div class="num" style="color:#2ecc71">{done}</div><div class="lbl">Completados</div></div>
  <div class="card"><div class="num" style="color:#f39c12">{pending}</div><div class="lbl">Pendientes</div></div>
  <div class="card"><div class="num" style="color:#e74c3c">{critical}</div><div class="lbl">Críticos</div></div>
</div>

<div class="progress">
  <div class="progress-bar" style="width:{progress_pct}%"></div>
  <div class="progress-text">{progress_pct}% completado ({done}/{total})</div>
</div>

<div class="chart-wrap">
  <h3>📊 Impact vs Effort — priorizar los del cuadrante superior izquierdo</h3>
  <canvas id="scatter"></canvas>
</div>

<h3>👥 Expertos</h3>
<div class="tabs">{tabs_html}</div>
{panels_html}

<h3>🗺️ Roadmap</h3>
<div class="roadmap">{roadmap_html}</div>

<script>
function showTab(key, btn) {{
  document.querySelectorAll('.panel').forEach(p => p.style.display = 'none');
  document.getElementById('panel-' + key).style.display = 'block';
  document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
  btn.classList.add('active');
}}

const points = {scatter_points};
new Chart(document.getElementById('scatter'), {{
  type: 'scatter',
  data: {{
    datasets: [{{
      label: 'Hallazgos',
      data: points,
      backgroundColor: points.map(p => p.color),
      pointRadius: 7,
      pointHoverRadius: 10,
    }}]
  }},
  options: {{
    responsive: true, maintainAspectRatio: false,
    scales: {{
      x: {{ title: {{ display:true, text:'Effort (1=fácil, 10=difícil)', color:'#bdc3c7' }}, min:0, max:11, ticks:{{color:'#7f8c8d'}}, grid:{{color:'#2a2f3a'}} }},
      y: {{ title: {{ display:true, text:'Impact (1=bajo, 10=alto)', color:'#bdc3c7' }}, min:0, max:11, ticks:{{color:'#7f8c8d'}}, grid:{{color:'#2a2f3a'}} }}
    }},
    plugins: {{
      legend: {{ display:false }},
      tooltip: {{ callbacks: {{ label: ctx => ctx.raw.id + ': ' + ctx.raw.title }} }}
    }}
  }}
}});
</script>
</body>
</html>""")


@app.get("/dashboard", response_class=HTMLResponse)
async def dashboard():
    metrics_file = MEMORY_DIR / "METRICS_HISTORY.md"
    log_file = MEMORY_DIR / "IMPROVEMENT_LOG.md"
    metrics_md = metrics_file.read_text(encoding="utf-8") if metrics_file.exists() else "_no metrics yet_"
    log_md = log_file.read_text(encoding="utf-8") if log_file.exists() else "_no improvements yet_"

    data = load_assembly()
    iterations = data.get("iterations", 0)
    commits = data.get("commits", 0)
    cycle = data.get("evaluation_cycle", 1)

    return HTMLResponse(f"""<!doctype html>
<html lang="es"><head><meta charset="utf-8"><title>Dashboard</title>
<style>
  body {{ margin:0; padding:20px; font-family:-apple-system,Segoe UI,sans-serif; background:#0f1115; color:#e6e9ef; }}
  h2, h3 {{ margin:0 0 12px; }}
  .grid {{ display:grid; grid-template-columns:repeat(3, 1fr); gap:14px; margin-bottom:18px; }}
  .card {{ background:#1a1d24; border:1px solid #2a2f3a; border-radius:10px; padding:18px; }}
  .num {{ font-size:32px; font-weight:700; }}
  .lbl {{ font-size:12px; color:#7f8c8d; text-transform:uppercase; }}
  .panel {{ background:#1a1d24; border:1px solid #2a2f3a; border-radius:10px; padding:18px; margin-bottom:14px; }}
  pre {{ background:#0a0c10; padding:14px; border-radius:6px; overflow:auto; font-size:12px; }}
</style></head>
<body>
<h2>📈 Dashboard de Mejora Continua</h2>
<div class="grid">
  <div class="card"><div class="num">{cycle}</div><div class="lbl">Ciclo de evaluación</div></div>
  <div class="card"><div class="num">{iterations}</div><div class="lbl">Iteraciones</div></div>
  <div class="card"><div class="num">{commits}</div><div class="lbl">Commits asamblea</div></div>
</div>
<div class="panel"><h3>📊 Métricas históricas</h3>{md.markdown(metrics_md, extensions=['tables'])}</div>
<div class="panel"><h3>📝 Log de mejoras</h3>{md.markdown(log_md, extensions=['tables'])}</div>
</body></html>""")


@app.get("/memory", response_class=HTMLResponse)
async def memory_view():
    files = ["IMPROVEMENT_LOG.md", "FAILED_ATTEMPTS.md", "METRICS_HISTORY.md", "ANALYSIS_STATE.md"]
    sections = ""
    for fname in files:
        fpath = MEMORY_DIR / fname
        body = fpath.read_text(encoding="utf-8") if fpath.exists() else "_(missing)_"
        sections += f"""
        <div class="panel">
          <h3>📄 {fname}</h3>
          {md.markdown(body, extensions=['tables', 'fenced_code'])}
        </div>"""

    return HTMLResponse(f"""<!doctype html>
<html lang="es"><head><meta charset="utf-8"><title>Memory</title>
<style>
  body {{ margin:0; padding:20px; font-family:-apple-system,Segoe UI,sans-serif; background:#0f1115; color:#e6e9ef; }}
  h2, h3 {{ margin:0 0 12px; }}
  .panel {{ background:#1a1d24; border:1px solid #2a2f3a; border-radius:10px; padding:18px; margin-bottom:14px; }}
  pre, code {{ background:#0a0c10; padding:6px; border-radius:4px; font-size:12px; }}
  table {{ border-collapse:collapse; }}
  th, td {{ border:1px solid #2a2f3a; padding:6px 10px; }}
</style></head>
<body>
<h2>🧠 Memoria del Sistema</h2>
{sections}
</body></html>""")


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host="127.0.0.1", port=8770)
