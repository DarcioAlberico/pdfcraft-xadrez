# Coerência mecânica do plano A9 (escrito na R10; desde a v11 a ficha §3.4 é a fonte das dependências).
# Extrai §3.0 (núcleo), §3.1 (grafo), §3.2 (ondas), §3.3 (passos) e §3.4 (ficha) e confere
# uns contra os outros. Uso: py -3.12 -I coerencia.py <plano.md>
import re
import sys
from collections import defaultdict, OrderedDict

path = sys.argv[1]
lines = open(path, encoding="utf-8").read().split("\n")


def section(start_pat, end_pat):
    s = next(i for i, l in enumerate(lines) if re.match(start_pat, l))
    e = next(i for i in range(s + 1, len(lines)) if re.match(end_pat, lines[i]))
    return s, e


ID = r"(?:A9-)?(?:[QTGFLVXINE]\d{1,2}[a-c]?|P)"
ID_RE = re.compile(r"(?<![A-Za-z0-9-])(" + ID + r")(?![A-Za-z0-9])")


def norm(i):
    i = i.strip()
    if i.startswith("A9-"):
        i = i[3:]
    return i


# ---------------------------------------------------------------- §3.3 passos
s33, e33 = section(r"^### 3\.3 ", r"^### 3\.4 ")
steps = OrderedDict()  # id -> (line, text)
cur = None
for i in range(s33, e33):
    l = lines[i]
    m = re.match(r"^\*\*A9-([A-Z]\d{1,2}[a-c]?)(?:\*\*)? ?[—-]", l) or re.match(r"^\*\*A9-([A-Z]\d{1,2}[a-c]?) ", l)
    if m:
        cur = m.group(1)
        steps[cur] = [i + 1, l]
    elif cur and l.strip() and not l.startswith("####"):
        steps[cur][1] += "\n" + l
    elif l.startswith("####"):
        cur = None

# sub-ids declared in each step: bold ids **Q1a** or A9-L0a
declared = {}
for sid, (ln, txt) in steps.items():
    subs = set()
    for m in re.finditer(r"\*\*(?:A9-)?([A-Z]\d{1,2}[a-c])\*\*", txt):
        if m.group(1).startswith(sid):
            subs.add(m.group(1))
    declared[sid] = sorted(subs)

# ---------------------------------------------------------------- §3.4 ficha
s34, e34 = section(r"^### 3\.4 ", r"^## 4\. ")
ficha = OrderedDict()  # id -> dict(line, wave, lane, deps, prs, effort, fmt)
fich_rows = []
for i in range(s34, e34):
    l = lines[i]
    if not l.startswith("| ") or l.startswith("| Passo") or l.startswith("|---"):
        continue
    cells = [c.strip() for c in l.strip().strip("|").split("|")]
    if len(cells) != 5:
        print("FICHA: linha com", len(cells), "células:", i + 1)
        continue
    passo, onda, dep, prs, fmt = cells
    fich_rows.append((i + 1, cells))
    ids = [norm(x) for x in re.split(r" · ", re.sub(r"\(com [^)]*\)", "", passo)) if x.strip()]
    if passo.startswith("N11") or passo.startswith("E0"):
        continue
    n = len(ids)
    ot = [x.strip() for x in onda.split(" · ")]
    if len(ot) == 2 * n:
        ow = [(ot[2 * k], ot[2 * k + 1]) for k in range(n)]
    elif len(ot) == 2:
        ow = [(ot[0], ot[1])] * n
    else:
        print("FICHA: onda ambígua", i + 1, onda)
        ow = [(ot[0], ot[-1])] * n
    dt = [x.strip() for x in dep.split(" · ")]
    if len(dt) == n:
        dw = dt
    elif len(dt) == 1:
        dw = dt * n
    else:
        print("FICHA: depende ambíguo", i + 1, dep)
        dw = (dt + [dt[-1]] * n)[:n]
    pt = [x.strip() for x in prs.split(" · ")]
    if len(pt) == 2 * n:
        pw = [(pt[2 * k], pt[2 * k + 1]) for k in range(n)]
    elif len(pt) == 2 and "cada" in pt[1]:
        pw = [(pt[0], pt[1])] * n
    elif len(pt) == 2:
        # a total shared by the ids: first id carries it
        pw = [(pt[0], pt[1])] + [("0", pt[1])] * (n - 1)
    else:
        print("FICHA: PRs ambíguo", i + 1, prs)
        pw = [(pt[0], pt[-1])] * n
    for k, sid in enumerate(ids):
        deps = set()
        for m in ID_RE.finditer(dw[k]):
            deps.add(norm(m.group(1)))
        dtext = dw[k]
        ficha[sid] = dict(line=i + 1, wave=ow[k][0], lane=ow[k][1], deps=deps, dtext=dtext, prs=pw[k][0], effort=pw[k][1], fmt=fmt)

