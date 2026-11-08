"""Build the FrameWise Lock-On F logo masters (symbol, small cut, wordmark, lockups).

Run:  python build_kit.py <out-dir>
Everything is constructed geometry: no fonts, no live text.
"""
import os
import sys

OUT = sys.argv[1] if len(sys.argv) > 1 else "."
os.makedirs(OUT, exist_ok=True)

# ---- colour tokens (from the Framewise design doc) ----
VIOLET, VIOLET_D = "#6D4AFF", "#8B73FF"
CYAN, CYAN_D = "#0891B2", "#22D3EE"
INK, PAPER = "#14151A", "#F2F3F7"
LIGHT = dict(frame=VIOLET, dot=CYAN, text=INK)
DARK = dict(frame=VIOLET_D, dot=CYAN_D, text=PAPER)


def n(v):
    return f"{v:.2f}".rstrip("0").rstrip(".")


# ---- symbol: Lock-On F -------------------------------------------------
def symbol_paths(t=28, R=16, lo=36, hi=220, arm=76, mid_end=96, dot=26, dot_r=6):
    """Reticle of frame corners. Left side is an F whose middle arm aims at the centre dot.
    Returns (frame_path, dot_path) in a 256 canvas."""
    c = (lo + hi) / 2                      # 128
    gap_hi = lo + arm                      # end of top-left / top-right arms
    # F: stem + top arm (rounded outer corner) + middle arm centred on c
    my0, my1 = c - t / 2, c + t / 2
    F = (f"M{n(lo)} {n(hi)}V{n(lo+R)}A{R} {R} 0 0 1 {n(lo+R)} {n(lo)}H{n(gap_hi)}V{n(lo+t)}"
         f"H{n(lo+t)}V{n(my0)}H{n(mid_end)}V{n(my1)}H{n(lo+t)}V{n(hi)}Z")
    x0 = hi - arm                          # start of right-hand arms
    TR = (f"M{n(x0)} {n(lo)}H{n(hi-R)}A{R} {R} 0 0 1 {n(hi)} {n(lo+R)}V{n(gap_hi)}"
          f"H{n(hi-t)}V{n(lo+t)}H{n(x0)}Z")
    BR = (f"M{n(x0)} {n(hi)}V{n(hi-t)}H{n(hi-t)}V{n(x0)}H{n(hi)}V{n(hi-R)}"
          f"A{R} {R} 0 0 1 {n(hi-R)} {n(hi)}Z")
    d0, d1 = c - dot / 2, c + dot / 2
    r = dot_r
    D = (f"M{n(d0+r)} {n(d0)}H{n(d1-r)}A{r} {r} 0 0 1 {n(d1)} {n(d0+r)}V{n(d1-r)}"
         f"A{r} {r} 0 0 1 {n(d1-r)} {n(d1)}H{n(d0+r)}A{r} {r} 0 0 1 {n(d0)} {n(d1-r)}"
         f"V{n(d0+r)}A{r} {r} 0 0 1 {n(d0+r)} {n(d0)}Z")
    return F + TR + BR, D


MAIN = symbol_paths()
# small-size cut (≤ 32 px): heavier strokes, bigger gaps, bigger target
SMALL = symbol_paths(t=40, R=10, lo=28, hi=228, arm=84, mid_end=90, dot=30, dot_r=5)


# ---- wordmark: custom squared letters, cap 160 (y 40–200), x-height 120, stroke 26 ----
def L_F(o):
    return f"M{o} 200V52a12 12 0 0 1 12-12h88v26h-74v42h62v26h-62v66z", 100


def L_r(o):
    return f"M{o} 200V110a30 30 0 0 1 30-30h46v26h-44a6 6 0 0 0-6 6v88z", 76


def L_a(o):
    return (f"M{o+30} 80h66v120h-66a30 30 0 0 1-30-30v-60a30 30 0 0 1 30-30z"
            f"M{o+32} 106h38v68h-38a6 6 0 0 1-6-6v-56a6 6 0 0 1 6-6z"), 96


def L_m(o):
    return (f"M{o} 200V110a30 30 0 0 1 30-30h90a30 30 0 0 1 30 30v90h-26v-88a6 6 0 0 0-6-6h-24"
            f"a6 6 0 0 0-6 6v88h-26v-88a6 6 0 0 0-6-6h-24a6 6 0 0 0-6 6v88z"), 150


