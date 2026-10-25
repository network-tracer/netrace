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
- PC prompt: `ipconfig`, `ipconfig /renew`, `ping`, `tracert`, `arp -a`

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
| Multi-area OSPF | EIGRP, RIP, BGP |
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
| M2 | Layer 2 + ARP/ICMP | Two PCs on a switch ping each other; visualiser v1 shows the ARP + ICMP exchange with the plain-language packet inspector |
| M3 | CLI + routing | IOS parser, router interfaces, static routes, traceroute, subinterfaces, SVIs; multi-line paste, config snippets, configs saved in the file, duplicate with variations |
| M4 | Switching | VLANs, trunks, STP, CDP/LLDP, port security |
| M5 | Services | DHCP, ACLs, NAT/PAT, OSPF |
| M6 | Labs + polish | Lab engine, 20 labs, performance pass, beta |

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