# ---------------------------------------------------------------- §3.2 ondas
s32, e32 = section(r"^### 3\.2 ", r"^\*\*Caminho crítico")
waves = OrderedDict()  # id -> (wave, lane, position, prs_hint)
wave_rows = {}
for i in range(s32, e32):
    l = lines[i]
    m = re.match(r"^\| \*\*(P|\d)\*\*", l)
    if not m:
        continue
    w = m.group(1)
    cells = [c.strip() for c in l.strip().strip("|").split("|")]
    wave_rows[w] = (i + 1, cells)
    for lane, cell in zip("ABC", cells[1:4]):
        # the sequence is the bold part (before the first " · " list of files)
        bolds = re.findall(r"\*\*(.+?)\*\*", cell)
        seq = " → ".join(bolds)
        if w == "6":
            seq = cell  # N2 → N9 → N10** → N11 ...
        if w == "P":
            if "Passo 0" in cell:
                waves["P"] = (w, lane, 0, 2 if "(2)" in cell else 1)
            continue
        pos = 0
        for part in re.split(r"→", seq):
            part = part.strip()
            hint = 2 if re.search(r"\(2\)", part) else 1
            # "L7 (+ G7b)", "G3c + V4b", "I15 (com I7a)", "G11 (motor)"
            for m2 in ID_RE.finditer(part):
                sid = norm(m2.group(1))
                if sid in ("N11", "N12", "N15", "N18"):
                    continue
                key = sid
                if key in waves and key == "G11":
                    key = "G11#ui"
                if key in waves and key == "G2b":
                    key = "G2b#ui"
                if key in waves and key == "I5":
                    key = "I5#2"
                waves[key] = (w, lane, pos, hint)
            pos += 1

# ---------------------------------------------------------------- §3.1 grafo
s31, e31 = section(r"^### 3\.1 ", r"^### 3\.2 ")
code = []
inside = False
for i in range(s31, e31):
    if lines[i].startswith("```"):
        inside = not inside
        continue
    if inside:
        code.append((i + 1, lines[i]))

edges = set()  # (before, after) = after depends on before
nodes = set()


def ids_in(s):
    return [norm(m.group(1)) for m in ID_RE.finditer(s)]


def split_top(s, sep):
    out, depth, cur = [], 0, ""
    for ch in s:
        if ch in "{(":
            depth += 1
        elif ch in "})":
            depth -= 1
        if ch == sep and depth == 0:
            out.append(cur)
            cur = ""
        else:
            cur += ch
    out.append(cur)
    return out


