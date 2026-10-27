"""Write the hand-authored, self-contained week 1 pages (stdlib only).

Run only when updating the suite, with the unmodified font described in NOTICE:
python tools/author-fixtures.py .upstream/Roboto-Regular.ttf
"""
import base64
import json
import sys
from pathlib import Path

font = base64.b64encode(Path(sys.argv[1]).read_bytes()).decode()
base = """*{box-sizing:border-box}html{background:#fff}body{margin:0;padding:20px;
font:16px/1.5 Arial,sans-serif;color:#172033}h1,h2,h3,p{margin:0 0 12px}
h1{font-size:26px;line-height:1.2}h2{font-size:20px}h3{font-size:17px}
.panel{padding:20px;border:1px solid #ccd5e0;border-radius:12px;background:#f4f7fb}
.muted{color:#52647a}.row{display:flex;align-items:center;gap:12px}
.badge{display:inline-block;padding:3px 10px;border-radius:20px;background:#dceafa;color:#183d72;font-size:12px}
"""
# Each entry is independently authored markup and CSS; no application data.
# Probes associate measured differences with features without asserting causality.
pages = {
    "01-card": ("Card", ".panel{box-shadow:0 6px 16px #b7c7d8}",
        '<article id="card" class="panel"><div class="badge">FIELD NOTES</div><h1>Walk the ridge</h1><p>A short route with wide views and quiet places to rest.</p><div class="muted">4 km · Easy pace</div></article>',
        [("card", "box-shadow"), ("card", "border-radius"), ("card", "padding")]),
    "02-list-rows": ("List rows", '.item{padding:16px 0;border-bottom:1px solid #ccd5e0}.dot{width:32px;height:32px;flex:none;background:#cde6df;border-radius:50%}.copy{flex:1}',
        '<main id="list"><h1>Today</h1><div class="row item"><div class="dot"></div><div class="copy">Morning walk</div><div class="muted">08:00</div></div><div class="row item"><div class="dot"></div><div class="copy">Read a chapter</div><div class="muted">12:30</div></div><div class="row item"><div class="dot"></div><div class="copy">Water the plants</div><div class="muted">18:00</div></div></main>',
        [("list", "flex layout"), ("list", "borders")]),
    "03-header": ("Header", 'header{justify-content:space-between;padding:12px;border-bottom:2px solid #4169a1}nav{display:flex;gap:18px}a{color:#244c82;text-decoration:none}',
        '<header id="header" class="row"><strong>Trail journal</strong><nav><a href="#">Map</a><a href="#">Notes</a></nav></header><main class="panel" style="margin-top:24px"><h1>Welcome outside</h1><p>Choose a route for the afternoon.</p></main>',
        [("header", "flex layout"), ("header", "inline text")]),
    "04-badges": ("Badges", '.row{flex-wrap:wrap}.warm{background:#ffe9c6}.green{background:#d6eee3}.outline{border:1px solid #7997bc;background:white}',
        '<h1>Route labels</h1><div id="badges" class="row"><span class="badge">Open</span><span class="badge warm">Steep</span><span class="badge green">Woodland</span><span class="badge outline">Loop route</span><span class="badge">Family friendly</span></div>',
        [("badges", "flex wrap"), ("badges", "border-radius"), ("badges", "inline text")]),
    "05-progress": ("Progress", '.track{height:16px;border-radius:8px;background:#dbe4ef;overflow:hidden}.fill{height:100%;width:65%;background:linear-gradient(90deg,#3267b2,#50b2a5)}',
        '<section id="progress" class="panel"><h2>Weekly distance</h2><p>13 of 20 km</p><div class="track"><div class="fill"></div></div><div class="muted" style="margin-top:12px">65% complete</div></section>',
        [("progress", "linear-gradient"), ("progress", "overflow clipping"), ("progress", "percentage sizing")]),
    "06-stat-tile": ("Stat tile", '.number{font-size:48px;line-height:1;font-weight:bold;letter-spacing:-2px}.units{font-size:16px;letter-spacing:0}.trend{color:#246d52;margin-top:20px}',
        '<article id="stat" class="panel"><p class="muted">Distance this month</p><div class="number">128<span class="units"> km</span></div><div class="trend">↑ 12% from last month</div></article>',
        [("stat", "letter-spacing"), ("stat", "line-height"), ("stat", "inline text")]),
    "07-table": ("Table", 'table{width:100%;border-collapse:collapse;font-size:14px}th,td{text-align:left;padding:12px 8px;border-bottom:1px solid #ccd5e0}th{background:#e7eef7}td:last-child{text-align:right}',
        '<h1>Trail summary</h1><table id="table"><thead><tr><th>Route</th><th>Grade</th><th>Length</th></tr></thead><tbody><tr><td>River bend</td><td>Easy</td><td>3 km</td></tr><tr><td>North ridge</td><td>Medium</td><td>8 km</td></tr><tr><td>Lake circuit</td><td>Easy</td><td>5 km</td></tr></tbody></table>',
        [("table", "table layout"), ("table", "border-collapse")]),
    "08-form-layout": ("Form-like layout", '.fields{display:grid;grid-template-columns:1fr 1fr;gap:16px}.field{min-width:0}.wide{grid-column:1/-1}.label{font-size:12px;color:#52647a;margin-bottom:4px}.value{border:1px solid #aab9cc;border-radius:6px;padding:10px;background:white}',
        '<section id="form" class="panel"><h1>Route details</h1><div class="fields"><div class="field wide"><div class="label">Name</div><div class="value">River bend</div></div><div class="field"><div class="label">Distance</div><div class="value">3 km</div></div><div class="field"><div class="label">Duration</div><div class="value">45 min</div></div><div class="field wide"><div class="label">Notes</div><div class="value">Follow the water to the old footbridge.</div></div></div></section>',
        [("form", "grid layout"), ("form", "borders"), ("form", "padding")]),
    "09-grid-gallery": ("Grid gallery", '.gallery{display:grid;grid-template-columns:repeat(2,1fr);gap:12px}.tile{height:110px;border-radius:10px;padding:12px;background:linear-gradient(145deg,#b9d6e8,#dee9cf)}.tile:nth-child(2){background:linear-gradient(45deg,#f7d2a2,#dcd4ec)}.tile:nth-child(3){grid-column:span 2;height:130px}',
        '<h1>Places to wander</h1><div id="gallery" class="gallery"><div class="tile">River</div><div class="tile">Meadow</div><div class="tile">Hilltop</div><div class="tile">Woods</div><div class="tile">Garden</div></div>',
        [("gallery", "grid layout"), ("gallery", "linear-gradient"), ("gallery", "border-radius")]),
    "10-nested-blocks": ("Nested blocks", '.outer{background:#dce8f5;padding:18px;border:3px solid #5c7b9f}.middle{background:#f2e6cf;padding:14px;margin:12px 8px}.inner{background:#d0e8db;padding:10px;border:2px solid #428267}',
        '<h1>Nested spaces</h1><div id="nested" class="outer">Outer box<div class="middle">Middle box<div class="inner">Inner box with wrapping text that continues onto another line.</div></div></div>',
        [("nested", "nested blocks"), ("nested", "margins"), ("nested", "padding"), ("nested", "borders")]),
    "11-margin-collapse": ("Margin collapse", 'section{background:#edf3fa}p{margin:24px 0}h2{margin:32px 0 16px}.anchor{height:12px;background:#4169a1}',
        '<div class="anchor"></div><section id="margins"><h2>Morning notes</h2><p>The path is dry.</p><p>Clouds move across the valley.</p></section><div class="anchor"></div>',
        [("margins", "margin collapse"), ("margins", "nested blocks")]),
    "12-borders-radius": ("Borders and radius", '.sample{height:92px;margin-bottom:18px;padding:20px;background:#e5eff9}.a{border:4px solid #4169a1;border-radius:24px 4px}.b{border-top:6px solid #af6b39;border-right:2px solid #4169a1;border-bottom:3px dashed #55795c;border-left:8px solid #55795c;border-radius:12px}',
        '<h1>Box edges</h1><div id="edges"><div class="sample a">Asymmetric corners</div><div class="sample b">Mixed border edges</div></div>',
        [("edges", "borders"), ("edges", "border-radius")]),
    "13-shadows": ("Shadows", '.sample{margin:24px 12px;padding:24px;border-radius:12px;background:#f0f5fa}.a{box-shadow:0 8px 16px 2px #9aabc2}.b{box-shadow:inset 0 2px 8px #879db7,0 2px 3px #ccd5e0}',
        '<h1>Depth samples</h1><section id="shadows"><div class="sample a">Soft outside shadow</div><div class="sample b">Inset and outside shadow</div></section>',
        [("shadows", "box-shadow"), ("shadows", "border-radius")]),
    "14-gradients": ("Gradients", '.swatch{height:100px;padding:16px;margin-bottom:16px;border-radius:8px}.a{background:linear-gradient(90deg,#a9c8eb 0%,#f3dcaa 70%,#dcaab3 100%)}.b{background:linear-gradient(135deg,#c8e6d7,#bfd1ed)}',
        '<h1>Color fields</h1><section id="gradients"><div class="swatch a">Three color stops</div><div class="swatch b">Diagonal blend</div></section>',
        [("gradients", "linear-gradient"), ("gradients", "border-radius")]),
    "15-embedded-font": ("Embedded font", f'@font-face{{font-family:FixtureRoboto;src:url(data:font/ttf;base64,{font}) format("truetype");font-weight:400;font-style:normal}}.web{{font-family:FixtureRoboto,serif}}.fallback{{font-family:MissingFixtureFont,Arial,sans-serif}}',
        '<section id="fonts" class="panel"><h1 class="web" style="font-weight:400">Across the valley</h1><p id="webfont" class="web">A quiet trail winds between the pines. 0123456789</p><p class="fallback">System fallback: A quiet trail winds between the pines.</p></section>',
        [("webfont", "embedded web font"), ("fonts", "system fallback")]),
    "16-inline-svg": ("Inline SVG", 'svg{flex:none}.icon-row{padding:16px;border:1px solid #ccd5e0;margin-bottom:12px}',
        '<h1>Route symbols</h1><section id="icons"><div class="row icon-row"><svg id="svg" xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32"><circle cx="16" cy="16" r="13" fill="#d4e8fa"/><path d="M8 17l5 5L24 10" fill="none" stroke="#24558b" stroke-width="3"/></svg><div>Route checked</div></div><div class="row icon-row"><svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32"><path d="M3 26L13 7l6 11 4-7 7 15z" fill="#53856b"/></svg><div>Hill walk</div></div></section>',
        [("icons", "inline SVG"), ("icons", "flex layout")]),
    "17-text-spacing": ("Text spacing", '.tight{line-height:1.1;letter-spacing:-.4px}.airy{line-height:2;letter-spacing:2px}.sample{padding:12px;background:#edf3fa;margin-bottom:20px}',
        '<h1>Reading rhythm</h1><section id="spacing"><p class="sample tight">A narrow path follows the curve of the hillside, with distant views over the water and the low fields.</p><p class="sample airy">A narrow path follows the curve of the hillside, with distant views over the water.</p></section>',
        [("spacing", "line-height"), ("spacing", "letter-spacing"), ("spacing", "text wrapping")]),
    "18-ellipsis": ("Text overflow", '.truncate{width:240px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;padding:10px;border:1px solid #ccd5e0}.clip{text-overflow:clip;margin-top:16px}',
        '<h1>Long route names</h1><section id="overflow"><div id="ellipsis" class="truncate">The very long woodland and riverside circular route</div><div class="truncate clip">The very long woodland and riverside circular route</div></section>',
        [("ellipsis", "text-overflow ellipsis"), ("overflow", "overflow clipping")]),
    "19-flex-alignment": ("Flex alignment", '.stack{display:flex;flex-direction:column;gap:16px}.band{display:flex;justify-content:space-between;align-items:center;padding:12px;background:#edf3fa;gap:8px}.box{background:#b9d1ec;padding:8px}.tall{height:70px}.grow{flex:1;min-width:0}',
        '<h1>Aligned bands</h1><section id="flex" class="stack"><div class="band"><div class="box">One</div><div class="box tall">Two</div><div class="box">Three</div></div><div class="band"><div class="box">Fixed</div><div class="box grow">Flexible copy wraps within the available space.</div></div></section>',
        [("flex", "flex layout"), ("flex", "gap"), ("flex", "text wrapping")]),
    "20-dashboard": ("Dashboard", '.dashboard{display:grid;grid-template-columns:1fr 1fr;gap:14px}.wide{grid-column:span 2}.value{font-size:28px;font-weight:bold}.mini{padding:16px}.bar{height:8px;border-radius:4px;background:linear-gradient(90deg,#4b7cb7 75%,#e0e8f1 75%)}',
        '<header class="row" style="justify-content:space-between;margin-bottom:24px"><h1 style="margin:0">Walking log</h1><span class="badge">This week</span></header><main id="dashboard" class="dashboard"><article class="panel mini"><div class="muted">Distance</div><div class="value">24 km</div></article><article class="panel mini"><div class="muted">Routes</div><div class="value">6</div></article><article class="panel wide"><h2>Weekly goal</h2><p>Three quarters of the way there.</p><div class="bar"></div></article><article class="panel wide"><h2>Next route</h2><p>River bend loop</p><div class="muted">Saturday · 09:00 · 3 km</div></article></main>',
        [("dashboard", "grid layout"), ("dashboard", "linear-gradient"), ("dashboard", "border-radius"), ("dashboard", "padding")]),
    "21-long-page": ("Long page", '.block{height:280px;padding:20px;margin-bottom:16px;border:2px solid #7293b8;background:#e8f1fa}',
        '<h1>Long route journal</h1><main id="long"><section class="block">Start by the bridge</section><section class="block">Climb through the woods</section><section class="block">Pause at the ridge</section><section class="block">Return beside the river</section></main>',
        [("long", "full-page height"), ("long", "margins")]),
}
manifest = {}
for name, (title, css, body, probes) in pages.items():
    Path(f"fixtures/{name}.html").write_text(
        '<!doctype html>\n<!-- Copyright 2026 g1BSsQ. Apache-2.0. -->\n'
        f'<html lang="en"><head><meta charset="utf-8"><title>{title}</title>\n'
        f'<style>\n{base}{css}\n</style></head><body>\n{body}\n</body></html>\n', encoding="utf-8")
    manifest[name] = {"title": title, "probes": [{"id": id, "feature": feature} for id, feature in probes]}
Path("fixtures/manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
print(f"Authored {len(pages)} self-contained pages")
