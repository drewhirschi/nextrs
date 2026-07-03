#!/usr/bin/env python3
"""Master builder for the nextrs mark. Tune constants, run, render, diff."""
import math

VB = 1024
CX = CY = 512.0

# ---- gear ----
N_TEETH   = 32
R_TIP     = 453.0
R_VALLEY  = 407.0
R_RING_IN = 338.0
SAMP      = 48

# ---- inner ring ----
INNER_R   = 338.0
INNER_SW  = 14.0

# ---- bolt holes ----
BOLT_R = 25.0
BOLTS = [(512,141),(879,494),(146,494),(353,860),(673,860)]

# ---- equilateral triangle (filled, centred in gear) ----
TRI_CX, TRI_CY = 512.0, 512.0   # centre (== gear centre)
TRI_R    = 312.0                # circumradius (vertex distance from centre)
TRI_ROUND = 0.0                 # corner rounding radius (0 = sharp)

# ---- R glyph ----
STEM_L, STEM_R = 360.0, 464.0
R_TOP, R_BOT   = 352.0, 768.0
SERIF_X, SERIF_H = 36.0, 40.0
BOWL_R   = 676.0
BOWL_BOT = 558.0        # y of bowl underside at stem
LEG_THK  = 64.0         # leg thickness (vertical)
LEG_FOOT = 668.0        # x centre of leg foot
LEG_SERIF= 40.0
CL,CR,CT,CB = 464.0, 584.0, 460.0, 540.0   # counter
MOAT = 3.0

def smoothstep(t):
    t=max(0.,min(1.,t)); return t*t*(3-2*t)

def radial(theta):
    # smooth rounded humps: cosine with slight tip/valley flattening
    c = math.cos(N_TEETH*theta)           # +1 at tip, -1 at valley
    s = math.copysign(smoothstep(abs(c)), c)  # gentle flatten
    mid=(R_TIP+R_VALLEY)/2; amp=(R_TIP-R_VALLEY)/2
    return mid + amp*s

def gear_path():
    pts=[]; total=N_TEETH*SAMP
    for i in range(total):
        th=2*math.pi*i/total; r=radial(th)
        pts.append((CX+r*math.cos(th),CY+r*math.sin(th)))
    d="M %.2f %.2f "%pts[0]+" ".join("L %.2f %.2f"%p for p in pts[1:])+" Z"
    r=R_RING_IN
    d+=" M %.2f %.2f A %.2f %.2f 0 1 0 %.2f %.2f A %.2f %.2f 0 1 0 %.2f %.2f Z"%(
        CX+r,CY,r,r,CX-r,CY,r,r,CX+r,CY)
    return d

def tri_path():
    verts=[(TRI_CX+TRI_R*math.cos(math.radians(a)),
            TRI_CY+TRI_R*math.sin(math.radians(a))) for a in (-90,30,150)]
    r=TRI_ROUND
    if r<=0:
        return "M %.2f %.2f L %.2f %.2f L %.2f %.2f Z"%(
            verts[0][0],verts[0][1],verts[1][0],verts[1][1],verts[2][0],verts[2][1])
    cmds=[]
    for i in range(3):
        p0=verts[(i-1)%3]; p1=verts[i]; p2=verts[(i+1)%3]
        v0=(p0[0]-p1[0],p0[1]-p1[1]); l0=math.hypot(*v0); u0=(v0[0]/l0,v0[1]/l0)
        v2=(p2[0]-p1[0],p2[1]-p1[1]); l2=math.hypot(*v2); u2=(v2[0]/l2,v2[1]/l2)
        a=(p1[0]+u0[0]*r,p1[1]+u0[1]*r); b=(p1[0]+u2[0]*r,p1[1]+u2[1]*r)
        cmds.append(("M %.2f %.2f" if i==0 else "L %.2f %.2f")%a)
        cmds.append("Q %.2f %.2f %.2f %.2f"%(p1[0],p1[1],b[0],b[1]))
    cmds.append("Z")
    return " ".join(cmds)