def parse_chain(s):
    """Returns (heads, tails) of the chain and adds the edges inside it."""
    s = s.strip()
    segs = [x.strip() for x in re.split(r"─►", s)]
    # re-join segments split inside braces
    joined, buf, depth = [], "", 0
    for seg in segs:
        buf = seg if not buf else buf + " ─► " + seg
        depth = buf.count("{") - buf.count("}")
        if depth == 0:
            joined.append(buf)
            buf = ""
    if buf:
        joined.append(buf)
    prev_tails = None
    first_heads = None
    for seg in joined:
        seg = seg.strip()
        if seg.startswith("{") and seg.endswith("}"):
            heads, tails = [], []
            for el in split_top(seg[1:-1], ","):
                h, t = parse_chain(el)
                heads += h
                tails += t
        else:
            # strip parentheses with no ids; keep "(+ G7b)" as companion; drop "(com X)"
            companions = []
            for par in re.findall(r"\(([^)]*)\)", seg):
                if par.startswith("+"):
                    companions += ids_in(par)
            seg2 = re.sub(r"\([^)]*\)", "", seg)
            heads = ids_in(seg2) + companions
            tails = heads[:]
        for n_ in heads + tails:
            nodes.add(n_)
        if prev_tails is not None:
            for a in prev_tails:
                for b in heads:
                    edges.add((a, b))
        else:
            first_heads = heads
        prev_tails = tails
    return first_heads or [], prev_tails or []


for ln, l in code:
    t = l.strip()
    if not t or t.startswith("Faixa ") or t == "Onda N":
        continue
    if t.startswith("N1 (") or " · N2 (" in t:
        for item in t.split(" · "):
            m = re.match(r"(\S+) \(([^)]*)\)(.*)", item.strip())
            if not m:
                continue
            node = norm(m.group(1))
            nodes.add(node)
            for d in ids_in(m.group(2)):
                edges.add((d, node))
            if "─►" in m.group(3):
                for nxt in ids_in(m.group(3)):
                    edges.add((node, nxt))
                    nodes.add(nxt)
        continue
    for stmt in [x for part in split_top(t, ";") for x in re.split(r"\s{3,}", part)]:
        stmt = stmt.strip()
        if not stmt:
            continue
        stmt = stmt.replace("(todo o resto)", "")
        parse_chain(stmt)

# "(todo o resto)": Q1a is before everything
graph_deps = defaultdict(set)
for a, b in edges:
    graph_deps[b].add(a)

# ---------------------------------------------------------------- §3.0 núcleo
s30, e30 = section(r"^### 3\.0 ", r"^### 3\.1 ")
core = []
for i in range(s30, e30):
    m = re.match(r"^\| (\d+) \| (.+?) \| (.+?) \| ([\d,]+) · ([\d,]+) \|", lines[i])
    if m:
        core.append((int(m.group(1)), m.group(2), m.group(3), int(m.group(4)), float(m.group(5).replace(",", "."))))

# ================================================================ relatório
out = []
P = out.append
P("# Saída do roteiro de coerência (R10)\n")
P(f"Passos de §3.3 (cabeçalhos): {len(steps)}")
P("Sub-ids declarados em negrito por passo: " + "; ".join(f"{k}: {','.join(v)}" for k, v in declared.items() if v))

step_units = set()
for sid in steps:
    step_units.add(sid)
    step_units.update(declared[sid])
ficha_ids = set(ficha)
wave_ids = {k.split("#")[0] for k in waves}

P("\n## 1. Conjuntos")
# units: a step with sub-ids is represented by its sub-ids in the ficha/waves
expanded = set()
for sid in steps:
    if declared[sid]:
        expanded.update(declared[sid])
    else:
        expanded.add(sid)
expanded.add("P")
missing_ficha = sorted(expanded - ficha_ids)
extra_ficha = sorted(ficha_ids - expanded)
P(f"Na §3.3 (expandido por sub-id) e sem linha na ficha: {missing_ficha}")
P(f"Na ficha e sem passo/sub-id declarado na §3.3: {extra_ficha}")
P(f"Na ficha e fora da tabela de ondas: {sorted(ficha_ids - wave_ids)}")
P(f"Na tabela de ondas e fora da ficha: {sorted(wave_ids - ficha_ids)}")
P(f"No grafo e fora da ficha: {sorted(nodes - ficha_ids)}")
P(f"Na ficha e fora do grafo (sem aresta nenhuma): {sorted(ficha_ids - nodes)}")

