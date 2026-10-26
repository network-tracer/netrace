# netrace — v1.0 scope

A native macOS network simulator for CCNA students. Built from public standards
(IEEE 802.x, IETF RFCs, publicly documented Cisco-style CLI behaviour). No Cisco
code, assets, names or file formats.

## Decisions

| Topic | Decision |
|---|---|
| Audience | CCNA (200-301) students practising for the exam |
| Protocol depth | CCNA core |
| Signature features | Packet visualiser, modern canvas UX, built-in labs |
| Platform | macOS 15+, Apple silicon and Intel |
| Visual style | Flat, crisp, custom design system. **No Liquid Glass** on any macOS version |
| Stack | **Rust core** (simulation, protocols, CLI, labs) + **SwiftUI/AppKit shell**, bridged with UniFFI |
| Document | `.netrace` package (JSON inside, serialised by the Rust core), `DocumentGroup` for autosave, versions, iCloud |

## Architecture

```
┌──────────────────────── Swift (macOS app) ────────────────────────┐
│  Canvas (Metal/Canvas) · Inspector · Terminal tabs · Visualiser   │
│  Lab panel · ⌘K palette · DocumentGroup                           │
└───────────────▲──────────────────────────────┬────────────────────┘
     snapshots, │ packet events, CLI output     │ commands (place, connect,
     lab results│                               │ CLI line, step, play/pause)
┌───────────────┴──── netrace-ffi (UniFFI → XCFramework) ───────────┐
├───────────────────────────── Rust core ───────────────────────────┤
│  netrace-sim      event scheduler, virtual clock, deterministic RNG│
│  netrace-engine   devices, frames, ARP/ICMP, explained traces (M2) │
│  netrace-proto    Ethernet, 802.1Q, STP, ARP, IPv4, ICMP, TCP/UDP, │
│                   OSPF, DHCP, ACL, NAT, CDP/LLDP                    │
│  netrace-devices  router, L2/L3 switch, PC, server                 │
│  netrace-cli      IOS-style parser, modes, completion, show output │
│  netrace-labs     lab format, checks, hints                        │
│  netrace-doc      .netrace serialisation (serde)                   │
└────────────────────────────────────────────────────────────────────┘
```

- The engine runs on its own thread; the UI reads snapshot diffs each frame, never shared mutable state.
- The Rust core has no UI or Apple dependencies, so it can later back a Windows/Linux front end.
- A headless `netrace` CLI binary (Rust) runs topologies and labs for tests and debugging.

## Visual design

- No Liquid Glass. Toolbars, sidebars and panels are custom-drawn with opaque surfaces.
  Until the custom chrome covers everything, set `UIDesignRequiresCompatibility = YES` in
  Info.plist so stock controls on macOS 26 keep the pre-glass look (Apple has said this key is temporary).
- Own design tokens in `Design/Theme.swift`: colour, spacing, radius, type
- Colour: Apple's system palette (red … pink, Gray–Gray 06) with light and dark values,
  mapped to semantic roles. Blue is the accent; green / red / orange / yellow are kept for status
- Type: SF Pro (interface), SF Mono (interface names, CLI). No bundled fonts
- Icons: in-house SVG set, 24 pt grid, 1.5 pt round strokes, template-tinted (`Assets.xcassets/Icons`)
- Device symbols: solid filled silhouettes in the style of SF Symbols' `.fill` variants, detail knocked out
- App icon: `design/app-icon.svg`, rendered by `scripts/render-app-icon.sh`
- View → Appearance: System / Light / Dark

## What students struggled with (design drivers)

From real CCNA classes using Packet Tracer and GNS3. Every milestone is checked against these.

