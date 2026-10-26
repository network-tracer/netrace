#!/usr/bin/env python3
"""Generates the bundled labs in labs/: each NN-name.netrace package holds the
starting topology and the lab (brief, tasks, hints, checks), and each
NN-name.solution is the scripted answer the tests play through the real CLI
to prove the lab can be finished. Run from the repository root:

    python3 scripts/make-labs.py

Lab files can also be written by hand; LABS.md documents the format.
"""

import json
import os
import shutil

OUT = "labs"


class Topology:
    def __init__(self):
        self.devices = []
        self.links = []
        self.next_id = 1

    def _id(self):
        i = self.next_id
        self.next_id += 1
        return i

    def add(self, kind, name, x, y, ip=None, gateway=None, dhcp=False, config=None):
        d = {"id": self._id(), "kind": kind, "name": name, "position": {"x": float(x), "y": float(y)}}
        if ip:
            address, prefix = ip.split("/")
            d["ip"] = {"address": address, "prefix_len": int(prefix)}
            if gateway:
                d["ip"]["gateway"] = gateway
        if dhcp:
            d["dhcp"] = True
        if config:
            d["config"] = config
        self.devices.append(d)
        return d["id"]

    def id(self, name):
        return next(d["id"] for d in self.devices if d["name"] == name)

    def cable(self, a, ap, b, bp):
        self.links.append({"id": self._id(), "a": {"device": a, "port": ap}, "b": {"device": b, "port": bp},
                           "medium": "copper"})

    def project(self, lab):
        return {"format": "netrace", "version": 2, "next_id": self.next_id, "devices": self.devices,
                "links": self.links, "annotations": [], "lab": lab}


def iface(address=None, up=True, **extra):
    i = {"shutdown": not up}
    if address:
        a, p = address.split("/")
        i["address"] = {"address": a, "prefix_len": int(p)}
    i.update(extra)
    return i


def router(interfaces=None, **extra):
    c = {"interfaces": interfaces or {}}
    c.update(extra)
    return c


def task(title, checks, detail="", hint=None):
    t = {"title": title, "checks": checks}
    if detail:
        t["detail"] = detail
    if hint:
        t["hint"] = hint
    return t


def lab(title, topic, brief, tasks, locked=None):
    the_lab = {"title": title, "topic": topic, "brief": brief, "tasks": tasks}
    if locked:
        # Scenery the student can look at but not change.
        the_lab["locked"] = locked
    return the_lab


# Checks
def address(device, ip, interface=None):
    a, p = ip.split("/")
    c = {"check": "address", "device": device, "address": a, "prefix_len": int(p)}
    if interface:
        c["interface"] = interface
    return c


def ping(a, b, succeeds=True):
    c = {"check": "ping", "from": a, "to": b}
    if not succeeds:
        c["succeeds"] = False
    return c


def up(device, interface):
    return {"check": "interface_up", "device": device, "interface": interface}


def cabled(a, b, a_port=None, b_port=None):
    c = {"check": "cabled", "a": a, "b": b}
    if a_port:
        c["a_port"] = a_port
    if b_port:
        c["b_port"] = b_port
    return c


def route(device, prefix, source=None):
    n, p = prefix.split("/")
    c = {"check": "route", "device": device, "network": n, "prefix_len": int(p)}
    if source:
        c["source"] = source
    return c


def config(device, contains):
    return {"check": "config", "device": device, "contains": contains}


def gateway(device, gw):
    return {"check": "gateway", "device": device, "gateway": gw}


def vlan(device, v, name=None):
    c = {"check": "vlan", "device": device, "vlan": v}
    if name:
        c["name"] = name
    return c


def access(device, port, v):
    return {"check": "access_port", "device": device, "port": port, "vlan": v}


def trunk(device, port):
    return {"check": "trunk", "device": device, "port": port}


def ospf_neighbors(device, count):
    return {"check": "ospf_neighbors", "device": device, "count": count}


def stp_root(device, v):
    return {"check": "stp_root", "device": device, "vlan": v}


def dhcp_lease(device):
    return {"check": "dhcp_lease", "device": device}


LABS = []


def register(name, topology, the_lab, solution):
    LABS.append((name, topology.project(the_lab), solution.strip() + "\n"))


G = "GigabitEthernet0/"
F = "FastEthernet0/"


def two_pcs_on_a_switch(addressed):
    t = Topology()
    sw = t.add("l2_switch", "SW1", 480, 240)
    pc1 = t.add("pc", "PC1", 360, 420, ip="192.168.1.10/24" if addressed else None)
    pc2 = t.add("pc", "PC2", 600, 420, ip="192.168.1.11/24" if addressed else None)
    t.cable(sw, F + "1", pc1, "FastEthernet0")
    t.cable(sw, F + "2", pc2, "FastEthernet0")
    return t


def two_lans(router_config=None, pcs_have_gateway=True):
    t = Topology()
    r1 = t.add("router", "R1", 480, 160, config=router_config)
    s1 = t.add("l2_switch", "SW1", 300, 300)
    s2 = t.add("l2_switch", "SW2", 660, 300)
    pc1 = t.add("pc", "PC1", 300, 460, ip="192.168.1.10/24", gateway="192.168.1.1" if pcs_have_gateway else None)
    pc2 = t.add("pc", "PC2", 660, 460, ip="192.168.2.10/24", gateway="192.168.2.1" if pcs_have_gateway else None)
    t.cable(r1, G + "0", s1, "GigabitEthernet0/1")
    t.cable(r1, G + "1", s2, "GigabitEthernet0/1")
    t.cable(s1, F + "1", pc1, "FastEthernet0")
    t.cable(s2, F + "1", pc2, "FastEthernet0")
    return t


# 01 First ping
t = two_pcs_on_a_switch(addressed=False)
register("01-first-ping", t, lab(
    "Your first ping", "Getting started",
    "Two PCs share a switch but have no IP addresses yet. Give each one an address in **192.168.1.0/24**, "
    "then ping between them and watch the exchange in the Events tab.\n\n"
    "Select a PC and set its address in the Inspector.",
    [
        task("Address PC1 as 192.168.1.10/24", [address("PC1", "192.168.1.10/24")],
             hint="Select PC1, then in the Inspector's IP Configuration type 192.168.1.10 and mask 255.255.255.0."),
        task("Address PC2 as 192.168.1.11/24", [address("PC2", "192.168.1.11/24")]),
        task("Ping PC2 from PC1", [ping("PC1", "PC2")],
             detail="Double-click PC1 to open its console and type `ping 192.168.1.11`.",
             hint="The first reply may take longer: PC1 asks for PC2's MAC address with ARP first."),
    ]), """
@ip PC1 192.168.1.10/24
@ip PC2 192.168.1.11/24
""")