P("\n## 2. Onda da ficha × onda da tabela §3.2")
for sid, f in ficha.items():
    if sid not in waves:
        continue
    w, lane, pos, hint = waves[sid]
    fw = f["wave"]
    fl = f["lane"].replace(" ", "")
    lane_ok = lane in fl or (sid + "#ui" in waves and waves[sid + "#ui"][1] in fl)
    if fw != w or not lane_ok:
        extra = f" e {waves[sid + '#ui']}" if sid + "#ui" in waves else ""
        P(f"- {sid}: ficha onda {fw} faixa {f['lane']} (l. {f['line']}) × §3.2 onda {w} faixa {lane}{extra}")

P("\n## 3. Dependências da ficha × arestas do grafo")
for sid, f in ficha.items():
    fd = {d for d in f["deps"] if d != sid}
    gd = graph_deps.get(sid, set())
    # Q1a is before everything ("todo o resto")
    fd2 = fd - {"Q1a", "P"}
    gd2 = gd - {"Q1a", "P"}
    only_f = sorted(fd2 - gd2)
    only_g = sorted(gd2 - fd2)
    if only_f or only_g:
        P(f"- {sid} (ficha l. {f['line']}): só na ficha {only_f} · só no grafo {only_g}")

P("\n## 4. Dependência numa onda posterior, ou na mesma onda noutra faixa / mais adiante")
order = {"P": -1, "0": 0, "1": 1, "2": 2, "3": 3, "4": 4, "5": 5, "6": 6}


def wave_of(x):
    if x in waves:
        return waves[x]
    return None


for sid, f in ficha.items():
    me = wave_of(sid)
    if not me:
        continue
    for d in sorted(f["deps"] | graph_deps.get(sid, set())):
        if d.startswith("D") or d == sid:
            continue
        dw = wave_of(d)
        if not dw:
            continue
        wm, wd = order[me[0]], order[dw[0]]
        if wd > wm:
            P(f"- {sid} (onda {me[0]}, faixa {me[1]}) depende de {d} (onda {dw[0]}, faixa {dw[1]}) — POSTERIOR")
        elif wd == wm:
            if dw[1] != me[1]:
                # a second slot (G11#ui) may be in the same lane
                alt = waves.get(d + "#ui")
                if alt and alt[1] == me[1] and alt[2] < me[2]:
                    continue
                P(f"- {sid} (onda {me[0]}, faixa {me[1]}, pos {me[2]}) depende de {d} (mesma onda, faixa {dw[1]}, pos {dw[2]}) — mesma onda, outra faixa")
            elif dw[2] >= me[2]:
                P(f"- {sid} depende de {d}, mesma onda e faixa, mas {d} vem depois (pos {dw[2]} ≥ {me[2]})")

P("\n## 5. Ciclos no grafo")
adj = defaultdict(set)
for a, b in edges:
    adj[a].add(b)
color = {}
cycles = []


def dfs(u, stack):
    color[u] = 1
    stack.append(u)
    for v in adj[u]:
        if color.get(v) == 1:
            cycles.append(stack[stack.index(v):] + [v])
        elif not color.get(v):
            dfs(v, stack)
    stack.pop()
    color[u] = 2


for n_ in sorted(nodes):
    if not color.get(n_):
        dfs(n_, [])
P(f"Ciclos: {cycles if cycles else 'nenhum'}; nós {len(nodes)}, arestas {len(edges)}")

P("\n## 6. Soma de PRs")
tot = 0
detail = []
for sid, f in ficha.items():
    m = re.match(r"(\d+)", f["prs"])
    n_ = int(m.group(1)) if m else 0
    if f["prs"] == "(no G3c)":
        n_ = 0
    tot += n_
    detail.append(f"{sid}={n_}")
