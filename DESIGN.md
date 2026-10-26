# netrace — interface direction

The look and layout netrace is built toward. Features are implemented
milestone by milestone (SCOPE.md); this file fixes how they will look and
where they live, so the interface can be reviewed before the engine catches up.

## Principles

1. **Monotone first.** The interface is neutral grey. Colour is reserved for
   meaning: the accent (selection, focus, the active tool), status (up / down /
   warning) and protocol identity in simulation views. Device kinds are told
   apart by shape, never by colour.
2. **Shape is identity.** Every device is drawn as its own silhouette (round
   router, wide switch, monitor-shaped PC, cloud for the internet). The
   silhouette *is* the container: no tile around it, no extra padding. Selection
   rings follow the outline.
3. **Mac-native.** System font (SF Pro / SF Mono), system accent colour (follows
   System Settings → Appearance), standard menus and shortcuts, trackpad
   scrolling and pinch-zoom, document windows with autosave. No Liquid Glass;
   panels are opaque.
4. **One window, everything reachable.** Library, canvas, inspector and a
   bottom panel. Every panel can be hidden; nothing opens in a modal window.
5. **Calm density.** 13 pt body, 11 pt secondary, 1 px separators, 8 pt grid.
   Professional tools read as quiet until something needs attention.

## Colour

Neutral scale (no tint), light / dark. Light mode avoids pure white and black, which glare together over long sessions:

| Role | Light | Dark | Used for |
|---|---|---|---|
| canvas | `#F0F0F0` | `#161616` | topology background |
| canvasGrid | `#D5D5D5` | `#2B2B2B` | grid dots |
| surface | `#F9F9F9` | `#1E1E1E` | sidebars, inspector, bottom panel |
| surfaceRaised | `#F4F4F4` | `#242424` | headers, floating controls |
| surfaceSunken | `#EEEEEE` | `#191919` | terminal, inset lists, status bar |
| hover | `#E9E9E9` | `#2A2A2A` | row / tile hover |
| border | `#DFDFDF` | `#2E2E2E` | separators |
| borderStrong | `#D4D4D4` | `#3A3A3A` | control outlines |
| textPrimary | `#1C1C1C` | `#EDEDED` | |
| textSecondary | `#5C5C5C` | `#A3A3A3` | labels, captions |
| textTertiary | `#6B6B6B` | `#8F8F8F` | hints, captions (≥ 4.8:1) |
| symbolInk | `#3A3A3C` | `#D9D9D9` | solid device silhouettes (charcoal, not black) |
| symbolKnockout | `#F0F0F0` | `#161616` | detail knocked out of silhouettes |
| root | `#D9A400` | `#FFD60A` | spanning-tree root bridge, root ports |
| link | `#7A7A7A` | `#7A7A7A` | cables (≥ 3:1 on canvas) |

Meaningful colour:

- **Accent**: the system accent colour (`Color.accentColor`).
- **Status**: up `#2F9E5B / #3DB46D`, warning `#A87300 / #E5A50A`, down `#D93F3F / #EF5350`.
  Appears as small indicators and link lights only, always with shape: filled / half / ring.
- **Protocols** (simulation, events, capture): ARP amber, ICMP sky, DHCP green, OSPF violet,
  STP slate, CDP / LLDP grey, TCP blue, UDP teal, DNS pink. Shown as a 7 pt swatch next to a
  neutral label, and as the fill of packet envelopes on the canvas.

## Type

| Token | Font |
|---|---|
| title | SF Pro 17 semibold |
| body | SF Pro 13 |
| label | SF Pro 12 medium |
| caption | SF Pro 11 |
| section header | SF Pro 10.5 semibold, uppercase, +0.5 tracking |
| mono | SF Mono 11.5 (interface names, addresses, CLI, packet fields) |

## Iconography

- **Device symbols**: drawn in code (`Design/DeviceSymbol.swift`) as solid filled silhouettes,
  like SF Symbols' `.fill` variants: one ink colour, with interior detail (ports, screens,
  keypads) knocked out in the background colour. Selection is a solid accent band hugging the
  silhouette. Custom device colours (M2.5) tint the ink. 13 kinds: router, switch, layer 3 switch, hub,
  firewall, access point, cloud, PC, laptop, server, printer, IP phone, smartphone.
- **SF Symbols** for standard actions (play, stop, zoom, sidebar, search, terminal…).
- **Custom SVG icons** (`Assets.xcassets/Icons`, 24 pt grid, 1.5 pt round strokes) where SF
  Symbols has nothing suitable: cable types (auto, copper, fibre, console), packet, PDU,
  capture, port, lab, topology, empty canvas.
- App icon: graphite squircle, white three-node network, one accent-blue packet.

## Layout