# 02 Cabling
t = Topology()
r1 = t.add("router", "R1", 480, 140, config=router({G + "0": iface("192.168.1.1/24")}))
sw = t.add("l2_switch", "SW1", 480, 280)
t.add("pc", "PC1", 360, 440, ip="192.168.1.10/24", gateway="192.168.1.1")
t.add("pc", "PC2", 600, 440, ip="192.168.1.11/24", gateway="192.168.1.1")
register("02-cabling", t, lab(
    "Cabling a small office", "Getting started",
    "Everything is configured, but nothing is plugged in. Cable both PCs to the switch and the switch to "
    "the router's **Gi0/0**, then check the PCs can reach their gateway.",
    [
        task("Cable PC1 and PC2 to SW1", [cabled("PC1", "SW1"), cabled("PC2", "SW1")],
             hint="Pick the Connect tool (C), then drag from a PC to the switch. Automatic picks the right cable."),
        task("Cable SW1 to R1 Gi0/0", [cabled("SW1", "R1", b_port="g0/0")],
             hint="Drag from SW1 to R1 and choose GigabitEthernet0/0 when asked for the router's port."),
        task("PC1 can ping its gateway", [ping("PC1", "192.168.1.1"), ping("PC2", "PC1")]),
    ]), """
@cable PC1 FastEthernet0 SW1 FastEthernet0/1
@cable PC2 FastEthernet0 SW1 FastEthernet0/2
@cable SW1 GigabitEthernet0/1 R1 GigabitEthernet0/0
""")

# 03 Default gateway
t = two_lans(pcs_have_gateway=False)
register("03-default-gateway", t, lab(
    "Routing between two LANs", "Routing",
    "R1 joins two LANs, but its interfaces have no addresses and are shut down, and the PCs have no "
    "default gateway. Make PC1 and PC2 reach each other through R1.\n\n"
    "Open R1's console (double-click it).",
    [
        task("R1 Gi0/0 is 192.168.1.1/24 and up", [address("R1", "192.168.1.1/24", "g0/0"), up("R1", "g0/0")],
             detail="`enable`, `configure terminal`, `interface g0/0`, `ip address 192.168.1.1 255.255.255.0`, `no shutdown`.",
             hint="Router interfaces start shut down. no shutdown brings the interface up."),
        task("R1 Gi0/1 is 192.168.2.1/24 and up", [address("R1", "192.168.2.1/24", "g0/1"), up("R1", "g0/1")]),
        task("Each PC uses R1 as its gateway", [gateway("PC1", "192.168.1.1"), gateway("PC2", "192.168.2.1")],
             hint="A PC sends anything outside its own network to its default gateway."),
        task("PC1 can ping PC2", [ping("PC1", "PC2")]),
    ]), """
R1: enable
R1: configure terminal
R1: interface g0/0
R1: ip address 192.168.1.1 255.255.255.0
R1: no shutdown
R1: interface g0/1
R1: ip address 192.168.2.1 255.255.255.0
R1: no shutdown
R1: end
@ip PC1 192.168.1.10/24 192.168.1.1
@ip PC2 192.168.2.10/24 192.168.2.1
""")

# 04 Device basics
t = Topology()
t.add("router", "Router", 480, 240)
register("04-device-basics", t, lab(
    "Securing a new router", "Device management",
    "A new router needs a name, an enable secret, a console password and a login banner before it goes "
    "into the rack. Do it all from its console.",
    [
        task("Name the router HQ", [config("HQ", "hostname HQ")],
             detail="In global configuration: `hostname HQ`. The prompt changes straight away."),
        task("Protect privileged mode with an enable secret", [config("HQ", "enable secret")],
             hint="enable secret stores a hash; enable password doesn't, so prefer secret."),
        task("Require a password on the console", [config("HQ", "line con 0"), config("HQ", " login")],
             detail="`line console 0`, `password cisco`, `login`."),
        task("Encrypt plain-text passwords", [config("HQ", "service password-encryption")]),
        task("Add a login banner", [config("HQ", "banner motd")],
             hint="banner motd # Authorised access only #  (any character can delimit the text)."),
    ]), """
Router: enable
Router: configure terminal
Router: hostname HQ
HQ: enable secret class
HQ: line console 0
HQ: password cisco
HQ: login
HQ: exit
HQ: service password-encryption
HQ: banner motd # Authorised access only #
HQ: end
""")

# 05 Static routes
t = Topology()
r1 = t.add("router", "R1", 360, 200, config=router({
    G + "0": iface("192.168.1.1/24"), G + "1": iface("10.0.0.1/30")}))
r2 = t.add("router", "R2", 600, 200, config=router({
    G + "0": iface("192.168.2.1/24"), G + "1": iface("10.0.0.2/30")}))
pc1 = t.add("pc", "PC1", 240, 380, ip="192.168.1.10/24", gateway="192.168.1.1")
pc2 = t.add("pc", "PC2", 720, 380, ip="192.168.2.10/24", gateway="192.168.2.1")
t.cable(r1, G + "1", r2, G + "1")
t.cable(r1, G + "0", pc1, "FastEthernet0")
t.cable(r2, G + "0", pc2, "FastEthernet0")
register("05-static-routes", t, lab(
    "Static routes", "Routing",
    "R1 and R2 are addressed and cabled, but each only knows its own networks. Add a static route on each "
    "so PC1 and PC2 can reach each other. Check `show ip route` as you go.",
    [
        task("R1 has a route to 192.168.2.0/24", [route("R1", "192.168.2.0/24", "static")],
             detail="`ip route 192.168.2.0 255.255.255.0 10.0.0.2`",
             hint="The next hop is R2's address on the link between them."),
        task("R2 has a route to 192.168.1.0/24", [route("R2", "192.168.1.0/24", "static")],
             hint="Without the return route, pings reach PC2 but the replies can't come back."),
        task("PC1 can ping PC2", [ping("PC1", "PC2")]),
    ]), """
R1: enable
R1: configure terminal
R1: ip route 192.168.2.0 255.255.255.0 10.0.0.2
R1: end
R2: enable
R2: configure terminal
R2: ip route 192.168.1.0 255.255.255.0 10.0.0.1
R2: end
""")

# 06 Default route
t = Topology()
r1 = t.add("router", "R1", 360, 220, config=router({
    G + "0": iface("192.168.1.1/24"), G + "1": iface("203.0.113.2/30")}))