def r_outer():
    SH   = 618.0                    # shoulder x (top edge -> curve)
    BMID = (R_TOP+BOWL_BOT)/2.0     # bowl right-side mid
    d =f"M {STEM_L-SERIF_X:.1f} {R_TOP:.1f} "
    d+=f"L {SH:.1f} {R_TOP:.1f} "
    d+=f"Q {BOWL_R:.1f} {R_TOP:.1f} {BOWL_R:.1f} {BMID:.1f} "        # elliptical upper-right
    d+=f"Q {BOWL_R:.1f} {BOWL_BOT:.1f} {SH-6:.1f} {BOWL_BOT:.1f} "  # elliptical lower-right
    d+=f"L {STEM_R+14:.1f} {BOWL_BOT:.1f} "                 # underside to stem waist
    # leg: diagonal limb from waist to bottom-right foot
    d+=f"L {LEG_FOOT+LEG_SERIF:.1f} {R_BOT-SERIF_H:.1f} "   # leg outer edge to foot
    d+=f"L {LEG_FOOT+LEG_SERIF:.1f} {R_BOT:.1f} "           # foot right serif
    d+=f"L {LEG_FOOT-LEG_SERIF:.1f} {R_BOT:.1f} "           # foot bottom
    d+=f"L {LEG_FOOT-LEG_SERIF:.1f} {R_BOT-SERIF_H:.1f} "   # foot inner up
    d+=f"L {STEM_R:.1f} {BOWL_BOT+LEG_THK:.1f} "            # leg inner back to stem
    d+=f"L {STEM_R:.1f} {R_BOT-SERIF_H:.1f} "               # stem right down to foot
    d+=f"L {STEM_R+SERIF_X:.1f} {R_BOT-SERIF_H:.1f} "       # stem foot right serif
    d+=f"L {STEM_R+SERIF_X:.1f} {R_BOT:.1f} "
    d+=f"L {STEM_L-SERIF_X:.1f} {R_BOT:.1f} "
    d+=f"L {STEM_L-SERIF_X:.1f} {R_BOT-SERIF_H:.1f} "
    d+=f"L {STEM_L:.1f} {R_BOT-SERIF_H:.1f} "
    d+=f"L {STEM_L:.1f} {R_TOP+SERIF_H:.1f} "               # up stem left
    d+=f"L {STEM_L-SERIF_X:.1f} {R_TOP+SERIF_H:.1f} Z"
    return d

def counter():
    # D-shape: flat left against stem, rounded right
    my=(CT+CB)/2.0
    d =f"M {CL:.1f} {CT:.1f} L {CL:.1f} {CB:.1f} L {CR-58:.1f} {CB:.1f} "
    d+=f"Q {CR:.1f} {CB:.1f} {CR:.1f} {my:.1f} "
    d+=f"Q {CR:.1f} {CT:.1f} {CR-58:.1f} {CT:.1f} Z"
    return d

FG   = "#111111"   # mark colour
PAD  = 40          # viewBox padding around content

def _box():
    x0 = CX - R_TIP - PAD; span = 2*(R_TIP+PAD)
    return x0, span

def mask_body():
    """Inner elements of the luminance mask (white = mark, black = knocked out).
    Identical for every colour/background, so all outputs share one geometry."""
    x0, span = _box()
    m  = f'<rect x="{x0:.0f}" y="{x0:.0f}" width="{span:.0f}" height="{span:.0f}" fill="black"/>'
    m += f'<path d="{gear_path()}" fill="white" fill-rule="evenodd"/>'
    m += f'<circle cx="{CX:.0f}" cy="{CY:.0f}" r="{INNER_R:.0f}" fill="none" stroke="white" stroke-width="{INNER_SW:.0f}"/>'
    m += f'<path d="{tri_path()}" fill="white"/>'
    for bx,by in BOLTS:
        m += f'<circle cx="{bx:.0f}" cy="{by:.0f}" r="{BOLT_R:.0f}" fill="black"/>'
    return m

def svg(color=FG, bg=None):
    """Transparent-safe mark. `color` fills the mark; `bg` optionally paints a backdrop."""
    x0, span = _box()
    body = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x0:.0f} {x0:.0f} {span:.0f} {span:.0f}">']
    body.append(f'<mask id="cut">{mask_body()}</mask>')
    if bg:
        body.append(f'<rect x="{x0:.0f}" y="{x0:.0f}" width="{span:.0f}" height="{span:.0f}" fill="{bg}"/>')
    body.append(f'<rect x="{x0:.0f}" y="{x0:.0f}" width="{span:.0f}" height="{span:.0f}" fill="{color}" mask="url(#cut)"/>')
    body.append('</svg>')
    return "\n".join(body)

