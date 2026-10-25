# netrace — UX analysis of the interface prototype

Reviewed 25 Oct 2026 against the build of the full interface (DESIGN.md), using
`examples/campus.netrace` (14 devices, 12 cables) in light and dark mode at
1280×800 and 1800×1073. The target user is a CCNA student: they build a
topology, cable it with specific ports, configure devices in a CLI, test with
pings in simulation mode, and follow lab instructions.

Severity: 🔴 blocks or seriously slows a core student task · 🟡 friction or
confusion · 🟢 polish.

## Status

Priorities 1–4 were implemented on 25 Oct 2026: double-click / Return opens the console,
the bottom panel maximises, the lab moved to the right pane, a port picker appears when
cabling, devices can be renamed (canvas, inspector, context menu), the toolbar is native
and no longer carries device power, contrast tokens were fixed and status indicators gained
shape. Priorities 5–6 and the 🟡/🟢 items not listed there remain open.

## Overall impression

The visual direction works. The monotone palette reads as a professional tool,
the shaped device symbols are identifiable at a glance even at 60–80 % zoom, and
the canvas is clearly the focus. The biggest opportunity is not visual but
**workflow**: the interface is laid out like a diagramming tool, while a CCNA
student spends most of their time *in a console, following lab instructions, on
specific ports*. Those three activities are currently the most cramped or least
supported parts of the window.

## Core task flows

| Task | Today | Verdict |
|---|---|---|
| Place devices | Drag from Library or double-click; ⌘K "Add …" | ✅ Good. Double-click and ⌘K drop devices at fixed spots that can be off-screen. |
| Cable devices | Connect tool → drag or click both; first free ports used | 🔴 Students can't choose ports, yet labs specify them ("uplink to Router Gi0/0"). |
| Rename a device | Not possible | 🔴 Hostnames are the first thing every lab asks for. |
| Configure a device | Bottom-panel Console (≈200 pt tall) or read-only Config tab | 🔴 The main activity gets the smallest area; no double-click-to-console. |
| Follow a lab | Lab tab in the bottom panel | 🔴 Shares the panel with Console, so instructions vanish while typing commands. |
| Test connectivity | Simulation mode, timeline, events, packet on cable | 🟡 Clear once found; PDU tool not built; no per-packet layer view from events. |
| Inspect a packet | Capture tab (per cable) | 🟡 Good for Wireshark-style work, but Packet Tracer's "click the envelope, see the OSI layers" is missing. |
| Arrange / tidy | Drag, snap, align/distribute | ✅ Good. |

## Usability