isp = t.add("router", "ISP", 620, 220, config=router({
    G + "0": iface("198.51.100.1/24"), G + "1": iface("203.0.113.1/30")},
    routes=[{"network": "192.168.1.0", "prefix_len": 24, "target": {"next_hop": "203.0.113.2"}, "distance": 1}]))
pc1 = t.add("pc", "PC1", 240, 400, ip="192.168.1.10/24", gateway="192.168.1.1")
web = t.add("server", "WEB", 740, 400, ip="198.51.100.10/24", gateway="198.51.100.1")
t.cable(r1, G + "1", isp, G + "1")
t.cable(r1, G + "0", pc1, "FastEthernet0")
t.cable(isp, G + "0", web, "GigabitEthernet0")
register("06-default-route", t, lab(
    "A default route to the ISP", "Routing",
    "R1 connects the office to an ISP. Rather than a route for every network on the internet, give it one "
    "**default route** that sends everything it doesn't know to the ISP.",
    [
        task("R1 has a default route", [route("R1", "0.0.0.0/0", "static")],
             detail="`ip route 0.0.0.0 0.0.0.0 203.0.113.1`",
             hint="0.0.0.0/0 matches every address, but only when nothing more specific does."),
        task("PC1 can reach WEB", [ping("PC1", "WEB")]),
    ], locked=["ISP", "WEB"]), """
R1: enable
R1: configure terminal
R1: ip route 0.0.0.0 0.0.0.0 203.0.113.1
R1: end
""")

# 07 VLANs
t = Topology()
sw = t.add("l2_switch", "SW1", 480, 200)
for i, (name, ip) in enumerate([("PC1", "192.168.10.11/24"), ("PC2", "192.168.20.11/24"),
                                 ("PC3", "192.168.10.12/24"), ("PC4", "192.168.20.12/24")]):
    pc = t.add("pc", name, 240 + i * 160, 380, ip=ip)
    t.cable(sw, F + str(i + 1), pc, "FastEthernet0")
register("07-vlans", t, lab(
    "Splitting a switch with VLANs", "Switching",
    "Sales (PC1, PC3) and Staff (PC2, PC4) share SW1. Put them in separate VLANs so each group is its own "
    "broadcast domain: VLAN 10 Sales on Fa0/1 and Fa0/3, VLAN 20 Staff on Fa0/2 and Fa0/4.",
    [
        task("Create VLAN 10 Sales and VLAN 20 Staff", [vlan("SW1", 10, "Sales"), vlan("SW1", 20, "Staff")],
             detail="`vlan 10`, `name Sales`, then the same for 20."),
        task("Fa0/1 and Fa0/3 are in VLAN 10", [access("SW1", "Fa0/1", 10), access("SW1", "Fa0/3", 10)],
             detail="`interface range fa0/1, fa0/3`, `switchport mode access`, `switchport access vlan 10`."),
        task("Fa0/2 and Fa0/4 are in VLAN 20", [access("SW1", "Fa0/2", 20), access("SW1", "Fa0/4", 20)]),
        task("Same VLAN talks, different VLANs don't", [ping("PC1", "PC3"), ping("PC2", "PC4"), ping("PC1", "PC2", False)],
             hint="Different VLANs need a router to talk; that's the router-on-a-stick lab."),
    ]), """
SW1: enable
SW1: configure terminal
SW1: vlan 10
SW1: name Sales
SW1: vlan 20
SW1: name Staff
SW1: exit
SW1: interface range fa0/1, fa0/3
SW1: switchport mode access
SW1: switchport access vlan 10
SW1: interface range fa0/2, fa0/4
SW1: switchport mode access
SW1: switchport access vlan 20
SW1: end
""")

# 08 Trunks
t = Topology()
s1 = t.add("l2_switch", "SW1", 340, 200, config={"vlans": {"10": {"name": "Sales"}}, "interfaces": {
    F + "1": {"switchport": {"access_vlan": 10}}}})
s2 = t.add("l2_switch", "SW2", 620, 200, config={"vlans": {"10": {"name": "Sales"}}, "interfaces": {
    F + "1": {"switchport": {"access_vlan": 10}}}})
pc1 = t.add("pc", "PC1", 340, 380, ip="192.168.10.11/24")
pc3 = t.add("pc", "PC3", 620, 380, ip="192.168.10.12/24")
t.cable(s1, "GigabitEthernet0/1", s2, "GigabitEthernet0/1")
t.cable(s1, F + "1", pc1, "FastEthernet0")
t.cable(s2, F + "1", pc3, "FastEthernet0")
register("08-trunks", t, lab(
    "Carrying VLANs between switches", "Switching",
    "PC1 and PC3 are both in VLAN 10, but on different switches. The link between the switches is an access "
    "port in VLAN 1, so VLAN 10 can't cross it. Make it a **trunk**.",
    [
        task("SW1 Gi0/1 is a trunk", [trunk("SW1", "g0/1")], detail="`interface g0/1`, `switchport mode trunk`."),
        task("SW2 Gi0/1 is a trunk", [trunk("SW2", "g0/1")]),
        task("PC1 can ping PC3", [ping("PC1", "PC3")],
             hint="Frames on the trunk carry an 802.1Q tag saying which VLAN they belong to. Look for it in the inspector."),
    ]), """
SW1: enable
SW1: configure terminal
SW1: interface g0/1
SW1: switchport mode trunk
SW1: end
SW2: enable
SW2: configure terminal
SW2: interface g0/1
SW2: switchport mode trunk
SW2: end
""")

# 09 Router on a stick
t = Topology()
r1 = t.add("router", "R1", 480, 120, config=router({G + "0": iface()}))
sw = t.add("l2_switch", "SW1", 480, 260, config={
    "vlans": {"10": {"name": "Sales"}, "20": {"name": "Staff"}},
    "interfaces": {F + "1": {"switchport": {"access_vlan": 10}}, F + "2": {"switchport": {"access_vlan": 20}}}})