def favicon_svg(light="#1A1820", dark="#ECEAE4"):
    """Adaptive favicon: dark ink on light UAs, paper on dark UAs (prefers-color-scheme)."""
    x0, span = _box()
    style = (f'<style>.m{{fill:{light}}}'
             f'@media(prefers-color-scheme:dark){{.m{{fill:{dark}}}}}</style>')
    return ("\n".join([
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x0:.0f} {x0:.0f} {span:.0f} {span:.0f}">',
        style,
        f'<mask id="cut">{mask_body()}</mask>',
        f'<rect class="m" x="{x0:.0f}" y="{x0:.0f}" width="{span:.0f}" height="{span:.0f}" mask="url(#cut)"/>',
        '</svg>']))

def react_component():
    """Emit a self-contained React (TSX) icon component. currentColor + size/className."""
    x0, span = _box()
    vb = f"{x0:.0f} {x0:.0f} {span:.0f} {span:.0f}"
    bolts = ", ".join(f"[{bx:.0f}, {by:.0f}]" for bx,by in BOLTS)
    return f'''// AUTO-GENERATED by scripts/gen-mark.py — do not edit by hand.
// Regenerate with:  python3 scripts/gen-mark.py react > client/src/NextrsMark.tsx
//
// The nextrs brand mark (equilateral triangle in a chainring gear) as an icon
// component. Monochrome: it paints in `currentColor`, so set the colour with
// CSS (`style={{{{ color }}}}`) or Tailwind (`text-*`), and the size with the
// `size` prop or width/height utilities (`w-8 h-8`, `size-8`).
import * as React from "react";

const VIEW_BOX = "{vb}";
const BOX = {{ x: {x0:.0f}, s: {span:.0f} }};
const INNER = {{ r: {INNER_R:.0f}, sw: {INNER_SW:.0f} }};
const BOLT_R = {BOLT_R:.0f};
const BOLTS: Array<[number, number]> = [{bolts}];
const GEAR_D =
  "{gear_path()}";
const TRI_D =
  "{tri_path()}";

export interface NextrsMarkProps extends React.SVGProps<SVGSVGElement> {{
  /** Rendered width & height. Number → px. Defaults to `"1em"` so it follows font-size. */
  size?: number | string;
  /** Accessible label. Omit for a decorative mark (aria-hidden). */
  title?: string;
}}

export const NextrsMark = React.forwardRef<SVGSVGElement, NextrsMarkProps>(
  function NextrsMark({{ size = "1em", title, ...props }}, ref) {{
    const uid = React.useId().replace(/:/g, "");
    const maskId = `nextrs-mark-${{uid}}`;
    return (
      <svg
        ref={{ref}}
        xmlns="http://www.w3.org/2000/svg"
        viewBox={{VIEW_BOX}}
        width={{size}}
        height={{size}}
        fill="currentColor"
        role={{title ? "img" : undefined}}
        aria-hidden={{title ? undefined : true}}
        {{...props}}
      >
        {{title ? <title>{{title}}</title> : null}}
        <mask id={{maskId}}>
          <rect x={{BOX.x}} y={{BOX.x}} width={{BOX.s}} height={{BOX.s}} fill="black" />
          <path d={{GEAR_D}} fill="white" fillRule="evenodd" />
          <circle cx="{CX:.0f}" cy="{CY:.0f}" r={{INNER.r}} fill="none" stroke="white" strokeWidth={{INNER.sw}} />
          <path d={{TRI_D}} fill="white" />
          {{BOLTS.map(([cx, cy], i) => (
            <circle key={{i}} cx={{cx}} cy={{cy}} r={{BOLT_R}} fill="black" />
          ))}}
        </mask>
        <rect x={{BOX.x}} y={{BOX.x}} width={{BOX.s}} height={{BOX.s}} mask={{`url(#${{maskId}})`}} />
      </svg>
    );
  }}
);

export default NextrsMark;
'''

if __name__=="__main__":
    import sys
    arg = sys.argv[1] if len(sys.argv)>1 else FG
    if arg == "react":
        sys.stdout.write(react_component())
    elif arg == "favicon":
        light = sys.argv[2] if len(sys.argv)>2 else "#1A1820"
        dark  = sys.argv[3] if len(sys.argv)>3 else "#ECEAE4"
        print(favicon_svg(light, dark))
    else:
        color = arg
        bg    = sys.argv[2] if len(sys.argv)>2 else None
        print(svg(color, bg))