| Pain point | What netrace does about it | Milestone |
|---|---|---|
| **Configuring takes too long.** Typing the same interface, VLAN and addressing blocks again and again. | Paste multi-line config straight into the CLI; config snippets (saved blocks with placeholders); "Apply to selected devices" for bulk changes; full IOS abbreviations and Tab completion. Duplicating devices keeps cabling (M1). | M1, M3 |
| **Configs have to be redone.** Work is lost between sessions or must be retyped on the next device. | Running and startup configs are saved inside the `.netrace` file; snapshots restore a known-good state; **Duplicate with variations**: copy a device's config to others and fill in only what differs (hostname, addresses) from a short form. | M3, M6 |
| **Layer information is hard to understand.** Packet Tracer's OSI view lists fields at every layer with little explanation. | Packet inspector leads with one plain-language sentence per hop ("SW1 learned PC1's MAC on Fa0/1 and forwarded the frame out Fa0/2"), then a layer strip (L1–L4) where only the layers that *changed or mattered* at that hop are highlighted; raw fields are one click away, with changed fields marked. | M2 |
| **Battery drain.** GNS3 runs real router images in virtual machines, so a MacBook dies in minutes. | No VMs: devices are lightweight simulations inside one process, driven by an event queue on a virtual clock. Idle means zero work. See *Energy* below. | All |
| **GNS3 is hard to get running.** A separate local server (often broken on Windows), Cisco images to find and license, heavy RAM/CPU use, a steep learning curve before the first ping. | One app, nothing else to install: no server, no VM, no images. Open it and drag a router out. Everything a CCNA lab needs is built in, and the bundled labs teach the interface as they go. | All |
| **Hard to see what a config actually does.** NAT, ACLs or VLANs are invisible on the diagram; you only find out by reading configs or running `show` commands. | *Configured features are visible* (below): the canvas marks where each feature applies, and the visualiser shows it acting on packets. | M4, M5 |
| **Diagrams all look alike.** Every cable and device is the same grey, so a busy topology is hard to read. | Colours for devices, notes and areas, distinct cable styles per type, and shapes for annotation (below). | M2.5 |

## Configured features are visible

Each feature gets a quiet marker on the canvas when it is configured, and an obvious effect when
it acts on a packet in Simulation mode. Markers can be switched off per feature (View → Overlays).

| Feature | On the canvas | In the visualiser |
|---|---|---|
| NAT / PAT | Inside / outside badges on the interfaces; the NAT router gets a NAT badge | The packet visibly changes address at the router: old address struck through, new one highlighted; the inspector shows the translation table row it used |
| ACLs | Shield badge on the interface and direction (in / out) where an ACL is applied | Permitted packets pass the shield; denied ones stop at it with the matching ACL line |
| VLANs | Access ports and cables tinted by VLAN (with a legend); trunks drawn striped | Frames carry a VLAN tag colour; tagging / untagging is shown at trunk ports |
| STP | Blocked ports shown with a red bar across the link end; the root bridge gets a gold ROOT badge (and a gold halo in the STP view) | BPDUs animate from the root as the simulation starts; a link failure replays as ports change state |
| DHCP | Server and relay badges; pools listed in the inspector | DORA exchange stepped through, with the lease appearing on the PC |
| OSPF | Neighbour adjacencies drawn as a thin overlay in the OSPF view, labelled with their area when there are several | Hellos and LS Updates animate as the simulation starts; DR / BDR marked |
| Routing | Hover a router to see its routes highlighted as paths on the diagram | The route that matched is named in the hop explanation |

## Visualisations come from the engine

The rule for every visualisation: it is drawn from what the engine actually did (trace events,
the computed spanning tree, routing tables, ACL matches, NAT tables, OSPF state), never from a
separate model of what the UI expects to happen. If the engine is wrong the picture is wrong
too, so the visualiser doubles as a debugging tool, and what it teaches is what the simulation
really does. Each engine decision carries the reason it was made (the route, the ACL line, the
STP role), and the UI only presents it.

| Topic | What the student sees | Comes from | Milestone |
|---|---|---|---|
| VLANs | Which ports a frame may leave by, and why the others are skipped (other VLAN, not allowed on the trunk, VLAN missing); tag added or removed at trunks | Switch forwarding decision per frame | M4 ✅ (trace), M5.5 (view) |
| STP | An STP view: root bridge highlighted, root / designated ports marked, blocked ports in red, each port's reason ("lower path cost to root via Gi0/1", "SW2 has the lower bridge ID on this segment"); break a link and watch the tree reconverge step by step | Spanning-tree computation, recomputed on topology change, with its tie-break decisions recorded | M5.5 |
| DHCP | Discover → Offer → Request → Acknowledge stepped through, with the lease appearing on the PC and in the server's binding table; relay hops shown | DHCP client / server / relay events | M5 |
| ACLs | A packet reaching R1 Gi0/1 *in*, the ACL evaluated line by line, "matched line 10: deny ip host 10.0.0.5 any → dropped" (or the implicit deny at the end); a shield badge where each ACL is applied | ACL evaluation result attached to the trace event | M5 |
| NAT / PAT | The packet's source address and port rewritten at the router, before and after side by side; the translation table entry it created or used; inside / outside badges | NAT translation event and table | M5 |
| OSPF | Neighbour states (Down → Init → 2-Way → Full), DR / BDR election, the link-state database per router, SPF result and why a route won; a link failure propagating as LSAs and routes changing | OSPF adjacency, LSDB and SPF events | M5 |