pc1 = t.add("pc", "PC1", 360, 420, ip="192.168.10.11/24", gateway="192.168.10.1")
pc2 = t.add("pc", "PC2", 600, 420, ip="192.168.20.11/24", gateway="192.168.20.1")
t.cable(r1, G + "0", sw, "GigabitEthernet0/1")
t.cable(sw, F + "1", pc1, "FastEthernet0")
t.cable(sw, F + "2", pc2, "FastEthernet0")
register("09-router-on-a-stick", t, lab(
    "Routing between VLANs (router on a stick)", "Switching",
    "PC1 (VLAN 10) and PC2 (VLAN 20) need to talk. Trunk SW1's uplink to R1, then give R1 one "
    "**subinterface** per VLAN on Gi0/0: .10 is 192.168.10.1/24 and .20 is 192.168.20.1/24.",
    [
        task("SW1 Gi0/1 is a trunk", [trunk("SW1", "g0/1")]),
        task("R1 Gi0/0.10 routes VLAN 10", [address("R1", "192.168.10.1/24", "g0/0.10"), config("R1", "encapsulation dot1Q 10")],
             detail="`interface g0/0.10`, `encapsulation dot1Q 10`, `ip address 192.168.10.1 255.255.255.0`.",
             hint="The encapsulation line tells the subinterface which VLAN tag it handles."),
        task("R1 Gi0/0.20 routes VLAN 20", [address("R1", "192.168.20.1/24", "g0/0.20"), config("R1", "encapsulation dot1Q 20")]),
        task("PC1 can ping PC2", [ping("PC1", "PC2")]),
    ]), """
SW1: enable
SW1: configure terminal
SW1: interface g0/1
SW1: switchport mode trunk
SW1: end
R1: enable
R1: configure terminal
R1: interface g0/0.10
R1: encapsulation dot1Q 10
R1: ip address 192.168.10.1 255.255.255.0
R1: interface g0/0.20
R1: encapsulation dot1Q 20
R1: ip address 192.168.20.1 255.255.255.0
R1: end
""")

# 10 STP root
t = Topology()
s1 = t.add("l2_switch", "SW1", 480, 140)
s2 = t.add("l2_switch", "SW2", 320, 340)
s3 = t.add("l2_switch", "SW3", 640, 340)
t.cable(s1, "GigabitEthernet0/1", s2, "GigabitEthernet0/1")
t.cable(s1, "GigabitEthernet0/2", s3, "GigabitEthernet0/1")
t.cable(s2, "GigabitEthernet0/2", s3, "GigabitEthernet0/2")
register("10-stp-root", t, lab(
    "Choosing the spanning-tree root", "Switching",
    "Three switches in a triangle form a loop, so spanning tree blocks one port. The root bridge was chosen "
    "by the lowest MAC address, not by design. Make **SW3** the root for VLAN 1 and watch which port blocks.\n\n"
    "Open the STP tab to see each port's role and why.",
    [
        task("SW3 is the root bridge for VLAN 1", [stp_root("SW3", 1)],
             detail="On SW3: `spanning-tree vlan 1 root primary` (or `spanning-tree vlan 1 priority 4096`).",
             hint="The lowest bridge ID wins: priority first, then MAC address."),
    ]), """
SW3: enable
SW3: configure terminal
SW3: spanning-tree vlan 1 root primary
SW3: end
""")

# 11 OSPF single area
def triangle(ospf=None, mistakes=None):
    t = Topology()
    names = ["R1", "R2", "R3"]
    pos = [(340, 300), (480, 140), (620, 300)]
    ifs = [{}, {}, {}]
    for (a, ap, b, bp, net) in [(0, "0", 1, "0", "10.0.12"), (1, "1", 2, "1", "10.0.23"), (2, "2", 0, "2", "10.0.13")]:
        ifs[a][G + ap] = iface(f"{net}.{a + 1}/24")
        ifs[b][G + bp] = iface(f"{net}.{b + 1}/24")
    ifs[0][G + "3"] = iface("192.168.1.1/24")
    ifs[2][G + "3"] = iface("192.168.3.1/24")
    for i in range(3):
        n = i + 1
        ifs[i]["Loopback0"] = iface(f"{n}.{n}.{n}.{n}/32")
    ids = []
    for i in range(3):
        cfg = router(ifs[i])
        if ospf and ospf[i]:
            cfg["ospf"] = [ospf[i]]
        if mistakes and mistakes.get(i):
            for k, v in mistakes[i].items():
                cfg["interfaces"][k].update(v)
        ids.append(t.add("router", names[i], *pos[i], config=cfg))
    for (a, ap, b, bp) in [(0, "0", 1, "0"), (1, "1", 2, "1"), (2, "2", 0, "2")]:
        t.cable(ids[a], G + ap, ids[b], G + bp)
    pc1 = t.add("pc", "PC1", 220, 440, ip="192.168.1.10/24", gateway="192.168.1.1")
    pc3 = t.add("pc", "PC3", 740, 440, ip="192.168.3.10/24", gateway="192.168.3.1")
    t.cable(ids[0], G + "3", pc1, "FastEthernet0")
    t.cable(ids[2], G + "3", pc3, "FastEthernet0")
    return t


t = triangle()
register("11-ospf-single-area", t, lab(
    "OSPF in one area", "Routing",
    "Three routers in a triangle, all addressed, no routing protocol. Turn on **OSPF** in area 0 on all "
    "three so they learn every network on their own.\n\nOpen the OSPF tab to watch neighbours come up and see why each route was chosen.",
    [
        task("R1 runs OSPF with both neighbours", [ospf_neighbors("R1", 2)],
             detail="`router ospf 1`, `network 0.0.0.0 255.255.255.255 area 0`.",
             hint="A neighbour only comes up when both ends run OSPF; R1 alone shows none."),
        task("R2 and R3 run OSPF too", [ospf_neighbors("R2", 2), ospf_neighbors("R3", 2)]),
        task("R1 learns PC3's network from OSPF", [route("R1", "192.168.3.0/24", "ospf")]),
        task("PC1 can ping PC3", [ping("PC1", "PC3")]),
    ]), "\n".join(
        f"{r}: {c}" for r in ["R1", "R2", "R3"] for c in
        ["enable", "configure terminal", "router ospf 1", "network 0.0.0.0 255.255.255.255 area 0", "end"]))

# 12 DHCP server
t = Topology()
r1 = t.add("router", "R1", 480, 140, config=router({G + "0": iface("192.168.1.1/24")}))
sw = t.add("l2_switch", "SW1", 480, 280)
pc1 = t.add("pc", "PC1", 360, 440, dhcp=True)
pc2 = t.add("pc", "PC2", 600, 440, dhcp=True)
t.cable(r1, G + "0", sw, "GigabitEthernet0/1")
t.cable(sw, F + "1", pc1, "FastEthernet0")
t.cable(sw, F + "2", pc2, "FastEthernet0")
register("12-dhcp-server", t, lab(
    "A DHCP server on the router", "Services",
    "PC1 and PC2 are set to DHCP, but nothing answers, so they give themselves 169.254 addresses. Make R1 "
    "hand out 192.168.1.0/24, keeping .1 to .10 for fixed devices.",
    [
        task("Keep 192.168.1.1–10 out of the pool", [config("R1", "ip dhcp excluded-address 192.168.1.1 192.168.1.10")]),
        task("Create a pool for the LAN", [config("R1", "ip dhcp pool"), config("R1", "network 192.168.1.0 255.255.255.0"),
                                          config("R1", "default-router 192.168.1.1")],
             detail="`ip dhcp pool LAN`, `network 192.168.1.0 255.255.255.0`, `default-router 192.168.1.1`.",
             hint="Without default-router the PCs get an address but no gateway."),
        task("Both PCs get a lease", [dhcp_lease("PC1"), dhcp_lease("PC2")],
             detail="Run `ipconfig /renew` on a PC, or watch the Events tab: Discover, Offer, Request, Acknowledge."),
        task("PC1 can ping its gateway", [ping("PC1", "192.168.1.1")]),
    ]), """
R1: enable
R1: configure terminal
R1: ip dhcp excluded-address 192.168.1.1 192.168.1.10
R1: ip dhcp pool LAN
R1: network 192.168.1.0 255.255.255.0
R1: default-router 192.168.1.1
R1: end
""")