def L_e(o):
    return (f"M{o} 170V110a30 30 0 0 1 30-30h36a30 30 0 0 1 30 30v42h-70v16a6 6 0 0 0 6 6h64v26h-66"
            f"a30 30 0 0 1-30-30z"
            f"M{o+32} 106h32a6 6 0 0 1 6 6v14h-44v-14a6 6 0 0 1 6-6z"), 96


def L_W(o):
    return (f"M{o} 40h26v128a6 6 0 0 0 6 6h37a6 6 0 0 0 6-6v-88h26v88a6 6 0 0 0 6 6h37a6 6 0 0 0 6-6"
            f"v-128h26v130a30 30 0 0 1-30 30h-116a30 30 0 0 1-30-30z"), 176


def L_i(o):
    return f"M{o} 80h26v120h-26z", 26


def L_i_dot(o):
    return f"M{o+6} 40h14a6 6 0 0 1 6 6v14a6 6 0 0 1-6 6h-14a6 6 0 0 1-6-6v-14a6 6 0 0 1 6-6z"


def L_s(o):
    return (f"M{o+30} 80h66v26h-64a6 6 0 0 0-6 6v9a6 6 0 0 0 6 6h34a30 30 0 0 1 30 30v13"
            f"a30 30 0 0 1-30 30h-66v-26h64a6 6 0 0 0 6-6v-9a6 6 0 0 0-6-6h-34a30 30 0 0 1-30-30"
            f"v-13a30 30 0 0 1 30-30z"), 96


# (glyph, space before) — optical spacing
SEQ = [(L_F, 0), (L_r, 16), (L_a, 14), (L_m, 22), (L_e, 20), (L_W, 22), (L_i, 22), (L_s, 22), (L_e, 20)]


def wordmark():
    x, body, dot = 0, [], ""
    for fn, sp in SEQ:
        x += sp
        d, w = fn(x)
        body.append(d)
        if fn is L_i:
            dot = L_i_dot(x)
        x += w
    return "".join(body), dot, x            # letters, i-dot, total width


WM, WM_DOT, WM_W = wordmark()               # cap box y 40..200, x 0..WM_W


# ---- writers ----
def write(name, vb, inner, title):
    with open(os.path.join(OUT, name), "w", encoding="utf-8") as fh:
        fh.write(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}">\n'
                 f"  <title>{title}</title>\n{inner}</svg>\n")


def sym_g(paths, pal, tf=""):
    fr, dt = paths
    t = f' transform="{tf}"' if tf else ""
    return (f'  <g{t}>\n    <path fill="{pal["frame"]}" d="{fr}"/>\n'
            f'    <path fill="{pal["dot"]}" d="{dt}"/>\n  </g>\n')


def wm_g(pal, tf):
    return (f'  <g transform="{tf}">\n    <path fill="{pal["text"]}" fill-rule="evenodd" d="{WM}"/>\n'
            f'    <path fill="{pal["dot"]}" d="{WM_DOT}"/>\n  </g>\n')


for suffix, pal in (("", LIGHT), ("-on-dark", DARK)):
    write(f"framewise-symbol{suffix}.svg", "0 0 256 256", sym_g(MAIN, pal), "FrameWise logo")
    write(f"framewise-symbol-small{suffix}.svg", "0 0 256 256", sym_g(SMALL, pal), "FrameWise logo")

    # wordmark only (24-unit margin)
    write(f"framewise-wordmark{suffix}.svg", f"-24 16 {WM_W+48} 208",
          wm_g(pal, "translate(0 0)"), "FrameWise")

    # horizontal lockup: cap height = 60 % of the symbol's 184-unit body, centred on it
    s = 0.6 * 184 / 160
    cap_top = 128 - 160 * s / 2
    gx = 220 + 52                            # gap ≈ 2 strokes after symbol
    w = round(gx + WM_W * s + 36)
    write(f"framewise-horizontal{suffix}.svg", f"0 0 {n(w)} 256",
          sym_g(MAIN, pal) + wm_g(pal, f"translate({n(gx)} {n(cap_top - 40*s)}) scale({n(s)})"),
          "FrameWise logo")

    # stacked lockup: symbol above, wordmark width ≈ 2.9 × symbol body
    s2 = 0.5
    ww = WM_W * s2
    W = ww + 72
    sx = W / 2 - 128
    wy = 220 + 44                            # cap top
    H = wy + 160 * s2 + 36
    write(f"framewise-stacked{suffix}.svg", f"0 0 {n(W)} {n(H)}",
          sym_g(MAIN, pal, f"translate({n(sx)} 0)") +
          wm_g(pal, f"translate(36 {n(wy - 40*s2)}) scale({s2})"),
          "FrameWise logo")

print("wordmark width", WM_W)