The STP view is the template: a mode that overlays one protocol's real state on the topology,
lets the user break things (shut a port, pull a cable), and replays how the protocol reacts.

## Compatibility with Cisco IOS and Packet Tracer

netrace's command line is *IOS-style*: familiar prompts, modes, abbreviations and output, written
from Cisco documentation and lab transcripts. It is not verified against real IOS or Packet Tracer
yet, so compatibility is a claim to test, command by command:

- **Compatibility matrix** ([COMPATIBILITY.md](COMPATIBILITY.md)): every command and `show` output,
  its status (implemented, verified against IOS 15.x, verified against Packet Tracer, differs on
  purpose), and the reference it was checked against.
- **Golden transcripts:** sessions captured from real IOS (lab equipment or CML) and Packet Tracer,
  replayed through netrace in tests; differences are either fixed or written down as deliberate.
- **Behaviour tests,** not just output: the same lab configuration should give the same
  reachability, the same spanning-tree roles and the same routing tables in all three.
- **Deliberate differences stay visible:** no DTP, converged STP without timers, simulated
  platforms. Where netrace differs, the console says so (as it does for `switchport mode dynamic desirable`).
- **Import later:** reading a running-config text into a device is the first step towards
  moving labs in from real gear; `.pkt` import stays out of scope.

## Command line and routing (built in M3)

- **Engine:** one IP stack shared by end devices, routers and a switch's `interface Vlan1`:
  connected, local, static (next hop or exit interface, recursive), floating static (distance)
  and default routes; longest-prefix match; TTL; ARP on every interface. Routers drop the packet
  that triggers ARP, as IOS does, so the first ping across a router times out, and the trace
  says why. ICMP Time Exceeded and Destination Unreachable back to the source. `shutdown`
  takes a link down at both ends.
- **Pings and traceroute:** PCs print Windows `ping` / `tracert`; routers and switches print IOS
  (`!!!!!`, `Success rate is …`, three probes per hop).
- **CLI** (`netrace-cli` crate): user / privileged / config / interface / subinterface / line
  modes with IOS prompts; unique abbreviations; `?` (both `sh?` and `show ?`), Tab, `do`, `no`
  forms; `% Invalid input detected at '^' marker.`, `% Incomplete command.`, `% Ambiguous
  command`; the domain-lookup delay on mistyped words (`no ip domain-lookup` removes it);
  `enable secret` and console `login` password prompts; `banner motd` with delimiters;
  `copy run start` with its filename prompt, `reload` with save / confirm prompts, `write`,
  `erase startup-config`; interface up/down console messages; type 7 password display.
- **show:** `running-config`, `startup-config`, `ip interface brief`, `ip route` (codes legend,
  gateway of last resort, classful grouping), `interfaces`, `arp`, `mac address-table`,
  `version`, `history`.
- **Saved in the file:** running and startup configuration of every router and switch; undo
  covers every configuration change made in a console.
- **Faster configuration:** paste many lines at once (or *Paste Lines*, ⇧⌘V); snippets with
  `{{Field=default}}` placeholders, built in and user-written, filled in from a short form and run
  in the right mode; *Copy Configuration To…* gives other devices of the same model one
  device's configuration with only hostnames, interface addresses and gateway changed, from a
  grid with suggested values; copy / paste of devices keeps their configuration.

## Switching (built in M4)

- **VLANs:** VLAN database (`vlan 10`, `name`), access ports, 802.1Q trunks with native VLAN and
  allowed lists (`add` / `remove`); frames carry a real tag on trunks and the inspector shows it;
  flooding and MAC learning stay inside a VLAN; a missing VLAN, a disallowed VLAN or a tagged
  frame on an access port is dropped with the reason. No DTP: `dynamic auto` acts as access.