| Finding | Severity | Recommendation |
|---|---|---|
| **No port choice when cabling.** Ports are auto-assigned, so a lab step like "Router Gi0/0 to Switch Gi0/1" can't be done on purpose. | 🔴 | On drop, show a port popover per end (free ports listed, a sensible default pre-selected, Return accepts). Keep "Automatic" cable as the fast path. |
| **Console is cramped.** The CLI, where students spend most of their time, lives in a ~200 pt bottom panel. | 🔴 | Double-click a device → open its console focused. Add "Maximise panel" (⌥⌘↑) and "Open in Window" for consoles. Remember the panel height per document. |
| **Lab and Console are mutually exclusive tabs.** | 🔴 | Move the lab to a right-side pane (an inspector tab, or a dedicated "Lab" sidebar that replaces the inspector while a lab is open) so instructions and console are visible together. CML's split panes are the model. |
| **Can't rename devices.** | 🔴 | Inline rename: double-click the name label on the canvas, or edit the title field in the inspector. Needs a `rename_device` command in the core. |
| **Two different "play" controls.** The toolbar's ▶ ❚❚ ■ powers devices on and off, and the timeline's ▶ ❚❚ plays the simulation. Same glyphs, unrelated meanings. | 🟡 | Remove the power group from the toolbar (devices are always on in a CCNA lab). Keep power in the Devices menu, the context menu and the inspector. The toolbar's ▶ then always means "simulation". |
| **The toolbar is 15 unlabeled grey glyphs.** Fine for experts, high recall cost for students; PDU and Capture icons are ambiguous. | 🟡 | Use native toolbar items (`ToolbarItem` + `Label`) so View → Customize Toolbar and "Icon and Text" work. Group as: Tools · Mode · Simulation · Panels. Add shortcut hints to every tooltip. |
| **New devices can land off-screen** (library double-click scans from the top-left; ⌘K uses a fixed point). | 🟡 | Place at the centre of the visible viewport, nudged to the nearest free spot. |
| **The bottom panel opens on Events in Realtime mode**, which lists events while saying "switch to Simulation". | 🟡 | Start new documents with the panel collapsed. Default to Console; auto-switch to Events when entering Simulation and back afterwards. |
| **Library and Topology share one search field**, so a filter typed in one silently filters the other. | 🟡 | Keep separate search text per tab. |
| **No packet detail from simulation.** Selecting an event moves the clock but doesn't show the packet. | 🟡 | Selecting an event or clicking the envelope shows a "Packet" inspector: layers (Ethernet, IP, ICMP…) with in/out decisions at that hop. This is the scope's signature "decision explanation" feature. |
| **Zoom to fit ignores the floating minimap and timeline**, so devices can sit underneath them. | 🟡 | Inset the fit rectangle by the overlays' frames. Let the minimap collapse to a button. |
| **The Hand tool duplicates trackpad scrolling.** | 🟢 | Keep H, but also support Space-drag (the Figma/Sketch convention) and consider dropping the toolbar button. |
| **The Topology outline lists all cables expanded**, which dominates on bigger labs. | 🟢 | Collapse the Cables group by default; show cables under each device on disclosure instead. |
| **Preview content is convincing enough to mistake for real data** (addresses, MACs, console output). | 🟢 | Keep the badges. Consider dimming preview-only values in the Overview tab too, where there is no badge. |

## Visual hierarchy

- **What draws the eye first:** the canvas topology. Correct.
- **Reading flow:** toolbar → canvas → inspector header → bottom panel. The left
  sidebar is quiet, which is right for a palette.
- **Emphasis:** selection (accent ring and name pill) and simulation state
  (accent rule plus timeline) stand out appropriately. Link lights are slightly
  too prominent relative to the cables, because the cables themselves are too faint.
- **Density at lower zoom:** interface labels use a fixed 10.5 pt font, scaled
  with the canvas. At 80 % that is ~8.4 pt and at 50 % ~5 pt, so they become
  noise. Use semantic zoom: hide interface labels below ~70 %, and keep device
  names at a constant on-screen size.
- **Inspector:** the header (symbol, name, status pill) is strong. The Overview
  tab is a long list of equal-weight rows; put the 3–4 things students check
  (hostname, interfaces up/total, address) first and fold the rest.

## Consistency

| Element | Issue | Recommendation |
|---|---|---|
| Tool naming | Toolbar says "Area", DESIGN.md says "Rectangle (R)". | Pick "Area" everywhere and update DESIGN.md. |
| Buttons | Inspector "Power" is a pull-down with an accent chevron; neighbouring buttons are plain. | Use the same small bordered style for all inspector action buttons. |
| Toolbar | Custom `IconButton`s, while the mode switch is a native segmented control. | Move to native toolbar items throughout (also fixes Customize Toolbar). |
| Section headers | Use the tertiary colour, which fails contrast (below). | Use `textSecondary`. |
| Port names | Full names in some places (Config), abbreviated elsewhere. | Abbreviate in compact UI, full name in tooltips and Config. This is mostly done already; make it the rule. |

## Accessibility

Measured contrast (WCAG 2.1):