# 13 DHCP relay
t = Topology()
r1 = t.add("router", "R1", 480, 160, config=router({G + "0": iface("192.168.1.1/24"), G + "1": iface("192.168.2.1/24")}))
s1 = t.add("l2_switch", "SW1", 300, 300)
pc1 = t.add("pc", "PC1", 300, 460, dhcp=True)
srv = t.add("router", "DHCP", 700, 300, config=router({G + "0": iface("192.168.2.10/24")},
    routes=[{"network": "0.0.0.0", "prefix_len": 0, "target": {"next_hop": "192.168.2.1"}, "distance": 1}],
    dhcp_pools=[{"name": "LAN1", "network": ["192.168.1.0", 24], "default_router": "192.168.1.1"}],
    dhcp_excluded=[["192.168.1.1", "192.168.1.10"]]))
t.cable(r1, G + "0", s1, "GigabitEthernet0/1")
t.cable(s1, F + "1", pc1, "FastEthernet0")
t.cable(r1, G + "1", srv, G + "0")
register("13-dhcp-relay", t, lab(
    "Reaching a DHCP server on another network", "Services",
    "A central router called DHCP already has a pool for 192.168.1.0/24, but PC1's Discover is a broadcast, "
    "and routers don't forward broadcasts. Make R1 **relay** it with a helper address.",
    [
        task("R1 Gi0/0 relays DHCP to 192.168.2.10", [config("R1", "ip helper-address 192.168.2.10")],
             detail="`interface g0/0`, `ip helper-address 192.168.2.10`.",
             hint="The helper goes on the interface facing the clients, where the broadcasts arrive."),
        task("PC1 gets a lease", [dhcp_lease("PC1")]),
        task("PC1 can ping the DHCP server", [ping("PC1", "192.168.2.10")]),
    ], locked=["DHCP"]), """
R1: enable
R1: configure terminal
R1: interface g0/0
R1: ip helper-address 192.168.2.10
R1: end
""")

# 14 Standard ACL
t = two_lans(router_config=router({G + "0": iface("192.168.1.1/24"), G + "1": iface("192.168.2.1/24")}))
pc3 = t.add("pc", "PC3", 420, 460, ip="192.168.1.20/24", gateway="192.168.1.1")
t.cable(t.id("SW1"), F + "2", pc3, "FastEthernet0")
register("14-acl-standard", t, lab(
    "A standard access list", "Security",
    "PC1 and PC3 are on the office LAN; PC2's LAN holds payroll. Only PC1 may reach it. Write a **standard "
    "ACL** that permits PC1 and denies the rest, and apply it outbound on R1 Gi0/1, close to the destination.",
    [
        task("Access list 10 permits PC1", [config("R1", "access-list 10 permit 192.168.1.10")],
             detail="`access-list 10 permit host 192.168.1.10`",
             hint="Every ACL ends with an invisible deny any, so you don't need to write the deny."),
        task("It's applied out on Gi0/1", [config("R1", "ip access-group 10 out")],
             detail="`interface g0/1`, `ip access-group 10 out`."),
        task("PC1 reaches payroll, PC3 doesn't", [ping("PC1", "PC2"), ping("PC3", "PC2", False)],
             hint="Click the dropped event in the Events tab: the inspector shows the line that matched."),
    ]), """
R1: enable
R1: configure terminal
R1: access-list 10 permit host 192.168.1.10
R1: interface g0/1
R1: ip access-group 10 out
R1: end
""")

# 15 Extended ACL
t = Topology()
r1 = t.add("router", "R1", 480, 160, config=router({G + "0": iface("192.168.1.1/24"), G + "1": iface("192.168.2.1/24")}))
s1 = t.add("l2_switch", "SW1", 320, 300)
pc1 = t.add("pc", "PC1", 240, 460, ip="192.168.1.10/24", gateway="192.168.1.1")
pc2 = t.add("pc", "PC2", 400, 460, ip="192.168.1.11/24", gateway="192.168.1.1")
srv = t.add("server", "SRV1", 660, 300, ip="192.168.2.10/24", gateway="192.168.2.1")
t.cable(r1, G + "0", s1, "GigabitEthernet0/1")
t.cable(s1, F + "1", pc1, "FastEthernet0")
t.cable(s1, F + "2", pc2, "FastEthernet0")
t.cable(r1, G + "1", srv, "GigabitEthernet0")
register("15-acl-extended", t, lab(
    "An extended access list", "Security",
    "Stop PC2 from pinging SRV1 without blocking anything else. An **extended ACL** can match the protocol "
    "and the destination as well as the source, so apply it inbound on Gi0/0, close to PC2.",
    [
        task("ACL 101 denies pings from PC2 to SRV1", [config("R1", "access-list 101 deny icmp host 192.168.1.11 host 192.168.2.10")],
             detail="`access-list 101 deny icmp host 192.168.1.11 host 192.168.2.10`"),
        task("…and permits everything else", [config("R1", "access-list 101 permit ip any any")],
             hint="Without this line the implicit deny at the end blocks all traffic."),
        task("It's applied in on Gi0/0", [config("R1", "ip access-group 101 in")]),
        task("PC2 can't ping SRV1; PC1 can", [ping("PC2", "SRV1", False), ping("PC1", "SRV1")]),
    ]), """
R1: enable
R1: configure terminal
R1: access-list 101 deny icmp host 192.168.1.11 host 192.168.2.10
R1: access-list 101 permit ip any any
R1: interface g0/0
R1: ip access-group 101 in
R1: end
""")