- **Inter-VLAN routing:** router subinterfaces (`encapsulation dot1Q N [native]`) and layer 3
  switch SVIs with `ip routing`, plus routed ports (`no switchport`); switch `Vlan` interfaces
  are up only while a port in their VLAN is up and forwarding.
- **Spanning tree:** PVST+ / Rapid PVST+, one tree per VLAN computed in its converged state (no
  waiting for timers): root election by priority and MAC, root / designated / alternate / backup
  ports with 802.1D costs and tie-breaks, hubs treated as shared segments. Blocked ports drop
  data frames, and the trace says why. Priorities, `root primary` / `secondary`, PortFast,
  BPDU guard (err-disables an edge port that faces a switch, taking its link down).
- **Port security:** maximum, sticky, static MACs, shutdown / restrict / protect; a violation in
  shutdown mode err-disables the port for the rest of the replay.
- **CDP / LLDP:** neighbours from the cabling (CDP on by default, LLDP after `lldp run` on both
  ends; PCs don't take part), `no cdp enable` per port.
- **CLI:** `vlan` mode, `interface range`, all the above commands, and `show vlan [brief]`,
  `show interfaces trunk | status | X switchport`, `show spanning-tree [vlan N]`,
  `show cdp neighbors [detail]`, `show lldp neighbors`, `show port-security [interface X | address]`,
  `show mac address-table` by VLAN; the running-config carries every switching setting.
- **Canvas:** access cables take their VLAN's colour, trunks are a striped band, a blocked port
  has an amber bar and light, an err-disabled port a red light, the root bridge a crown, with a
  legend (View → Hide VLANs and Spanning Tree, ⌥⌘V). Parallel cables between two devices fan out.
- **Snippets:** create VLANs, access ports, trunk port, router-on-a-stick subinterface, port security.
- Later: BPDUs and convergence animated in Simulation mode, EtherChannel, VTP, voice VLAN.

## Canvas styling

Built in M2.5 (file format 2; format 1 files still open):

- **Shapes:** rounded rectangle, rectangle, ellipse and regular polygons with 3–12 sides, each
  with a border colour and style (solid, dashed, dotted), a fill colour and fill opacity.
- **How shapes interact:** shapes sit behind cables and devices, in a z-order you can change
  (bring to front / forward, send backward / to back); dragging snaps to the grid, and only
  when asked (View → Snap to Items, or ⌘ while dragging) to other devices' centres and shapes'
  edges and centres, with guide lines; devices whose
  centre is inside a shape move with it, and so do shapes and notes inside it (nesting works);
  overlapping fills blend, never hide devices; group / ungroup (⌥⌘G) makes devices, shapes and
  notes select and move together; lock (⌘L) stops a shape or note moving or resizing.
- **Colours:** a fixed palette of nine system colours for devices (tinting the solid symbol),
  shape borders and fills, notes and cables, readable in light and dark mode.
- **Cables:** copper is a single line, fibre a double line; each cable can take a colour and a
  dashed or dotted pattern.

Later, as the media and features arrive: console cables dotted, serial zig-zag, wireless
associations as faint arcs, per-cable labels, lines and arrows as annotations, copying shapes
and notes with ⌘C / ⌘V.

## After v1.0: enterprise track (CCNP)

Packet Tracer stops at CCNA. GNS3 / CML go further but need real images, servers and lots of RAM.
After v1.0, netrace can grow the same engine toward CCNP ENCOR / ENARSI topics, still as a
simulation with no images:

- IOS-XE style CLI depth: `show` output, `do`, `section` / `include` filters, configuration
  archives and rollback
- Multi-area OSPF, EIGRP, BGP basics, route redistribution and filtering (prefix lists, route maps)
- HSRP / VRRP, EtherChannel (LACP / PAgP), MST
- VRF-lite, GRE tunnels, IPsec basics, NAT64 later
- QoS markings visible in the visualiser
- Larger topologies (500+ devices) within the same energy budget

This is a separate product phase: it is planned, not promised for v1.x.

## Energy

The engine and UI are built so a lab can run all class on battery.

- **Event-driven, never polling.** The engine only runs when an event is due; a quiet network costs nothing. No timers firing while idle.
- **Virtual time.** Protocol timers (STP, OSPF, ARP aging) are events on the virtual clock, not real sleeping threads; accelerated timers simply move the clock.
- **Redraw only on change.** The canvas re-renders when the model or viewport changes; continuous animation runs only while a simulation is playing.
- **Pause when unseen.** Playback pauses when the window is hidden, minimised or fully covered.
- **Targets:** 0 % CPU and "Low" Energy Impact in Activity Monitor when idle; under 5 % CPU on an M1 MacBook Air while a 50-device topology runs in realtime. Measured each milestone with Instruments (Energy Log) and `powermetrics`.

## Devices

| Device | v1.0 |
|---|---|
| Router | 4× GigabitEthernet, subinterfaces, full IOS-style CLI |
| Layer 2 switch | 24× FastEthernet + 2× GigabitEthernet |
| Layer 3 switch | 24× GigabitEthernet, SVIs, `ip routing` |
| PC / Laptop | IP config panel + command prompt |
| Server | PC plus DHCP and simple TCP/UDP listener services |

Cables: copper (auto-MDIX, so straight vs crossover never blocks a student) and
fibre. Link lights show green / amber (STP blocking) / red (down).

## Protocols and features

### Layer 2
- Ethernet II framing, MAC learning, aging, flooding
- 802.1Q VLANs: access/trunk ports, native VLAN, allowed-VLAN lists (static trunks, no DTP)
- Spanning tree: 802.1D and Rapid PVST+, root election, port roles/states, PortFast, BPDU guard
- CDP and LLDP neighbours
- Port security: max MACs, sticky, violation modes

### Layer 3
- IPv4, ARP, ICMP (echo, unreachable, TTL exceeded → traceroute)
- Connected, static, default and floating static routes; longest-prefix match; administrative distance
- Inter-VLAN routing: router-on-a-stick and SVIs
- OSPFv2 single-area: neighbours, DR/BDR, cost, passive interfaces, default-information originate

### Services and security
- DHCP server (pools, excluded addresses), relay (`ip helper-address`), client on PCs
- Standard and extended ACLs, numbered and named (IP / ICMP / TCP / UDP ports)
- NAT: static, dynamic, PAT (overload)
- Minimal TCP (handshake + data) and UDP so extended ACLs and PAT have real traffic to act on

### CLI (IOS-style)
- Modes: user → privileged → global config → interface / line / router / sub-modes
- Abbreviations (`sh ip int br`), `?` help, Tab completion, `do`, `no` forms, command history
- Authentic error output (`% Invalid input detected at '^' marker.`)
- `show`: running-config, startup-config, ip interface brief, ip route, interfaces, vlan brief,
  interfaces trunk, mac address-table, spanning-tree, ip ospf neighbor/interface, cdp neighbors,
  ip dhcp binding, access-lists, ip nat translations, arp, port-security
- `copy running-config startup-config`, `write`, `reload`
- PC prompt: `ipconfig`, `ipconfig /renew`, `ping`, `tracert`, `arp -a`, `curl`, `telnet`

## Signature feature 1: packet visualiser

- **Realtime** and **Simulation** modes, switchable at any time
- Timeline with scrubbing, play / pause / step, speed control and protocol filters
- Packets animate along cables; colour-coded by protocol
- Click a packet → layer-by-layer inspector (Ethernet / 802.1Q / IP / ICMP / TCP / UDP fields)
- **Decision explanations** at every hop, in plain language, e.g. “SW1 learned AA:BB… on Fa0/1;
  destination unknown, flooded to VLAN 10 ports”, “R1 dropped: ACL 101 line 20 denies TCP/80”
- Timer acceleration toggle so STP / OSPF convergence doesn't make students wait 30–40 s
- Deterministic engine: the same topology and actions always replay identically

## Signature feature 2: modern canvas UX

- Infinite canvas: trackpad pinch-zoom, pan, minimap, snapping, align / distribute
- Device library sidebar with search; drag to place
- Smart cables: drag device-to-device, the next free compatible port is picked; port popover to override
- Inspector sidebar replaces modal windows; config edits apply live
- Terminal tabs and split panes, CLI syntax highlighting
- ⌘K command palette, full undo/redo, keyboard shortcuts for everything
- Multi-select, copy/paste devices with their config, labels, notes, area shapes

## Signature feature 3: built-in labs

- A lab = starting topology + Markdown instructions + automatic checks + optional hints
- Checks assert on simulated state: “R1 Gi0/0 is 10.0.0.1/24”, “PC1 can ping PC4”,
  “R2 has 2 OSPF neighbours”, “SW1 Fa0/5 is in VLAN 20”
- Progress bar, “Check my work”, per-item feedback, locked items students can't change
- About 20 bundled labs following CCNA topics, from cabling and IP addressing to NAT/PAT
- Lab file format documented so labs can be written by hand (in-app authoring comes later)

## Not in v1.0

| Deferred to v1.x | Out of scope |
|---|---|
| IPv6 addressing, SLAAC, static routes (**first priority for v1.1**, it's on the CCNA exam) | Importing Packet Tracer `.pkt` files |
| EtherChannel (LACP), HSRP | Bridging to real host networks |
| OSPF stub / NSSA areas, area ranges, authentication (basic multi-area works) | EIGRP, RIP, BGP |
| DNS / HTTP server, NTP, Syslog, SNMP | Windows / Linux builds |
| Telnet / SSH between simulated devices | Real-time multi-user collaboration |
| Wireless (WLC / APs) | AI tutor |
| `.pcap` export for Wireshark | |
| In-app lab authoring, iPad app | |

## Milestones

| # | Milestone | Done when |
|---|---|---|
| M0 ✅ | Foundation | Cargo workspace with event scheduler and tests, UniFFI bindings built into an XCFramework by one script, Xcode app links it, `.netrace` document opens and saves |
| M1 ✅ | Canvas | Place, cable (with port choice), rename, arrange and annotate devices: notes and areas saved in the document, marquee selection, copy/paste/duplicate keeping cables between copies, arrow-key nudging, semantic zoom, new devices placed in view, Space-drag panning; all undoable |
| M2 ✅ | Layer 2 + ARP/ICMP | Two PCs on a switch ping each other; visualiser v1 shows the ARP + ICMP exchange with the plain-language packet inspector |
| M2.5 ✅ | Canvas styling | Shapes (incl. *n*-sided polygons) with colours and interaction rules, device and note colours, distinct cable styles; smooth pinch / scroll zoom anchored at the pointer |
| M3 ✅ | CLI + routing | IOS parser, router interfaces, static routes, traceroute, subinterfaces, SVIs; multi-line paste, config snippets, configs saved in the file, duplicate with variations. Subinterfaces and SVIs other than Vlan1 are configured and saved now and carry traffic once trunks arrive (M4) |
| M4 ✅ | Switching | VLANs, trunks, STP, CDP/LLDP, port security; VLAN tinting and STP markers on the canvas; router-on-a-stick and SVI routing |
| M5 ✅ | Services | Minimal TCP ✅ (handshake, one request and response, close; PC `curl` and `telnet`; extended ACL ports and PAT act on it), DHCP ✅ (server, relay, APIPA, DORA in the trace) and ACLs ✅ (standard / extended, numbered / named, deciding line in the inspector, shield badges), NAT/PAT ✅ (static, pool, overload; before / after addresses and the rule in the inspector, inside / outside badges), OSPF ✅ (neighbours with mismatch reasons, DR / BDR election, LSDB, SPF with equal-cost paths, inter-area and default routes) |
| M5.5 ✅ | Protocol views | STP view (roles, reasons, blocked ports, reconvergence when a link fails), OSPF view (router IDs, neighbours, elections, routes and why, LSDB, cost and DR / BDR labels, adjacency lines labelled with their area, link-failure replay), VLAN view (members, hosts, trunks, gateways; the chosen VLAN lit on the canvas), ACL / NAT / DHCP / TCP in the packet inspector, BPDUs and OSPF hellos / LS Updates traced as packets when the simulation starts (no periodic repeats), NAT router badge, a router's routes shown along its cables on hover |
| M6 | Labs + polish | Lab engine ✅ (checks on simulated state, live progress, per-check feedback, hints), 20 labs ✅ (each proven by a scripted solution, see LABS.md); still to do: locked items, performance pass, beta |

## Quality targets

- Energy targets above are met at every milestone
- 100-device topology stays at 60 fps on an M1 MacBook Air
- Rust core builds and passes `cargo test` with no Xcode involved; protocol-level unit tests throughout
- Every CLI command in scope has golden-output tests (snapshot tests in Rust)
- FFI surface kept small and coarse-grained: commands in, snapshots/events out

## Open questions

- Distribution: Mac App Store, notarised DMG, or both
- Licence: open source, free, or paid
- Final product name (check “netrace” for trademark / App Store conflicts)