| Pair | Light | Dark | Needs | Result |
|---|---|---|---|---|
| Primary text on surface | 17.9 | 14.2 | 4.5 | ✅ |
| Secondary text on surface | 6.7 | 6.6 | 4.5 | ✅ |
| **Tertiary text (section headers, captions) on surface** | **3.4** | **3.5** | 4.5 | ❌ |
| **Tertiary text on sunken (outline counts, hints)** | **3.1** | **3.7** | 4.5 | ❌ |
| **Cables on canvas** (non-text graphic) | **2.5** | **2.8** | 3.0 | ❌ |
| Device outlines on canvas | 13.9 | 13.7 | 3.0 | ✅ |
| Status up / down dots | 3.1 / 4.1 | 6.9 / 5.2 | 3.0 | ✅ |
| **Status warning (amber) dot** | **2.7** | 8.4 | 3.0 | ❌ light |
| **White on accent (selected name pill, 12 pt)** | **4.0** | **3.7** | 4.5 | ❌ |

- **Fix tokens:** tertiary text → `#6B6B6B` light / `#8F8F8F` dark (≥ 4.8:1 on every surface) (or use secondary
  for anything informative); cables → `#7A7A7A` in both modes (3.9 / 4.2:1); amber
  → `#A87300` light (3.8:1).
- **Selected name pill:** make the label semibold, or switch to an accent-tinted
  pill with accent text, which works with any system accent colour.
- **Colour alone:** link lights and power dots rely only on green/amber/red. Add
  shape: filled for up, hollow ring for down, half-filled for suspended. Put the
  state in tooltips.
- **Targets:** toolbar and timeline buttons are 26 pt and fine. The console
  session close "×" (8 pt glyph) and the 7 pt link lights are too small to hit;
  give them at least 20 pt hit areas.
- **VoiceOver:** devices have labels. Cables are drawn in a Canvas, so they are
  invisible to VoiceOver; give the existing cable hit areas accessibility labels
  ("Cable, R1 Gi0/0 to SW1 Gi0/1"). Custom tab strips need the `.isSelected`
  trait. Tables are native and fine.
- **Motion:** packet animation should respect Reduce Motion (jump between hops instead).
- **Keyboard:** single-letter tools, ⌘K and menu shortcuts are good. Missing:
  moving the canvas selection with the arrow keys, Tab between panels, and
  Return to open the selected device's console.

## What works well

- **Shape-as-identity symbols.** Every kind is distinguishable without colour,
  and the selection ring following the outline is a quality touch.
- **Calm monotone palette** with colour reserved for meaning. Simulation mode is
  unmistakable (accent rule, timeline) without shouting.
- **Cables carry real information**: interface labels (from GNS3), link lights
  (from Packet Tracer), dashed fibre.
- **The inspector adapts to the selection** (device, cable, multiple, project),
  and multi-select align/distribute is genuinely useful.
- **⌘K and the menus** expose every action; nothing is toolbar-only.
- **Honest preview badges** keep the prototype truthful about what is real.
- **The Topology outline** with port-usage counts ("4/26") is a fast way to find
  free ports, and is better than GNS3's summary.

## Priority recommendations

1. **Make the console-and-lab loop first-class.** Double-click a device to open
   its console; let the console maximise or open in its own window; move the lab
   into a right-side pane so instructions stay visible while typing. *Why:*
   this is where students spend most of their time.
2. **Port selection when cabling, and inline rename.** Both are required to follow
   any CCNA lab as written. Ship them in M1 alongside the canvas work.
3. **Clear up the toolbar.** Remove device power (devices are always on), so ▶
   only means "simulation". Use native toolbar items with "Icon and Text"
   support, and group them as Tools · Mode · Simulation · Panels.
4. **Fix the failing contrast tokens** (tertiary text, cables, amber, selected
   label) and add shape to status indicators. It's a small token change with a big
   legibility gain.
5. **Semantic zoom and smarter placement.** Hide interface labels below ~70 %,
   keep names a constant size, place new devices in the visible viewport, and
   make zoom-to-fit respect the overlays.
6. **Packet layer view from events and envelopes.** This is the step that makes
   simulation mode teach rather than just animate.

## Not covered

- No usability testing with real students. These findings come from heuristic
  review and task walkthroughs; a five-person test with CCNA students on
  "cable this lab and make PC1 ping the gateway" would validate priorities 1–3.
- Console, events, capture and labs show preview content, so their real
  interaction (typing speed, completion, scrolling under load) is untested.
- Window sizes below 1280×800 and very large topologies (100+ devices) were not reviewed.