# 16 PAT and 17 static NAT share a topology.
def nat_lab():
    t = Topology()
    r1 = t.add("router", "R1", 400, 180, config=router({G + "0": iface("192.168.1.1/24"), G + "1": iface("203.0.113.1/24")},
        routes=[{"network": "0.0.0.0", "prefix_len": 0, "target": {"next_hop": "203.0.113.2"}, "distance": 1}]))
    isp = t.add("router", "ISP", 660, 180, config=router({G + "1": iface("203.0.113.2/24"), G + "0": iface("198.51.100.1/24")}))
    sw = t.add("l2_switch", "SW1", 400, 320)
    pc1 = t.add("pc", "PC1", 280, 460, ip="192.168.1.10/24", gateway="192.168.1.1")
    pc2 = t.add("pc", "PC2", 400, 460, ip="192.168.1.11/24", gateway="192.168.1.1")
    srv = t.add("server", "SRV1", 520, 460, ip="192.168.1.20/24", gateway="192.168.1.1")
    web = t.add("server", "WEB", 660, 340, ip="198.51.100.10/24", gateway="198.51.100.1")
    t.cable(r1, G + "0", sw, "GigabitEthernet0/1")
    t.cable(r1, G + "1", isp, G + "1")
    t.cable(isp, G + "0", web, "GigabitEthernet0")
    t.cable(sw, F + "1", pc1, "FastEthernet0")
    t.cable(sw, F + "2", pc2, "FastEthernet0")
    t.cable(sw, F + "3", srv, "GigabitEthernet0")
    return t


register("16-nat-pat", nat_lab(), lab(
    "Sharing one public address (PAT)", "Services",
    "The ISP only routes 203.0.113.0/24, so replies to the office's private addresses get lost. Use **PAT** "
    "on R1 so the whole LAN shares Gi0/1's address.",
    [
        task("Mark the inside and outside interfaces", [config("R1", "ip nat inside"), config("R1", "ip nat outside")],
             detail="Gi0/0: `ip nat inside`. Gi0/1: `ip nat outside`."),
        task("Translate the LAN to Gi0/1's address", [config("R1", "ip nat inside source list 1 interface GigabitEthernet0/1 overload")],
             detail="`access-list 1 permit 192.168.1.0 0.0.0.255`, then `ip nat inside source list 1 interface g0/1 overload`.",
             hint="overload is what lets many hosts share one address, told apart by port."),
        task("PC1 and PC2 reach WEB", [ping("PC1", "WEB"), ping("PC2", "WEB")],
             hint="show ip nat translations lists each flow; the inspector shows each packet's addresses before and after."),
    ], locked=["ISP", "WEB"]), """
R1: enable
R1: configure terminal
R1: interface g0/0
R1: ip nat inside
R1: interface g0/1
R1: ip nat outside
R1: exit
R1: access-list 1 permit 192.168.1.0 0.0.0.255
R1: ip nat inside source list 1 interface g0/1 overload
R1: end
""")

register("17-static-nat", nat_lab(), lab(
    "Publishing a server (static NAT)", "Services",
    "SRV1 has a private address, so nothing on the internet can reach it. Give it the public address "
    "**203.0.113.10** with static NAT, so WEB (on the internet) can reach it.",
    [
        task("Mark the inside and outside interfaces", [config("R1", "ip nat inside"), config("R1", "ip nat outside")]),
        task("Map SRV1 to 203.0.113.10", [config("R1", "ip nat inside source static 192.168.1.20 203.0.113.10")],
             detail="`ip nat inside source static 192.168.1.20 203.0.113.10`",
             hint="R1 answers ARP for 203.0.113.10 on the outside, so the ISP can deliver to it."),
        task("WEB can reach SRV1 at its public address", [ping("WEB", "203.0.113.10")]),
    ], locked=["ISP", "WEB"]), """
R1: enable
R1: configure terminal
R1: interface g0/0
R1: ip nat inside
R1: interface g0/1
R1: ip nat outside
R1: exit
R1: ip nat inside source static 192.168.1.20 203.0.113.10
R1: end
""")

# 18 Port security
t = Topology()
sw = t.add("l2_switch", "SW1", 480, 220)
pc1 = t.add("pc", "PC1", 360, 400, ip="192.168.1.10/24")
pc2 = t.add("pc", "PC2", 600, 400, ip="192.168.1.11/24")
t.cable(sw, F + "1", pc1, "FastEthernet0")
t.cable(sw, F + "2", pc2, "FastEthernet0")
register("18-port-security", t, lab(
    "Port security", "Security",
    "Lock SW1 Fa0/1 to the one PC that belongs there: allow a single MAC address, learn it automatically, "
    "and shut the port down if anything else is plugged in.",
    [
        task("Fa0/1 is an access port with port security on", [access("SW1", "Fa0/1", 1), config("SW1", "switchport port-security")],
             detail="`interface fa0/1`, `switchport mode access`, `switchport port-security`.",
             hint="Port security only works on access (or trunk) ports, not dynamic ones."),
        task("It learns the MAC address (sticky)", [config("SW1", "switchport port-security mac-address sticky")]),
        task("PC1 still works", [ping("PC1", "PC2")]),
    ]), """
SW1: enable
SW1: configure terminal
SW1: interface fa0/1
SW1: switchport mode access
SW1: switchport port-security
SW1: switchport port-security mac-address sticky
SW1: end
""")

# 19 OSPF troubleshooting
ospf_all = {"process_id": 1, "networks": [{"address": "0.0.0.0", "wildcard": "255.255.255.255", "area": 0}]}
r2_wrong = {"process_id": 1, "networks": [{"address": "10.0.12.0", "wildcard": "0.0.0.255", "area": 0},
                                          {"address": "10.0.23.0", "wildcard": "0.0.0.255", "area": 1},
                                          {"address": "2.2.2.2", "wildcard": "0.0.0.0", "area": 0}]}
t = triangle(ospf=[ospf_all, r2_wrong, ospf_all],
             mistakes={0: {G + "2": {"ospf": {"hello": 5}}}})
register("19-ospf-troubleshooting", t, lab(
    "Fixing OSPF", "Routing",
    "OSPF is configured on all three routers, but two adjacencies never come up. Find the two mistakes and "
    "fix them. The OSPF tab names what's wrong with each neighbour; `show ip ospf interface` agrees.",
    [
        task("R2 and R3 become neighbours", [ospf_neighbors("R2", 2)],
             hint="Hellos carry the area ID, and both ends must agree."),
        task("R1 and R3 become neighbours", [ospf_neighbors("R1", 2), ospf_neighbors("R3", 2)],
             hint="Hello and dead timers must match exactly on both ends of a link."),
        task("PC1 can ping PC3", [ping("PC1", "PC3")]),
    ]), """
R2: enable
R2: configure terminal
R2: router ospf 1
R2: no network 10.0.23.0 0.0.0.255 area 1
R2: network 10.0.23.0 0.0.0.255 area 0
R2: end
R1: enable
R1: configure terminal
R1: interface g0/2
R1: no ip ospf hello-interval
R1: end
""")