P(f"Soma da coluna PRs da ficha (sem N11–N18): {tot}")
P("Por passo: " + " ".join(detail))
wtot = 0
for k, (w, lane, pos, hint) in waves.items():
    wtot += hint
P(f"Soma pela tabela de ondas (1 por id, 2 com '(2)'): {wtot}")

P("\n## 7. Núcleo (§3.0)")
P(f"Linhas: {len(core)}; PRs {sum(c[3] for c in core)}; dias {sum(c[4] for c in core)}")
for c in core:
    ids = ids_in(c[1])
    fprs = 0
    for x in ids:
        if x in ficha:
            m = re.match(r"(\d+)", ficha[x]["prs"])
            fprs += int(m.group(1)) if m else 0
        elif x == "F2":
            fprs += sum(int(re.match(r"(\d+)", ficha[y]["prs"]).group(1)) for y in ("F2a", "F2b") if y in ficha) // (2 if ficha.get("F2a", {}).get("prs") == ficha.get("F2b", {}).get("prs") and "F2a" in ficha and ficha["F2a"]["prs"] == "2" else 1)
    P(f"- #{c[0]} {c[1]}: ids {ids}; PRs no núcleo {c[3]}; ondas {[waves.get(x, ('?',))[0] for x in ids]}")

# perdas: §5 lista; núcleo "Fecha"
s5 = next(i for i, l in enumerate(lines) if l.startswith("| Perdas e resultados errados"))
loss = re.findall(r"[AR]\d-\d\d", lines[s5])
P(f"\nItens da métrica de perdas (§5 l. {s5 + 1}): {len(loss)} → {loss}")
closed = []
for c in core:
    closed += [x for x in re.findall(r"[AR]\d-\d\d", c[2]) if x in loss]
P(f"Fechados pelo núcleo (coluna Fecha ∩ métrica): {len(set(closed))} → {sorted(set(closed))}")
s30txt = "\n".join(lines[s30:e30])
after = re.search(r"Logo depois \(11–14\):\*\*(.+?)\*\*Pode esperar", s30txt, re.S)
if after:
    later = [x for x in re.findall(r"[AR]\d-\d\d", after.group(1)) if x in loss]
    P(f"Fechados por 11–14: {sorted(set(later))}; total {len(set(closed) | set(later))}")
    P(f"Restam: {sorted(set(loss) - set(closed) - set(later))}")

# owner (§1.2 'Passo' column) of each loss item vs the core step that claims it
s12, e12 = section(r"^## Anexo A", r"^## Anexo B")
owner = {}
for i in range(s12, e12):
    l = lines[i]
    if not l.startswith("| ") or l.startswith("| Id") or l.startswith("|---"):
        continue
    cells = [c.strip() for c in l.strip().strip("|").split("|")]
    for x in re.findall(r"[AR]\d-\d\d", cells[0]):
        owner.setdefault(x, (i + 1, cells[-1]))
P("\nDono (§1.2, coluna Passo) dos itens que o núcleo diz fechar:")
for c in core:
    for x in re.findall(r"[AR]\d-\d\d", c[2]):
        o = owner.get(x)
        P(f"- núcleo #{c[0]} ({c[1]}) fecha {x}: §1.2 l. {o[0] if o else '?'} diz passo '{o[1] if o else '—'}'")

# step header 'onda N' vs ficha
P("\n## 8. 'onda N' escrito no cabeçalho do passo × ficha")
for sid, (ln, txt) in steps.items():
    head = txt.split("\n")[0]
    for m in re.finditer(r"\(onda (\d)", head):
        f = ficha.get(sid) or ficha.get(sid + "a")
        if f and f["wave"] != m.group(1):
            P(f"- {sid} l. {ln}: cabeçalho diz onda {m.group(1)}; ficha {f['wave']}")
    for m in re.finditer(r"\*\*([A-Z]\d{1,2}[a-c])\*\* \(onda (\d)", txt):
        f = ficha.get(m.group(1))
        if f and f["wave"] != m.group(2):
            P(f"- {m.group(1)} (texto do passo {sid}, l. {ln}): diz onda {m.group(2)}; ficha {f['wave']}")

