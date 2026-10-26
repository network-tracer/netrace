# netrace

A native macOS network simulator for CCNA students. Rust simulation core,
SwiftUI/AppKit app. See [SCOPE.md](SCOPE.md) for the v1.0 plan and
[DESIGN.md](DESIGN.md) for the interface direction.

## Requirements

- macOS 15+, Xcode 26+
- Rust (stable) via rustup; `rustup target add x86_64-apple-darwin` for universal release builds
- [XcodeGen](https://github.com/yonaskolb/XcodeGen) (`brew install xcodegen`)

## Getting started

```bash
make run     # build the Rust core, generate the Xcode project, build and launch
make open    # same preparation, then open app/Netrace.xcodeproj in Xcode
make test    # Rust tests
make lint    # rustfmt + clippy
```

Choose **File → New from Lab…** (⌥⌘N) for 20 guided CCNA labs that check
your work as you go (see [LABS.md](LABS.md)). Or open `examples/services.netrace` for a DHCP and access list lab,
`examples/nat.netrace` for NAT and PAT, `examples/ospf.netrace` for OSPF,
`examples/vlans.netrace` for a VLAN, trunk and router-on-a-stick lab
(with a redundant link spanning tree blocks), `examples/two-lans.netrace` to
configure a router in its console and route between two LANs, `examples/office-lan.netrace` to try the simulation
(pings between configured PCs), `examples/starter.netrace` for a small
topology, or `examples/campus.netrace` for one that uses every device kind.

## Beta builds

`make beta` runs lint and the tests, then builds an optimised universal
(Apple silicon + Intel) Release app into `dist/netrace-VERSION-beta.zip`.
It's signed ad hoc until there's a Developer ID, so on another Mac the first
launch needs right-click → Open (or System Settings → Privacy & Security →
Open Anyway).

After changing anything in `crates/`, run `make core` before building in Xcode:
the app links the static library and Swift bindings that step produces.

## Layout

```
crates/
  netrace-sim/   deterministic event scheduler, virtual clock, seeded RNG
  netrace-doc/   project model + .netrace file format (JSON, validated on load)
  netrace-engine/ network simulation: VLANs, spanning tree, routing, ARP, ICMP, explained traces
  netrace-cli/   IOS-style command line: modes, parser, help, show output
  netrace-labs/  lab checks on simulated state, and the bundled labs
  netrace-ffi/   UniFFI bridge: the only API the Swift app sees
app/
  project.yml    XcodeGen spec (the .xcodeproj is generated, not committed)
  Netrace/       SwiftUI app: document, canvas, library, inspector, design tokens
  Netrace/Design/          tokens (Theme), device symbols, device catalog, icons, appearance
  Netrace/Workspace/       per-window interface state; preview data for unbuilt panels
  Netrace/Views/           Window (toolbar, menus, ⌘K), Sidebar, Canvas, Inspector, BottomPanel
  Netrace/Assets.xcassets/ Icons/*.imageset (SVG source of each icon), AppIcon
design/
  app-icon.svg   app icon source
scripts/
  build-core.sh        Rust → NetraceCore.xcframework + Generated/netrace_ffi.swift
  render-app-icon.sh   design/app-icon.svg → AppIcon.appiconset (needs librsvg)
  make-labs.py         generates labs/ (topologies, tasks, checks, solutions)
labs/            the 20 bundled labs and their solution scripts
examples/        sample projects
```

Generated and git-ignored: `app/Netrace.xcodeproj`, `app/Frameworks/`,
`app/Netrace/Core/Generated/`, `app/Support/Info.plist`, `app/Support/Netrace.entitlements`.

## How the pieces talk

The Swift app owns windows and rendering; the Rust core owns every rule.
Swift calls coarse commands on a `Project` object (`addDevice`, `moveDevice`,
`connect`, …) and redraws from the plain-value `ProjectSnapshot` the core returns.
Every edit registers its inverse with the window's undo manager, which is also
how macOS tracks unsaved changes.

A `.netrace` file is a package (folder) containing `project.json`.