# 20 Layer 3 switch
t = Topology()
l3 = t.add("l3_switch", "DSW1", 480, 200)
pc1 = t.add("pc", "PC1", 360, 380, ip="192.168.10.11/24", gateway="192.168.10.1")
pc2 = t.add("pc", "PC2", 600, 380, ip="192.168.20.11/24", gateway="192.168.20.1")
t.cable(l3, "GigabitEthernet1/0/1", pc1, "FastEthernet0")
t.cable(l3, "GigabitEthernet1/0/2", pc2, "FastEthernet0")
register("20-layer3-switch", t, lab(
    "Routing on a layer 3 switch", "Switching",
    "A multilayer switch can route between VLANs itself, with no router: one **SVI** (interface Vlan N) per "
    "VLAN as the gateway, and `ip routing` turned on.",
    [
        task("VLANs 10 and 20 with a port each", [vlan("DSW1", 10), vlan("DSW1", 20),
                                                  access("DSW1", "Gi1/0/1", 10), access("DSW1", "Gi1/0/2", 20)]),
        task("SVIs are the gateways", [address("DSW1", "192.168.10.1/24", "Vlan10"), address("DSW1", "192.168.20.1/24", "Vlan20")],
             detail="`interface vlan 10`, `ip address 192.168.10.1 255.255.255.0`, `no shutdown`."),
        task("Routing is on", [config("DSW1", "ip routing")],
             hint="Without ip routing the switch answers pings to its SVIs but won't forward between them."),
        task("PC1 can ping PC2", [ping("PC1", "PC2")]),
    ]), """
DSW1: enable
DSW1: configure terminal
DSW1: vlan 10
DSW1: vlan 20
DSW1: exit
DSW1: interface g1/0/1
DSW1: switchport mode access
DSW1: switchport access vlan 10
DSW1: interface g1/0/2
DSW1: switchport mode access
DSW1: switchport access vlan 20
DSW1: interface vlan 10
DSW1: ip address 192.168.10.1 255.255.255.0
DSW1: no shutdown
DSW1: interface vlan 20
DSW1: ip address 192.168.20.1 255.255.255.0
DSW1: no shutdown
DSW1: exit
DSW1: ip routing
DSW1: end
""")


def main():
    if os.path.isdir(OUT):
        shutil.rmtree(OUT)
    os.makedirs(OUT)
    for name, project, solution in LABS:
        os.makedirs(f"{OUT}/{name}.netrace")
        with open(f"{OUT}/{name}.netrace/project.json", "w") as f:
            json.dump(project, f, indent=2)
            f.write("\n")
        with open(f"{OUT}/{name}.solution", "w") as f:
            f.write(solution)
    print(f"{len(LABS)} labs in {OUT}/")


main()