# 'Depende de' in the step text vs ficha
P("\n## 9. 'Depende de' no texto do passo × ficha")
for sid, (ln, txt) in steps.items():
    for m in re.finditer(r"Depende de ([^.;]+)", txt):
        ds = set(ids_in(m.group(1)))
        units = declared[sid] or [sid]
        fdeps = set()
        for u in units:
            if u in ficha:
                fdeps |= ficha[u]["deps"]
        # a step dependency may name the parent (I0) for sub-ids (I0c)
        miss = [d for d in ds if not any(fd == d or fd.startswith(d) for fd in fdeps)]
        if miss:
            P(f"- {sid} l. {ln}: texto 'Depende de {m.group(1).strip()[:60]}' → não está na ficha: {miss} (ficha: {sorted(fdeps)})")

P("\n## 10. Dias por faixa e onda (mín.–máx.), pela regra do §3.2 (S 0,5 · M 1–1,5 · L 2–3)")
EFF = {"S": (0.5, 0.5), "S–M": (0.5, 1.5), "M": (1.0, 1.5), "M–L": (1.0, 3.0), "L": (2.0, 3.0)}


def eff_days(sid):
    f = ficha.get(sid)
    if not f:
        return (0, 0)
    e = f["effort"].replace(" cada", "").strip()
    m = re.match(r"(\d+)", f["prs"])
    n_ = int(m.group(1)) if m else 0
    if "+" in e:  # "S + M"
        a, b = [EFF.get(x.strip(), (1, 1)) for x in e.split("+")]
        return (a[0] + b[0], a[1] + b[1])
    lo, hi = EFF.get(e, (1, 1))
    return (lo * max(n_, 1), hi * max(n_, 1)) if n_ else (0, 0)


lane_days = defaultdict(lambda: [0.0, 0.0])
for k, (w, lane, pos, hint) in waves.items():
    sid = k.split("#")[0]
    if k.endswith("#ui") or k.endswith("#2"):
        lo, hi = (0.5, 0.75)  # the second PR of a step split across lanes
    else:
        lo, hi = eff_days(sid)
        if sid + "#ui" in waves:  # half here, half in the other lane
            lo, hi = lo / 2, hi / 2
    lane_days[(w, lane)][0] += lo
    lane_days[(w, lane)][1] += hi
tot_lo = tot_hi = 0
for w in ["0", "1", "2", "3", "4", "5", "6"]:
    row = wave_rows.get(w)
    plan_days = row[1][4] if row else "?"
    vals = {l: lane_days.get((w, l), [0, 0]) for l in "ABC"}
    mx = max(vals.values(), key=lambda v: v[1])
    P(f"- onda {w}: A {vals['A'][0]:.1f}–{vals['A'][1]:.1f} · B {vals['B'][0]:.1f}–{vals['B'][1]:.1f} · C {vals['C'][0]:.1f}–{vals['C'][1]:.1f} · plano diz {plan_days}")
for l in "ABC":
    lo = sum(v[0] for (w, ll), v in lane_days.items() if ll == l)
    hi = sum(v[1] for (w, ll), v in lane_days.items() if ll == l)
    P(f"- faixa {l} no total: {lo:.1f}–{hi:.1f} dias")
allo = sum(v[0] for v in lane_days.values())
allhi = sum(v[1] for v in lane_days.values())
P(f"- todas as faixas: {allo:.1f}–{allhi:.1f} dias de agente")

open(sys.argv[2], "w", encoding="utf-8").write("\n".join(out) + "\n")
print("\n".join(out))