```
┌ Toolbar ─────────────────────────────────────────────────────────────────────┐
│ ◧  │ [↖ ✋ ⌁ ✎ ▭] │ [Realtime|Simulation] │        ✉+ ◎ │ ⌘ │ ▤ ◨          │
├────────────┬───────────────────────────────────────────────┬─────────────────┤
│ Library /  │ canvas                                        │ Inspector | Lab │
│ Topology   │   devices, cables, interface labels,          │ Overview /      │
│            │   link lights, packets (simulation)           │ Interfaces /    │
│ search     │                                               │ Config          │
│ Network    │                        [sim timeline]         │ — or the open   │
│ End devices│ [− 100% + ⤢]                      [minimap]   │ lab: brief,     │
│ Connections│                                               │ checks, hints   │
│ Annotations├───────────────────────────────────────────────┤                 │
│            │ Console │ Events │ Capture          [⤢] [⌄]   │                 │
│            │ bottom panel                                  │                 │
├────────────┴───────────────────────────────────────────────┴─────────────────┤
│ ● Realtime  00:00:12.4 │ 6 devices · 5 links │ 2 selected     100%  core 0.1 │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Where each idea comes from

| Feature | Inspired by | Lives in |
|---|---|---|
| Realtime / Simulation mode switch | Packet Tracer | toolbar, status bar |
| Event list with protocol filters, step / play / scrub | Packet Tracer | Events tab, canvas timeline |
| Packets animated along cables | Packet Tracer | canvas (simulation mode) |
| Add simple PDU (one-click ping) | Packet Tracer | toolbar, Devices menu |
| Labs with instructions and automatic checks | Packet Tracer (Activity Wizard) | Lab pane (right), beside the console |
| Start / suspend / stop / reload all devices | GNS3 | Devices menu, context menu, inspector (not the toolbar) |
| Device categories + "add link" in a side palette | GNS3 devices toolbar | Library |
| Topology summary with device status | GNS3 | Topology tab in the sidebar |
| Show / hide interface labels | GNS3 | View menu, canvas |
| Console to one / all devices | GNS3, CML | Console tab, context menu |
| Snapshots | GNS3 | project inspector, File menu |
| Notes, rectangles, ellipses, lines | GNS3, CML annotations | toolbar tools, Library |
| Inspector tabs (overview / interfaces / config) | CML sidebar | Inspector |
| Tabbed session panes, tab selects node on canvas | CML panes | bottom panel |
| Packet capture on a link, summary + detail panes | CML, Wireshark | Capture tab, link inspector |
| Link conditioning (latency, jitter, loss, bandwidth) | CML, GNS3 link filters | link inspector |
| Multi-select alignment | CML | context menu (M1) |
| Lab actions on empty-canvas right-click | CML | canvas context menu |
| Minimap, zoom control, command palette | Figma, Xcode, Linear | canvas overlays, ⌘K |
| Topology as text (export) | containerlab | File menu (later) |

## Interaction defaults

- Tools: Select (V), Hand (H), Connect (C), Note (N), Shape (R). Single-letter keys work while
  the canvas has focus. The toolbar uses native, customisable items (View → Customize Toolbar,
  "Icon and Text"), grouped Tools · Mode · Simulation · Panels. Device power is not on the
  toolbar, so ▶ only ever means "play the simulation".
- Drag a device from the Library, or double-click it.
- **Connect:** with a specific cable (Copper, Fibre, Console — Copper is the default), drag from
  one device to another or click both; a popover asks for the port on each end, pre-filled with
  the first free ports, so Return accepts. "Automatic" connects immediately on free ports.
- **Double-click a device** to open its console (focused, ready to type). **Return** does the same
  for the selected device. **Double-click its name** (or edit the inspector title, or Rename… in the
  context menu) to rename it; hostnames follow IOS rules and must be unique.
- **Arrange:** drag on empty canvas (even inside an area) for marquee selection, Shift adds;
  ⌘A selects all; arrow keys nudge (Shift for bigger steps); hold Space and drag to pan with any
  tool. ⌘C / ⌘V / ⌘D copy, paste and duplicate devices *with the cables between them*; copies get
  fresh hostnames and land in view.
- **Annotate:** Note tool (N) — click to place, type, click away. Shape tool (R) — pick rounded
  rectangle, rectangle, ellipse or polygon (and its number of sides) in the bar above the canvas,
  then drag to outline a zone and name it. Shapes sit behind devices and only their border and
  label take clicks; rectangles carry the label inside the top-left corner, round and pointed
  shapes just above the outline. Select to move, drag the corner handle to resize, double-click
  to edit; all saved in the file.
- **Style:** the inspector (or the right-click menu) sets a device's colour, a cable's colour and
  line pattern, a note's colour, and a shape's outline, border colour and pattern, fill colour and
  opacity. Copper cables are a single line, fibre a double line.
- **Moving together:** dragging a shape brings the devices, notes and shapes inside it. Arrange
  menu: Group / Ungroup (⌥⌘G / ⌥⇧⌘G), Bring to Front / Forward (⇧⌘F / ⌥⇧⌘F), Send Backward / to
  Back (⌥⇧⌘B / ⇧⌘B), Lock / Unlock (⌘L / ⌥⌘L). Drags snap to other items with guide lines, else
  to the grid. Marquee and Shift-click select devices, shapes and notes together.
- **Zoom and pan:** pinch zooms around the pointer; two-finger scroll (or the mouse wheel) pans,
  with momentum; ⌘-scroll zooms around the pointer; ⌘+ / ⌘− / ⌘0 and the zoom control zoom around
  the middle of the view; ⌘9 fits the topology. The canvas has no edges.
- **Semantic zoom:** interface labels hide below 70 %; device names grow to stay readable.
- The bottom panel maximises (⌥⌘↑) so the console can take the window; the lab stays visible in
  the right pane (⌥⌘4).
- Status is shape plus colour everywhere: filled = up, half = suspended, ring = down.
- Right-click a device: console, rename, configure, capture, power, duplicate, delete.
  Right-click a cable: capture, conditioning, delete. Right-click empty canvas: lab actions.
- **Simulate:** give end devices an address in the inspector's IP Configuration. Add PDU (toolbar,
  ⇧⌘P or the Events tab), then click the source and the destination. In Realtime the result
  appears at once; in Simulation, play or step and watch every frame travel, including flooded
  copies, with a red ✕ where a device ignores or drops one. PCs also accept `ping`, `tracert`,
  `ipconfig` and `arp -a` in their console.
- **Console (routers and switches):** the IOS command line, one session per device that keeps its
  mode while you switch tabs. `?` shows help as soon as it's typed (the line stays), Tab
  completes, ↑ / ↓ step through history, Ctrl-Z leaves configuration mode, passwords aren't
  echoed. Pasting several lines runs them in order. The toolbar has *Snippets* (fill-in blocks
  of configuration, run in the right mode) and *Paste Lines*. The inspector's Interfaces tab shows
  real up/down states and addresses, its Config tab the live running-config, with *Copy To…*
  for duplicating a configuration with variations.
- **Packet inspector:** selecting an event shows one plain sentence about what the device did and
  why, then a layer strip (L1–L4) where only the layers that device read or built are highlighted;
  raw fields expand on demand, with fields set by that device marked. Previous / Next steps
  through the whole ping.
- ⌘K opens the command palette for every action and every device kind.

## Visual language for styling and overlays

- **Colour on the canvas** comes from a fixed set of system colours (blue, green, orange, red,
  purple, teal, gray) so diagrams stay legible in both modes. Chrome stays monotone; colour is
  reserved for things the user chose or that carry meaning (status, protocols, VLANs).
- **Cable styles:** copper solid, fibre double line (built); console dotted and serial zig-zag
  when those media exist. Colour and dash pattern are optional, per cable.
- **Shapes:** rounded rectangle, rectangle, ellipse, *n*-sided polygon (built); lines and arrows
  later. Fill, border and opacity; behind devices; snapping, grouping, z-order, lock.
- **VLAN overlay (built):** access cables in their VLAN's colour (a fixed palette per VLAN number,
  VLAN 1 neutral), trunks as a striped band, an amber bar across a port spanning tree blocks, a
  red light on an err-disabled port, a crown on each root bridge, and a legend above the zoom
  control. A cable's own colour, if set, wins over the VLAN tint.
- **STP view (built):** View → Spanning Tree View (⌥⌘S) or the STP tab of the right pane. The root
  bridge gets a gold ring and crown; each switch port a role badge (R green, D blue, ✕ red for
  blocked); interface labels step aside. The pane gives the engine's reason for every role and
  each switch's path to the root, with a VLAN picker. Right-click a cable → Fail Link replays the
  reconvergence one change at a time (Play / Step), highlighting the port that changed; the
  project itself isn't touched, and Restore Link ends the replay.
- **Snapping:** dragging always snaps to the grid; lining up with other devices and shapes is
  opt-in (View → Snap to Items), and holding ⌘ flips it for one drag.
- **Shape labels:** neutral by default (white or near-black over a strong fill, whichever reads),
  or any palette colour from the inspector or right-click menu.
- **Feature overlays** (see SCOPE.md, *Configured features are visible*): small badges at
  interfaces (NAT inside/outside, ACL shield, DHCP), VLAN tints, STP blocked-port bars, OSPF area
  outlines. Badges use SF Symbols where they exist, are 12 pt, and never cover a device name.

## Status of the interface

The interface is complete as a layout. Panels backed by milestones that are not built yet
(console, events, capture, labs, conditioning, snapshots) show clearly labelled preview content.