# The showcase network the intro tour walks through: "The Shadow Network",
# a multi-agency VLAN operation with every feature from M1-M6 configured
# and working. Written to examples/tour.netrace.
def tour():
    t = Topology()
    any_ = {"address": "0.0.0.0", "wildcard": "255.255.255.255"}
    L3 = "GigabitEthernet1/0/"
    agencies = [  # VLAN, name, switch, centre of its zone, swatch, hosts
        (10, "CYBERSECURITY", "SW-CYBER", (460, 270), "blue", [("pc", "IDS-IPS"), ("pc", "SOC-1")]),
        (20, "COUNTER-TERROR", "SW-CT", (1140, 270), "red", [("pc", "THREAT-MON"), ("laptop", "SECURE-LINK")]),
        (30, "INTERNAL-AFFAIRS", "SW-IA", (460, 620), "yellow", [("pc", "AUDITOR"), ("server", "PERSONNEL")]),
        (40, "FIELD-OPS", "SW-FIELD", (1140, 620), "green", [("laptop", "TAC-TEAM"), ("pc", "GPS-TRACK")]),
    ]
    vlans = {str(v): {"name": n} for v, n, *_ in agencies}
    vlans.update({"50": {"name": "DATA-CENTER"}, "99": {"name": "WATCHERS"}})
    trunk_ = {"switchport": {"mode": "trunk"}}

    # The edge: the ISP's cloud and a firewall doing PAT and originating
    # the default route into OSPF.
    isp = t.add("cloud", "ISP", 735, 115, config=router({
        "Ethernet0": iface("203.0.113.2/30"), "Ethernet1": iface("198.51.100.1/24")}))
    intel = t.add("server", "THREAT-INTEL", 865, 115, ip="198.51.100.10/24", gateway="198.51.100.1")
    fw = t.add("firewall", "FW", 800, 245, config=router({
        "GigabitEthernet1/1": iface("203.0.113.1/30", nat="outside", description="ISP"),
        "GigabitEthernet1/2": iface("10.0.0.1/30", nat="inside", description="CORE"),
    },
        routes=[{"network": "0.0.0.0", "prefix_len": 0, "target": {"next_hop": "203.0.113.2"}, "distance": 1}],
        ospf=[{"process_id": 1, "router_id": "1.1.1.1", "default_originate": True,
               "networks": [{"address": "10.0.0.0", "wildcard": "0.255.255.255", "area": 0}]}],
        access_lists=[{"name": "1", "kind": "standard", "remarks": ["Agency networks allowed out"], "entries": [
            {"sequence": 10, "action": "permit", "protocol": "ip",
             "source": {"address": "10.0.0.0", "wildcard": "0.255.255.255"}, "destination": any_},
        ]}],
        nat_rules=[{"dynamic": {"acl": "1", "target": {"interface": "GigabitEthernet1/1"}, "overload": True}}]))

    # The core: a layer 3 switch routing between the VLANs, root of every
    # spanning tree, DHCP server for the agencies, the access list keeping
    # Field Ops out of personnel data, and a SPAN session for the Watchers.
    core_ifaces = {
        L3 + "24": iface("10.0.0.2/30", switchport={"routed": True}, description="FW"),
        L3 + "20": iface(switchport={"mode": "access", "access_vlan": 99}, description="WATCHERS (SPAN)"),
    }
    for n, (v, name, sw, *_rest) in enumerate(agencies, start=1):
        core_ifaces[L3 + str(n)] = dict(trunk_, shutdown=False, description=sw)
        core_ifaces[f"Vlan{v}"] = iface(f"10.{v}.{v}.1/24", description=name)
    core_ifaces[L3 + "5"] = dict(trunk_, shutdown=False, description="SW-DATA")
    core_ifaces["Vlan50"] = iface("10.50.50.1/24", description="DATA-CENTER")
    core_ifaces["Vlan40"]["acl_in"] = "140"
    core = t.add("l3_switch", "CORE", 800, 420, config={
        "ip_routing": True,
        "vlans": vlans,
        "interfaces": core_ifaces,
        "spanning_tree": {"mode": "rapid_pvst", "priorities": {v: 4096 for v in ["1", "10", "20", "30", "40", "50", "99"]}},
        "dhcp_pools": [{"name": name, "network": [f"10.{v}.{v}.0", 24], "default_router": f"10.{v}.{v}.1",
                        "dns_server": "10.50.50.10", "domain_name": "shadow.net"} for v, name, *_ in agencies],
        "dhcp_excluded": [[f"10.{v}.{v}.1", f"10.{v}.{v}.49"] for v, *_ in agencies],
        "access_lists": [{"name": "140", "kind": "extended",
                          "remarks": ["Field Ops may not open personnel records"], "entries": [
            {"sequence": 10, "action": "deny", "protocol": "ip",
             "source": {"address": "10.40.40.0", "wildcard": "0.0.0.255"},
             "destination": {"address": "10.30.30.20", "wildcard": "0.0.0.0"}},
            {"sequence": 20, "action": "permit", "protocol": "ip", "source": any_, "destination": any_},
        ]}],
        "ospf": [{"process_id": 1, "router_id": "2.2.2.2",
                  "networks": [{"address": "10.0.0.0", "wildcard": "0.255.255.255", "area": 0}],
                  "passive": [f"Vlan{v}" for v in [10, 20, 30, 40, 50]]}],
        "monitor_sessions": {"1": {"sources": [[L3 + "3", "both"]], "destination": L3 + "20"}},
    })
    watcher = t.add("pc", "WATCHER", 150, 440, ip="10.99.99.10/24", gateway="10.99.99.1")
    t.cable(isp, "Ethernet0", fw, "GigabitEthernet1/1")
    t.cable(isp, "Ethernet1", intel, "GigabitEthernet0")
    t.cable(fw, "GigabitEthernet1/2", core, L3 + "24")
    t.cable(core, L3 + "20", watcher, "FastEthernet0")

    # Each agency: an access switch trunked to the core, its workstations
    # on DHCP behind port security (MAC filtering), its servers addressed.
    psec = {"enabled": True, "maximum": 1, "sticky": True, "violation": "restrict"}
    for n, (v, name, sw_name, (cx, cy), _, hosts) in enumerate(agencies, start=1):
        ports = {"GigabitEthernet0/1": trunk_, "GigabitEthernet0/2": trunk_}
        for k, (kind, _) in enumerate(hosts, start=1):
            port = {"mode": "access", "access_vlan": v, "portfast": True}
            if kind != "server":
                port["port_security"] = psec
            ports[F + str(k)] = {"switchport": port}
        sw = t.add("l2_switch", sw_name, cx, cy - 55, config={"vlans": vlans, "interfaces": ports})
        t.cable(core, L3 + str(n), sw, "GigabitEthernet0/1")
        for k, (kind, host) in enumerate(hosts, start=1):
            x = cx + (-75 if k == 1 else 75)
            if kind == "server":
                h = t.add(kind, host, x, cy + 50, ip=f"10.{v}.{v}.20/24", gateway=f"10.{v}.{v}.1")
            else:
                h = t.add(kind, host, x, cy + 50, dhcp=True)
            t.cable(sw, F + str(k), h, "GigabitEthernet0" if kind == "server" else "FastEthernet0")
    # A second path between two agencies: spanning tree blocks one end.
    t.cable(t.id("SW-IA"), "GigabitEthernet0/2", t.id("SW-FIELD"), "GigabitEthernet0/2")

    # The data centre everyone reaches.
    data = t.add("l2_switch", "SW-DATA", 800, 755, config={"vlans": vlans, "interfaces": {
        "GigabitEthernet0/1": trunk_,
        F + "1": {"switchport": {"mode": "access", "access_vlan": 50, "portfast": True}},
        F + "2": {"switchport": {"mode": "access", "access_vlan": 50, "portfast": True}}}})
    vault = t.add("server", "VAULT", 725, 860, ip="10.50.50.10/24", gateway="10.50.50.1")
    apps = t.add("server", "APPS", 875, 860, ip="10.50.50.20/24", gateway="10.50.50.1")
    t.cable(core, L3 + "5", data, "GigabitEthernet0/1")
    t.cable(data, F + "1", vault, "GigabitEthernet0")
    t.cable(data, F + "2", apps, "GigabitEthernet0")

    project = t.project(None)
    del project["lab"]

    def zone(cx, cy, w, h, text, swatch, sides, line="solid"):
        return {"id": t._id(), "kind": "area",
                "rect": {"x": cx - w / 2, "y": cy - h / 2, "width": w, "height": h}, "text": text,
                "style": {"shape": {"kind": "polygon", "sides": sides}, "stroke": swatch, "fill": swatch,
                          "fill_opacity": 0.16, "line": line, "label": swatch}}
    project["annotations"] = [
        zone(800, 165, 330, 270, "EDGE  ·  FIREWALL", "gray", 6),
        zone(800, 425, 230, 200, "CORE", "teal", 8),
        *[zone(cx, cy, 330, 290, f"VLAN {v}  ·  {name}", swatch, 6) for v, name, _, (cx, cy), swatch, _ in agencies],
        zone(800, 805, 360, 260, "VLAN 50  ·  DATA-CENTER", "orange", 6),
        zone(150, 440, 220, 200, "VLAN 99  ·  WATCHERS", "purple", 5, line="dashed"),
        {"id": t._id(), "kind": "note", "rect": {"x": 20.0, "y": 30.0, "width": 320.0, "height": 150.0},
         "text": "THE SHADOW NETWORK\nA multi-agency VLAN operation with everything netrace simulates: VLANs and "
                 "trunks, rapid spanning tree, inter-VLAN routing on the core, DHCP, port security, an access "
                 "list, OSPF, PAT through the firewall, TCP, and a SPAN session the Watchers listen on. "
                 "Help > Take the Tour walks you through the app on it."},
    ]
    project["next_id"] = t.next_id
    with open("examples/tour.netrace/project.json", "w") as f:
        json.dump(project, f, indent=2)
        f.write("\n")
    print("examples/tour.netrace")


os.makedirs("examples/tour.netrace", exist_ok=True)
tour()
