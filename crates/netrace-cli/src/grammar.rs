//! The commands each mode understands, and matching typed lines against
//! them the way IOS does: unique abbreviations (`sh ip int br`), `?` help,
//! Tab completion, and errors pointing at the offending word.

use std::net::Ipv4Addr;

use netrace_doc::DeviceKind;

use crate::iface;

/// Command modes, named after their prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
    /// `R1>`
    User,
    /// `R1#`
    Privileged,
    /// `R1(config)#`
    Config,
    /// `R1(config-if)#`
    Interface,
    /// `R1(config-subif)#`
    Subinterface,
    /// `R1(config-line)#`
    Line,
    /// `SW1(config-vlan)#`
    Vlan,
    /// `R1(dhcp-config)#`
    DhcpPool,
    /// `R1(config-std-nacl)#` / `R1(config-ext-nacl)#`
    Acl,
    /// `R1(config-router)#`
    Router,
}

impl Ctx {
    /// Any of the configuration modes (where `do`, `exit` and `end` work).
    pub fn is_config(self) -> bool {
        !matches!(self, Ctx::User | Ctx::Privileged)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Devices {
    Any,
    Router,
    Switch,
    L3Switch,
    /// Routers and layer 3 switches.
    Routing,
}

impl Devices {
    fn allows(self, kind: DeviceKind) -> bool {
        match self {
            Devices::Any => true,
            Devices::Router => kind == DeviceKind::Router,
            Devices::Switch => kind.is_switch(),
            Devices::L3Switch => kind == DeviceKind::L3Switch,
            Devices::Routing => kind == DeviceKind::Router || kind == DeviceKind::L3Switch,
        }
    }
}

pub(crate) struct Command {
    pub id: &'static str,
    /// Keywords, and placeholders: `A.B.C.D`, `WORD`, `LINE` (rest of the
    /// line), `IFACE`, `TARGET` (address or interface), `<a-b>`; a final
    /// element in brackets is optional.
    pub syntax: &'static str,
    pub ctx: &'static [Ctx],
    pub devices: Devices,
    /// Help for each placeholder, in order.
    pub args: &'static [&'static str],
}

use Ctx::*;
const EXEC: &[Ctx] = &[User, Privileged];
const PRIV: &[Ctx] = &[Privileged];
const CONFIG: &[Ctx] = &[Config];
const IF: &[Ctx] = &[Interface, Subinterface];
const SUBIF: &[Ctx] = &[Subinterface];
const LINE: &[Ctx] = &[Line];
const CONFIG_ANY: &[Ctx] = &[
    Config,
    Interface,
    Subinterface,
    Line,
    Vlan,
    DhcpPool,
    Acl,
    Router,
];
/// Global configuration commands that also work from sub-modes (`interface`, `line`, `vlan`).
const CONFIG_NAV: &[Ctx] = &[
    Config,
    Interface,
    Subinterface,
    Line,
    Vlan,
    DhcpPool,
    Acl,
    Router,
];
const SWITCHPORT: &[Ctx] = &[Interface];

macro_rules! cmd {
    ($id:literal, $syntax:literal, $ctx:expr) => {
        cmd!($id, $syntax, $ctx, Devices::Any, &[])
    };
    ($id:literal, $syntax:literal, $ctx:expr, $devices:expr) => {
        cmd!($id, $syntax, $ctx, $devices, &[])
    };
    ($id:literal, $syntax:literal, $ctx:expr, $devices:expr, $args:expr) => {
        Command {
            id: $id,
            syntax: $syntax,
            ctx: $ctx,
            devices: $devices,
            args: $args,
        }
    };
}

pub(crate) const COMMANDS: &[Command] = &[
    // EXEC
    cmd!("enable", "enable", EXEC),
    cmd!("disable", "disable", PRIV),
    cmd!("exit", "exit", EXEC),
    cmd!("logout", "logout", EXEC),
    cmd!(
        "ping",
        "ping A.B.C.D",
        EXEC,
        Devices::Any,
        &["Destination address"]
    ),
    cmd!(
        "ping-repeat",
        "ping A.B.C.D repeat <1-2147483647>",
        EXEC,
        Devices::Any,
        &["Destination address", "Repeat count"]
    ),
    cmd!(
        "traceroute",
        "traceroute A.B.C.D",
        EXEC,
        Devices::Any,
        &["Destination address"]
    ),
    cmd!("configure", "configure terminal", PRIV),
    cmd!("copy-run-start", "copy running-config startup-config", PRIV),
    cmd!("copy-start-run", "copy startup-config running-config", PRIV),
    cmd!("write", "write [memory]", PRIV),
    cmd!("write-erase", "write erase", PRIV),
    cmd!("erase-startup", "erase startup-config", PRIV),
    cmd!("reload", "reload", PRIV),
    cmd!("clear-arp", "clear arp-cache", PRIV),
    cmd!("show-version", "show version", EXEC),
    cmd!("show-run", "show running-config", PRIV),
    cmd!("show-start", "show startup-config", PRIV),
    cmd!("show-ip-brief", "show ip interface brief", EXEC),
    cmd!("show-ip-route", "show ip route", EXEC),
    cmd!(
        "show-interfaces",
        "show interfaces [IFACE]",
        EXEC,
        Devices::Any,
        &["Interface name"]
    ),
    cmd!("show-arp", "show arp", EXEC),
    cmd!("show-ip-arp", "show ip arp", EXEC),
    cmd!("show-mac", "show mac address-table", EXEC, Devices::Switch),
    cmd!("show-history", "show history", EXEC),
    cmd!("show-vlan-brief", "show vlan brief", EXEC, Devices::Switch),
    cmd!("show-vlan", "show vlan", EXEC, Devices::Switch),
    cmd!("show-trunk", "show interfaces trunk", EXEC, Devices::Switch),
    cmd!(
        "show-status",
        "show interfaces status",
        EXEC,
        Devices::Switch
    ),
    cmd!(
        "show-switchport",
        "show interfaces IFACE switchport",
        EXEC,
        Devices::Switch,
        &["Interface name"]
    ),
    cmd!("show-stp", "show spanning-tree", EXEC, Devices::Switch),
    cmd!(
        "show-stp-vlan",
        "show spanning-tree vlan <1-4094>",
        EXEC,
        Devices::Switch,
        &["VLAN ID"]
    ),
    cmd!("show-cdp", "show cdp neighbors", EXEC),
    cmd!("show-cdp-detail", "show cdp neighbors detail", EXEC),
    cmd!("show-lldp", "show lldp neighbors", EXEC),
    cmd!("show-psec", "show port-security", EXEC, Devices::Switch),
    cmd!(
        "show-psec-if",
        "show port-security interface IFACE",
        EXEC,
        Devices::Switch,
        &["Interface name"]
    ),
    cmd!(
        "show-psec-address",
        "show port-security address",
        EXEC,
        Devices::Switch
    ),
    cmd!(
        "show-ospf-neighbor",
        "show ip ospf neighbor [detail]",
        EXEC,
        Devices::Routing
    ),
    cmd!(
        "show-ospf-interface-brief",
        "show ip ospf interface brief",
        EXEC,
        Devices::Routing
    ),
    cmd!(
        "show-ospf-interface",
        "show ip ospf interface [IFACE]",
        EXEC,
        Devices::Routing
    ),
    cmd!(
        "show-ospf-database",
        "show ip ospf database",
        EXEC,
        Devices::Routing
    ),
    cmd!("show-ospf", "show ip ospf", EXEC, Devices::Routing),
    cmd!(
        "show-ip-protocols",
        "show ip protocols",
        EXEC,
        Devices::Routing
    ),
    cmd!(
        "show-ip-route-ospf",
        "show ip route ospf",
        EXEC,
        Devices::Routing
    ),
    cmd!(
        "show-nat-translations",
        "show ip nat translations [verbose]",
        EXEC,
        Devices::Router
    ),
    cmd!(
        "show-nat-statistics",
        "show ip nat statistics",
        EXEC,
        Devices::Router
    ),
    cmd!(
        "show-dhcp-binding",
        "show ip dhcp binding",
        EXEC,
        Devices::Router
    ),
    cmd!("show-dhcp-pool", "show ip dhcp pool", EXEC, Devices::Router),
    cmd!(
        "show-acls",
        "show access-lists [WORD]",
        EXEC,
        Devices::Any,
        &["ACL number or name"]
    ),
    cmd!(
        "show-ip-acls",
        "show ip access-lists [WORD]",
        EXEC,
        Devices::Any,
        &["ACL number or name"]
    ),
    // Global configuration
    cmd!(
        "hostname",
        "hostname WORD",
        CONFIG,
        Devices::Any,
        &["This system's network name"]
    ),
    cmd!(
        "interface",
        "interface IFACE",
        CONFIG_NAV,
        Devices::Any,
        &["Interface name"]
    ),
    cmd!(
        "ip-route",
        "ip route A.B.C.D A.B.C.D TARGET [<1-255>]",
        CONFIG,
        Devices::Any,
        &[
            "Destination prefix",
            "Destination prefix mask",
            "Forwarding router's address or exit interface",
            "Distance metric for this route"
        ]
    ),
    cmd!(
        "no-ip-route",
        "no ip route A.B.C.D A.B.C.D [TARGET]",
        CONFIG,
        Devices::Any,
        &[
            "Destination prefix",
            "Destination prefix mask",
            "Forwarding router's address or exit interface"
        ]
    ),
    cmd!(
        "ip-default-gateway",
        "ip default-gateway A.B.C.D",
        CONFIG,
        Devices::Switch,
        &["IP address of default gateway"]
    ),
    cmd!(
        "no-ip-default-gateway",
        "no ip default-gateway",
        CONFIG,
        Devices::Switch
    ),
    cmd!("ip-routing", "ip routing", CONFIG, Devices::L3Switch),
    cmd!("no-ip-routing", "no ip routing", CONFIG, Devices::L3Switch),
    cmd!("ip-routing-router", "ip routing", CONFIG, Devices::Router),
    cmd!(
        "enable-secret",
        "enable secret WORD",
        CONFIG,
        Devices::Any,
        &["The UNENCRYPTED (cleartext) 'enable' secret"]
    ),
    cmd!(
        "enable-password",
        "enable password WORD",
        CONFIG,
        Devices::Any,
        &["The UNENCRYPTED (cleartext) 'enable' password"]
    ),
    cmd!("no-enable-secret", "no enable secret", CONFIG),
    cmd!("no-enable-password", "no enable password", CONFIG),
    cmd!("no-domain-lookup", "no ip domain-lookup", CONFIG),
    cmd!("domain-lookup", "ip domain-lookup", CONFIG),
    cmd!("no-domain-lookup", "no ip domain lookup", CONFIG),
    cmd!("domain-lookup", "ip domain lookup", CONFIG),
    cmd!("service-pe", "service password-encryption", CONFIG),
    cmd!("no-service-pe", "no service password-encryption", CONFIG),
    cmd!(
        "banner-motd",
        "banner motd LINE",
        CONFIG,
        Devices::Any,
        &["c  banner-text c, where 'c' is a delimiting character"]
    ),
    cmd!("no-banner-motd", "no banner motd", CONFIG),
    cmd!(
        "line-con",
        "line console <0-0>",
        CONFIG_NAV,
        Devices::Any,
        &["First Line number"]
    ),
    cmd!(
        "line-vty",
        "line vty <0-15> <0-15>",
        CONFIG_NAV,
        Devices::Any,
        &["First Line number", "Last Line number"]
    ),
    // Interface configuration
    cmd!(
        "ip-address",
        "ip address A.B.C.D A.B.C.D",
        IF,
        Devices::Any,
        &["IP address", "IP subnet mask"]
    ),
    cmd!("no-ip-address", "no ip address", IF),
    cmd!("shutdown", "shutdown", IF),
    cmd!("no-shutdown", "no shutdown", IF),
    cmd!(
        "description",
        "description LINE",
        IF,
        Devices::Any,
        &["Up to 240 characters describing this interface"]
    ),
    cmd!("no-description", "no description", IF),
    cmd!(
        "encapsulation",
        "encapsulation dot1Q <1-4094>",
        SUBIF,
        Devices::Router,
        &["IEEE 802.1Q VLAN ID"]
    ),
    cmd!(
        "encapsulation-native",
        "encapsulation dot1Q <1-4094> native",
        SUBIF,
        Devices::Router,
        &["IEEE 802.1Q VLAN ID"]
    ),
    // Switch ports
    cmd!(
        "sw-mode-access",
        "switchport mode access",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "sw-mode-trunk",
        "switchport mode trunk",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "sw-mode-dynamic",
        "switchport mode dynamic auto",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "sw-mode-desirable",
        "switchport mode dynamic desirable",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "sw-access-vlan",
        "switchport access vlan <1-4094>",
        SWITCHPORT,
        Devices::Switch,
        &["VLAN ID of the VLAN when this port is in access mode"]
    ),
    cmd!(
        "no-sw-access-vlan",
        "no switchport access vlan",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "sw-native-vlan",
        "switchport trunk native vlan <1-4094>",
        SWITCHPORT,
        Devices::Switch,
        &["VLAN ID of the native VLAN when this port is in trunking mode"]
    ),
    cmd!(
        "no-sw-native-vlan",
        "no switchport trunk native vlan",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "sw-allowed",
        "switchport trunk allowed vlan WORD",
        SWITCHPORT,
        Devices::Switch,
        &["VLAN IDs of the allowed VLANs (e.g. 10,20-30), all or none"]
    ),
    cmd!(
        "sw-allowed-add",
        "switchport trunk allowed vlan add WORD",
        SWITCHPORT,
        Devices::Switch,
        &["VLAN IDs to add"]
    ),
    cmd!(
        "sw-allowed-remove",
        "switchport trunk allowed vlan remove WORD",
        SWITCHPORT,
        Devices::Switch,
        &["VLAN IDs to remove"]
    ),
    cmd!(
        "no-sw-allowed",
        "no switchport trunk allowed vlan",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "sw-encapsulation",
        "switchport trunk encapsulation dot1q",
        SWITCHPORT,
        Devices::L3Switch
    ),
    cmd!(
        "sw-nonegotiate",
        "switchport nonegotiate",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "no-sw-nonegotiate",
        "no switchport nonegotiate",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "psec",
        "switchport port-security",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "no-psec",
        "no switchport port-security",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "psec-max",
        "switchport port-security maximum <1-132>",
        SWITCHPORT,
        Devices::Switch,
        &["Maximum addresses"]
    ),
    cmd!(
        "psec-sticky",
        "switchport port-security mac-address sticky",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "no-psec-sticky",
        "no switchport port-security mac-address sticky",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "psec-mac",
        "switchport port-security mac-address WORD",
        SWITCHPORT,
        Devices::Switch,
        &["48 bit mac address (H.H.H)"]
    ),
    cmd!(
        "psec-protect",
        "switchport port-security violation protect",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "psec-restrict",
        "switchport port-security violation restrict",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "psec-shutdown",
        "switchport port-security violation shutdown",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "portfast",
        "spanning-tree portfast",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "no-portfast",
        "no spanning-tree portfast",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "bpduguard",
        "spanning-tree bpduguard enable",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "no-bpduguard",
        "spanning-tree bpduguard disable",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!(
        "no-bpduguard",
        "no spanning-tree bpduguard enable",
        SWITCHPORT,
        Devices::Switch
    ),
    cmd!("cdp-enable", "cdp enable", IF),
    cmd!("no-cdp-enable", "no cdp enable", IF),
    cmd!(
        "no-switchport",
        "no switchport",
        SWITCHPORT,
        Devices::L3Switch
    ),
    cmd!("switchport-l2", "switchport", SWITCHPORT, Devices::L3Switch),
    // VLANs, spanning tree, discovery (global)
    cmd!(
        "vlan",
        "vlan WORD",
        CONFIG_NAV,
        Devices::Switch,
        &["ISL VLAN IDs 1-4094 (e.g. 10 or 10,20-30)"]
    ),
    cmd!(
        "no-vlan",
        "no vlan WORD",
        CONFIG_NAV,
        Devices::Switch,
        &["ISL VLAN IDs 1-4094"]
    ),
    cmd!(
        "vlan-name",
        "name WORD",
        &[Vlan],
        Devices::Switch,
        &["The ascii name for the VLAN"]
    ),
    cmd!("no-vlan-name", "no name", &[Vlan], Devices::Switch),
    cmd!(
        "interface-range",
        "interface range LINE",
        CONFIG_NAV,
        Devices::Switch,
        &["Interfaces, e.g. f0/1 - 12, f0/15"]
    ),
    cmd!(
        "stp-pvst",
        "spanning-tree mode pvst",
        CONFIG,
        Devices::Switch
    ),
    cmd!(
        "stp-rapid",
        "spanning-tree mode rapid-pvst",
        CONFIG,
        Devices::Switch
    ),
    cmd!(
        "stp-priority",
        "spanning-tree vlan WORD priority <0-61440>",
        CONFIG,
        Devices::Switch,
        &["VLAN IDs", "bridge priority in increments of 4096"]
    ),
    cmd!(
        "no-stp-priority",
        "no spanning-tree vlan WORD priority",
        CONFIG,
        Devices::Switch,
        &["VLAN IDs"]
    ),
    cmd!(
        "stp-root-primary",
        "spanning-tree vlan WORD root primary",
        CONFIG,
        Devices::Switch,
        &["VLAN IDs"]
    ),
    cmd!(
        "stp-root-secondary",
        "spanning-tree vlan WORD root secondary",
        CONFIG,
        Devices::Switch,
        &["VLAN IDs"]
    ),
    // DHCP and access lists
    cmd!(
        "dhcp-pool",
        "ip dhcp pool WORD",
        CONFIG_NAV,
        Devices::Router,
        &["Pool name"]
    ),
    cmd!(
        "no-dhcp-pool",
        "no ip dhcp pool WORD",
        CONFIG,
        Devices::Router,
        &["Pool name"]
    ),
    cmd!(
        "dhcp-network",
        "network A.B.C.D A.B.C.D",
        &[DhcpPool],
        Devices::Router,
        &["Network number in dotted-decimal notation", "Network mask"]
    ),
    cmd!(
        "dhcp-router",
        "default-router A.B.C.D",
        &[DhcpPool],
        Devices::Router,
        &["Router's IP address"]
    ),
    cmd!(
        "dhcp-dns",
        "dns-server A.B.C.D",
        &[DhcpPool],
        Devices::Router,
        &["Server's IP address"]
    ),
    cmd!(
        "dhcp-domain",
        "domain-name WORD",
        &[DhcpPool],
        Devices::Router,
        &["Domain name"]
    ),
    cmd!(
        "dhcp-excluded",
        "ip dhcp excluded-address A.B.C.D [A.B.C.D]",
        CONFIG,
        Devices::Router,
        &["Low IP address", "High IP address"]
    ),
    cmd!(
        "no-dhcp-excluded",
        "no ip dhcp excluded-address A.B.C.D [A.B.C.D]",
        CONFIG,
        Devices::Router,
        &["Low IP address", "High IP address"]
    ),
    cmd!(
        "helper",
        "ip helper-address A.B.C.D",
        IF,
        Devices::Any,
        &["IP destination address"]
    ),
    cmd!(
        "no-helper",
        "no ip helper-address [A.B.C.D]",
        IF,
        Devices::Any,
        &["IP destination address"]
    ),
    cmd!(
        "access-group",
        "ip access-group WORD in",
        IF,
        Devices::Any,
        &["Access list number or name"]
    ),
    cmd!(
        "access-group",
        "ip access-group WORD out",
        IF,
        Devices::Any,
        &["Access list number or name"]
    ),
    cmd!(
        "no-access-group",
        "no ip access-group WORD in",
        IF,
        Devices::Any,
        &["Access list number or name"]
    ),
    cmd!(
        "no-access-group",
        "no ip access-group WORD out",
        IF,
        Devices::Any,
        &["Access list number or name"]
    ),
    cmd!(
        "acl-numbered",
        "access-list <1-2699> permit LINE",
        CONFIG,
        Devices::Any,
        &[
            "IP access list number",
            "Source (standard) or protocol, source, destination (extended)"
        ]
    ),
    cmd!(
        "acl-numbered",
        "access-list <1-2699> deny LINE",
        CONFIG,
        Devices::Any,
        &[
            "IP access list number",
            "Source (standard) or protocol, source, destination (extended)"
        ]
    ),
    cmd!(
        "acl-remark",
        "access-list <1-2699> remark LINE",
        CONFIG,
        Devices::Any,
        &["IP access list number", "Comment up to 100 characters"]
    ),
    cmd!(
        "no-acl",
        "no access-list <1-2699>",
        CONFIG,
        Devices::Any,
        &["IP access list number"]
    ),
    cmd!(
        "acl-named",
        "ip access-list standard WORD",
        CONFIG_NAV,
        Devices::Any,
        &["Access list name"]
    ),
    cmd!(
        "acl-named",
        "ip access-list extended WORD",
        CONFIG_NAV,
        Devices::Any,
        &["Access list name"]
    ),
    cmd!(
        "no-acl-named",
        "no ip access-list standard WORD",
        CONFIG,
        Devices::Any,
        &["Access list name"]
    ),
    cmd!(
        "no-acl-named",
        "no ip access-list extended WORD",
        CONFIG,
        Devices::Any,
        &["Access list name"]
    ),
    cmd!(
        "nacl-permit",
        "permit LINE",
        &[Acl],
        Devices::Any,
        &["Source (standard) or protocol, source, destination (extended)"]
    ),
    cmd!(
        "nacl-deny",
        "deny LINE",
        &[Acl],
        Devices::Any,
        &["Source (standard) or protocol, source, destination (extended)"]
    ),
    cmd!(
        "nacl-seq-permit",
        "<1-2147483647> permit LINE",
        &[Acl],
        Devices::Any,
        &["Sequence Number", "Matching conditions"]
    ),
    cmd!(
        "nacl-seq-deny",
        "<1-2147483647> deny LINE",
        &[Acl],
        Devices::Any,
        &["Sequence Number", "Matching conditions"]
    ),
    cmd!(
        "nacl-no-seq",
        "no <1-2147483647>",
        &[Acl],
        Devices::Any,
        &["Sequence Number"]
    ),
    cmd!(
        "nacl-remark",
        "remark LINE",
        &[Acl],
        Devices::Any,
        &["Comment up to 100 characters"]
    ),
    // OSPF
    cmd!(
        "router-ospf",
        "router ospf <1-65535>",
        CONFIG_NAV,
        Devices::Routing,
        &["Process ID"]
    ),
    cmd!(
        "no-router-ospf",
        "no router ospf <1-65535>",
        CONFIG,
        Devices::Routing,
        &["Process ID"]
    ),
    cmd!(
        "ospf-network",
        "network A.B.C.D A.B.C.D area <0-4294967295>",
        &[Router],
        Devices::Routing,
        &[
            "Network number",
            "OSPF wild card bits",
            "OSPF area ID as a decimal value"
        ]
    ),
    cmd!(
        "no-ospf-network",
        "no network A.B.C.D A.B.C.D area <0-4294967295>",
        &[Router],
        Devices::Routing,
        &[
            "Network number",
            "OSPF wild card bits",
            "OSPF area ID as a decimal value"
        ]
    ),
    cmd!(
        "ospf-router-id",
        "router-id A.B.C.D",
        &[Router],
        Devices::Routing,
        &["OSPF router-id in IP address format"]
    ),
    cmd!(
        "no-ospf-router-id",
        "no router-id [A.B.C.D]",
        &[Router],
        Devices::Routing,
        &["OSPF router-id in IP address format"]
    ),
    cmd!(
        "ospf-passive-default",
        "passive-interface default",
        &[Router],
        Devices::Routing
    ),
    cmd!(
        "no-ospf-passive-default",
        "no passive-interface default",
        &[Router],
        Devices::Routing
    ),
    cmd!(
        "ospf-passive",
        "passive-interface IFACE",
        &[Router],
        Devices::Routing,
        &["Interface to suppress routing updates on"]
    ),
    cmd!(
        "no-ospf-passive",
        "no passive-interface IFACE",
        &[Router],
        Devices::Routing,
        &["Interface to send routing updates on"]
    ),
    cmd!(
        "ospf-default-originate",
        "default-information originate",
        &[Router],
        Devices::Routing
    ),
    cmd!(
        "no-ospf-default-originate",
        "no default-information originate",
        &[Router],
        Devices::Routing
    ),
    cmd!(
        "ospf-ref-bw",
        "auto-cost reference-bandwidth <1-4294967>",
        &[Router],
        Devices::Routing,
        &["The reference bandwidth in terms of Mbits per second"]
    ),
    cmd!(
        "no-ospf-ref-bw",
        "no auto-cost reference-bandwidth [LINE]",
        &[Router],
        Devices::Routing
    ),
    cmd!(
        "if-ospf-cost",
        "ip ospf cost <1-65535>",
        IF,
        Devices::Routing,
        &["Cost"]
    ),
    cmd!(
        "if-ospf-priority",
        "ip ospf priority <0-255>",
        IF,
        Devices::Routing,
        &["Priority"]
    ),
    cmd!(
        "if-ospf-hello",
        "ip ospf hello-interval <1-65535>",
        IF,
        Devices::Routing,
        &["Seconds"]
    ),
    cmd!(
        "if-ospf-dead",
        "ip ospf dead-interval <1-65535>",
        IF,
        Devices::Routing,
        &["Seconds"]
    ),
    cmd!(
        "if-ospf-area",
        "ip ospf <1-65535> area <0-4294967295>",
        IF,
        Devices::Routing,
        &["Process ID", "OSPF area ID as a decimal value"]
    ),
    cmd!(
        "if-ospf-p2p",
        "ip ospf network point-to-point",
        IF,
        Devices::Routing
    ),
    cmd!(
        "if-ospf-broadcast",
        "ip ospf network broadcast",
        IF,
        Devices::Routing
    ),
    cmd!(
        "no-if-ospf",
        "no ip ospf LINE",
        IF,
        Devices::Routing,
        &["OSPF interface setting to remove"]
    ),
    // NAT
    cmd!("nat-side", "ip nat inside", IF, Devices::Router),
    cmd!("nat-side", "ip nat outside", IF, Devices::Router),
    cmd!("no-nat-side", "no ip nat inside", IF, Devices::Router),
    cmd!("no-nat-side", "no ip nat outside", IF, Devices::Router),
    cmd!(
        "nat-pool",
        "ip nat pool WORD A.B.C.D A.B.C.D netmask A.B.C.D",
        CONFIG,
        Devices::Router,
        &[
            "Pool name",
            "Start IP address",
            "End IP address",
            "Network mask"
        ]
    ),
    cmd!(
        "nat-pool",
        "ip nat pool WORD A.B.C.D A.B.C.D prefix-length <1-32>",
        CONFIG,
        Devices::Router,
        &[
            "Pool name",
            "Start IP address",
            "End IP address",
            "Prefix length"
        ]
    ),
    cmd!(
        "no-nat-pool",
        "no ip nat pool WORD [LINE]",
        CONFIG,
        Devices::Router,
        &["Pool name"]
    ),
    cmd!(
        "nat-static",
        "ip nat inside source static A.B.C.D A.B.C.D",
        CONFIG,
        Devices::Router,
        &["Inside local IP address", "Inside global IP address"]
    ),
    cmd!(
        "no-nat-static",
        "no ip nat inside source static A.B.C.D A.B.C.D",
        CONFIG,
        Devices::Router,
        &["Inside local IP address", "Inside global IP address"]
    ),
    cmd!(
        "nat-list-pool",
        "ip nat inside source list WORD pool WORD [overload]",
        CONFIG,
        Devices::Router,
        &[
            "Access list number or name",
            "Name pool of global addresses"
        ]
    ),
    cmd!(
        "nat-list-iface",
        "ip nat inside source list WORD interface IFACE [overload]",
        CONFIG,
        Devices::Router,
        &[
            "Access list number or name",
            "Interface whose address is used"
        ]
    ),
    cmd!(
        "no-nat-list",
        "no ip nat inside source list WORD [LINE]",
        CONFIG,
        Devices::Router,
        &["Access list number or name"]
    ),
    cmd!("cdp-run", "cdp run", CONFIG),
    cmd!("no-cdp-run", "no cdp run", CONFIG),
    cmd!("lldp-run", "lldp run", CONFIG),
    cmd!("no-lldp-run", "no lldp run", CONFIG),
    // Line configuration
    cmd!(
        "password",
        "password WORD",
        LINE,
        Devices::Any,
        &["The UNENCRYPTED (cleartext) line password"]
    ),
    cmd!("no-password", "no password", LINE),
    cmd!("login", "login", LINE),
    cmd!("no-login", "no login", LINE),
    cmd!("logging-sync", "logging synchronous", LINE),
    cmd!("no-logging-sync", "no logging synchronous", LINE),
    cmd!(
        "exec-timeout",
        "exec-timeout <0-35791> [<0-2147483>]",
        LINE,
        Devices::Any,
        &["Timeout in minutes", "Timeout in seconds"]
    ),
    // Every configuration mode
    cmd!("exit-config", "exit", CONFIG_ANY),
    cmd!("end", "end", CONFIG_ANY),
    cmd!("do", "do LINE", CONFIG_ANY, Devices::Any, &["Exec Command"]),
];

/// What each keyword is for, as `?` shows it. A few words mean different
/// things in EXEC and configuration modes.
pub(crate) fn keyword_help(keyword: &str, ctx: Ctx) -> &'static str {
    let exec = matches!(ctx, Ctx::User | Ctx::Privileged);
    match (keyword, exec) {
        ("interface", true) => return "IP interface status and configuration",
        ("interface", false) => return "Select an interface to configure",
        ("route", true) => return "IP routing table",
        ("route", false) => return "Establish static routes",
        ("ip", true) => return "IP information",
        ("ip", false) if ctx == Ctx::Config => return "Global IP configuration subcommands",
        ("ip", false) => return "Interface Internet Protocol config commands",
        ("enable", true) => return "Turn on privileged commands",
        ("enable", false) => return "Modify enable password parameters",
        _ => {}
    }
    match keyword {
        "disable" => "Turn off privileged commands",
        "exit" => "Exit from the EXEC / current mode",
        "logout" => "Exit from the EXEC",
        "ping" => "Send echo messages",
        "repeat" => "specify repeat count",
        "traceroute" => "Trace route to destination",
        "configure" => "Enter configuration mode",
        "terminal" => "Configure from the terminal",
        "copy" => "Copy from one file to another",
        "running-config" => "Current operating configuration",
        "startup-config" => "Contents of startup configuration",
        "write" => "Write running configuration to memory, network, or terminal",
        "memory" => "Write to NV memory",
        "erase" => "Erase a filesystem",
        "reload" => "Halt and perform a cold restart",
        "clear" => "Reset functions",
        "arp-cache" => "Clear the entire ARP cache",
        "show" => "Show running system information",
        "version" => "System hardware and software status",
        "brief" => "Brief summary of IP status and configuration",
        "interfaces" => "Interface status and configuration",
        "arp" => "ARP table",
        "mac" => "MAC configuration",
        "address-table" => "MAC forwarding table",
        "history" => "Display the session command history",
        "hostname" => "Set system's network name",
        "no" => "Negate a command or set its defaults",
        "default-gateway" => "Specify default gateway (if not routing IP)",
        "routing" => "Enable IP routing",
        "secret" => "Assign the privileged level secret",
        "password" => "Assign the password",
        "domain-lookup" => "Enable IP Domain Name System hostname translation",
        "domain" => "IP DNS Resolver",
        "lookup" => "Enable IP Domain Name System hostname translation",
        "service" => "Modify use of network based services",
        "password-encryption" => "Encrypt system passwords",
        "banner" => "Define a login banner",
        "motd" => "Set Message of the Day banner",
        "line" => "Configure a terminal line",
        "console" => "Primary terminal line",
        "vty" => "Virtual terminal",
        "address" => "Set the IP address of an interface",
        "shutdown" => "Shutdown the selected interface",
        "description" => "Interface specific description",
        "encapsulation" => "Set encapsulation type for an interface",
        "dot1Q" => "IEEE 802.1Q Virtual LAN",
        "switchport" => "Set switching mode characteristics",
        "mode" => "Set trunking mode of the interface",
        "access" => "Set access mode characteristics of the interface",
        "trunk" => "Set trunking characteristics of the interface",
        "native" => "Set trunking native characteristics when interface is in trunking mode",
        "allowed" => "Set allowed VLAN characteristics when interface is in trunking mode",
        "add" => "add VLANs to the current list",
        "remove" => "remove VLANs from the current list",
        "dynamic" => "Set trunking mode to dynamically negotiate access or trunk mode",
        "auto" => "Set trunking mode dynamic negotiation parameter to AUTO",
        "desirable" => "Set trunking mode dynamic negotiation parameter to DESIRABLE",
        "nonegotiate" => "Device will not engage in negotiation protocol on this interface",
        "port-security" => "Security related command",
        "maximum" => "Max secure addresses",
        "mac-address" => "Secure mac address",
        "sticky" => "Configure dynamic secure addresses as sticky",
        "violation" => "Security violation mode",
        "protect" => "Security violation protect mode",
        "restrict" => "Security violation restrict mode",
        "spanning-tree" => "Spanning Tree Subsystem",
        "portfast" => "Spanning tree portfast options",
        "bpduguard" => "Don't accept BPDUs on this interface",
        "pvst" => "Per-Vlan spanning tree mode",
        "rapid-pvst" => "Per-Vlan rapid spanning tree mode",
        "priority" => "Set the bridge priority for the spanning tree",
        "root" => "Configure switch as root",
        "primary" => "Configure this switch as primary root for this spanning tree",
        "secondary" => "Configure switch as secondary root",
        "cdp" => "CDP information / Global CDP configuration subcommands",
        "neighbors" => "CDP neighbor entries",
        "detail" => "Show detailed information",
        "run" => "Enable the protocol",
        "lldp" => "LLDP information / Global LLDP configuration subcommands",
        "vlan" => "VLAN commands / Vlan commands",
        "name" => "Ascii name of the VLAN",
        "range" => "interface range command",
        "status" => "Show interface line status",
        "dhcp" => "Configure DHCP server and relay parameters / DHCP information",
        "pool" => "Configure DHCP address pools / DHCP pools information",
        "binding" => "DHCP address bindings",
        "excluded-address" => "Prevent DHCP from assigning certain addresses",
        "network" => "Network number and mask",
        "default-router" => "Default routers",
        "dns-server" => "DNS servers",
        "domain-name" => "Domain name",
        "helper-address" => "Specify a destination address for UDP broadcasts",
        "access-group" => "Specify access control for packets",
        "access-list" => "Add an access list entry",
        "access-lists" => "List access lists",
        "standard" => "Standard Access List",
        "extended" => "Extended Access List",
        "permit" => "Specify packets to forward",
        "deny" => "Specify packets to reject",
        "remark" => "Access list entry comment",
        "in" => "inbound packets",
        "nat" => "NAT configuration commands / IP NAT information",
        "ospf" => "Open Shortest Path First (OSPF)",
        "router-id" => "router-id for this OSPF process",
        "area" => "Set the OSPF area ID",
        "passive-interface" => "Suppress routing updates on an interface",
        "default-information" => "Control distribution of default information",
        "originate" => "Distribute a default route",
        "auto-cost" => "Calculate OSPF interface cost according to bandwidth",
        "reference-bandwidth" => "Use reference bandwidth method to assign OSPF cost",
        "cost" => "Interface cost",
        "hello-interval" => "Time between HELLO packets",
        "dead-interval" => "Interval after which a neighbor is declared dead",
        "point-to-point" => "Specify OSPF point-to-point network",
        "broadcast" => "Specify OSPF broadcast multi-access network",
        "neighbor" => "Neighbor list",
        "database" => "Database summary",
        "protocols" => "IP routing protocol process parameters and statistics",
        "inside" => "Inside address translation / Inside interface for address translation",
        "outside" => "Outside interface for address translation",
        "source" => "Source address translation",
        "static" => "Specify static local->global mapping",
        "list" => "Specify access list describing local addresses",
        "overload" => "Overload an address translation",
        "netmask" => "Specify the network mask",
        "prefix-length" => "Specify the prefix length",
        "translations" => "Translation entries",
        "statistics" => "Translation statistics",
        "verbose" => "Display extra information",
        "out" => "outbound packets",
        "login" => "Enable password checking",
        "logging" => "Modify message logging facilities",
        "synchronous" => "Synchronized message output",
        "exec-timeout" => "Set the EXEC timeout",
        "end" => "Exit from configure mode",
        "do" => "To run exec commands in config mode",
        _ => "",
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Elem {
    Keyword(&'static str),
    Ip,
    Word,
    Line,
    Iface,
    Target,
    Num(u32, u32),
    Optional(Box<Elem>),
}

pub(crate) fn elems(syntax: &'static str) -> Vec<Elem> {
    syntax
        .split(' ')
        .map(|word| {
            if let Some(inner) = word.strip_prefix('[').and_then(|w| w.strip_suffix(']')) {
                // Only placeholders are ever optional here.
                let inner: &'static str = inner;
                return Elem::Optional(Box::new(element(inner)));
            }
            element(word)
        })
        .collect()
}

fn element(word: &'static str) -> Elem {
    match word {
        "A.B.C.D" => Elem::Ip,
        "WORD" => Elem::Word,
        "LINE" => Elem::Line,
        "IFACE" => Elem::Iface,
        "TARGET" => Elem::Target,
        w if w.starts_with('<') => {
            let (a, b) = w
                .trim_matches(|c| c == '<' || c == '>')
                .split_once('-')
                .expect("ranges are <a-b>");
            Elem::Num(
                a.parse().expect("range start"),
                b.parse().expect("range end"),
            )
        }
        w => Elem::Keyword(w),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    pub text: String,
    /// Character offset in the line, for the `^` marker.
    pub start: usize,
}

pub(crate) fn tokenize(line: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut start = 0;
    for (i, c) in line.chars().enumerate() {
        if c.is_whitespace() {
            if !current.is_empty() {
                tokens.push(Token {
                    text: std::mem::take(&mut current),
                    start,
                });
            }
        } else {
            if current.is_empty() {
                start = i;
            }
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(Token {
            text: current,
            start,
        });
    }
    tokens
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Keyword(&'static str),
    Ip(Ipv4Addr),
    Word(String),
    Line(String),
    Iface(String),
    Num(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fail {
    /// The token at this index doesn't fit.
    Invalid(usize),
    /// The line ended too early.
    Incomplete,
}

impl Fail {
    fn reach(self, tokens: usize) -> usize {
        match self {
            Fail::Invalid(i) => i,
            Fail::Incomplete => tokens,
        }
    }
}

fn keyword_matches(keyword: &str, typed: &str) -> bool {
    !typed.is_empty()
        && keyword
            .to_ascii_lowercase()
            .starts_with(&typed.to_ascii_lowercase())
}

fn parse_ip(text: &str) -> Option<Ipv4Addr> {
    // IOS wants all four parts; Rust's parser agrees.
    text.parse().ok()
}

/// Matches tokens against a command's elements. `line` is the original
/// text, for `LINE` placeholders that keep spacing.
pub(crate) fn match_elems(
    elems: &[Elem],
    tokens: &[Token],
    line: &str,
    kind: DeviceKind,
) -> Result<Vec<Value>, Fail> {
    fn go(
        elems: &[Elem],
        tokens: &[Token],
        line: &str,
        kind: DeviceKind,
        ti: usize,
        mut values: Vec<Value>,
    ) -> Result<Vec<Value>, Fail> {
        let Some(elem) = elems.first() else {
            return if ti == tokens.len() {
                Ok(values)
            } else {
                Err(Fail::Invalid(ti))
            };
        };
        if let Elem::Optional(inner) = elem {
            if ti == tokens.len() {
                return go(&elems[1..], tokens, line, kind, ti, values);
            }
            let mut with: Vec<Elem> = vec![(**inner).clone()];
            with.extend_from_slice(&elems[1..]);
            return go(&with, tokens, line, kind, ti, values.clone()).or_else(|e| {
                go(&elems[1..], tokens, line, kind, ti, values).map_err(|e2| {
                    if e2.reach(tokens.len()) > e.reach(tokens.len()) {
                        e2
                    } else {
                        e
                    }
                })
            });
        }
        let Some(token) = tokens.get(ti) else {
            return Err(Fail::Incomplete);
        };
        let rest = &elems[1..];
        match elem {
            Elem::Keyword(k) => {
                if !keyword_matches(k, &token.text) {
                    return Err(Fail::Invalid(ti));
                }
                values.push(Value::Keyword(k));
                go(rest, tokens, line, kind, ti + 1, values)
            }
            Elem::Ip => {
                let Some(ip) = parse_ip(&token.text) else {
                    return Err(Fail::Invalid(ti));
                };
                values.push(Value::Ip(ip));
                go(rest, tokens, line, kind, ti + 1, values)
            }
            Elem::Word => {
                values.push(Value::Word(token.text.clone()));
                go(rest, tokens, line, kind, ti + 1, values)
            }
            Elem::Line => {
                let text: String = line.chars().skip(token.start).collect();
                values.push(Value::Line(text.trim_end().to_string()));
                Ok(values)
            }
            Elem::Num(a, b) => {
                match token.text.parse::<u32>() {
                    Ok(n) if (*a..=*b).contains(&n) => values.push(Value::Num(n)),
                    _ => return Err(Fail::Invalid(ti)),
                }
                go(rest, tokens, line, kind, ti + 1, values)
            }
            Elem::Iface | Elem::Target => {
                if *elem == Elem::Target
                    && let Some(ip) = parse_ip(&token.text)
                {
                    let mut v = values.clone();
                    v.push(Value::Ip(ip));
                    return go(rest, tokens, line, kind, ti + 1, v);
                }
                if let Some(name) = iface::parse(&token.text, kind) {
                    let mut v = values.clone();
                    v.push(Value::Iface(name));
                    if let Ok(done) = go(rest, tokens, line, kind, ti + 1, v) {
                        return Ok(done);
                    }
                }
                if let Some(next) = tokens.get(ti + 1)
                    && let Some(name) = iface::parse_two(&token.text, &next.text, kind)
                {
                    values.push(Value::Iface(name));
                    return go(rest, tokens, line, kind, ti + 2, values);
                }
                // A known type with no number yet ("int gig") is incomplete;
                // anything else is invalid here.
                if ti + 1 == tokens.len() && iface::expand_type(&token.text, kind).is_some() {
                    return Err(Fail::Incomplete);
                }
                // "gig 0/9": the type was fine, so point at the number.
                let bad = if tokens.get(ti + 1).is_some()
                    && !token.text.chars().any(|c| c.is_ascii_digit())
                    && iface::expand_type(&token.text, kind).is_some()
                {
                    ti + 1
                } else {
                    ti
                };
                Err(Fail::Invalid(bad))
            }
            Elem::Optional(_) => unreachable!("handled above"),
        }
    }
    go(elems, tokens, line, kind, 0, Vec::new())
}

/// The commands available in a mode on this device.
pub(crate) fn commands_for(ctx: Ctx, kind: DeviceKind) -> impl Iterator<Item = &'static Command> {
    COMMANDS
        .iter()
        .filter(move |c| c.ctx.contains(&ctx) && c.devices.allows(kind))
}

pub(crate) enum Parsed {
    Ok(&'static Command, Vec<Value>),
    Invalid(usize),
    Incomplete,
    Ambiguous,
    Empty,
}

/// Matches a whole line in a mode.
pub(crate) fn parse(line: &str, ctx: Ctx, kind: DeviceKind) -> Parsed {
    let tokens = tokenize(line);
    if tokens.is_empty() {
        return Parsed::Empty;
    }
    let mut successes: Vec<(&'static Command, Vec<Value>)> = Vec::new();
    let mut furthest = Fail::Invalid(0);
    for command in commands_for(ctx, kind) {
        match match_elems(&elems(command.syntax), &tokens, line, kind) {
            Ok(values) => successes.push((command, values)),
            Err(fail) => {
                let better = fail.reach(tokens.len()) > furthest.reach(tokens.len())
                    || (fail.reach(tokens.len()) == furthest.reach(tokens.len())
                        && fail == Fail::Incomplete);
                if better {
                    furthest = fail;
                }
            }
        }
    }
    // Several commands fit: a keyword beats a placeholder ("mac-address sticky"
    // over "mac-address WORD"), then an exactly typed keyword decides, else
    // it's ambiguous.
    if successes.len() > 1 {
        let keywords = |values: &Vec<Value>| {
            values
                .iter()
                .filter(|v| matches!(v, Value::Keyword(_)))
                .count()
        };
        let most = successes
            .iter()
            .map(|(_, v)| keywords(v))
            .max()
            .unwrap_or(0);
        successes.retain(|(_, v)| keywords(v) == most);
    }
    if successes.len() > 1 {
        let exact: Vec<_> = successes
            .iter()
            .filter(|(_, values)| {
                values.iter().zip(&tokens).all(|(v, t)| match v {
                    Value::Keyword(k) => k.eq_ignore_ascii_case(&t.text),
                    _ => true,
                })
            })
            .cloned()
            .collect();
        let same_id = successes.iter().all(|(c, _)| c.id == successes[0].0.id);
        if exact.len() == 1 {
            successes = exact;
        } else if !same_id {
            return Parsed::Ambiguous;
        }
    }
    if let Some((command, values)) = successes.into_iter().next() {
        return Parsed::Ok(command, values);
    }
    // A word that starts several keywords is ambiguous rather than invalid.
    let at = furthest.reach(tokens.len());
    if at < tokens.len() && keywords_at(ctx, kind, &tokens[..at], &tokens[at].text).len() > 1 {
        return Parsed::Ambiguous;
    }
    if furthest == Fail::Incomplete
        && let Some(last) = tokens.last()
        && keywords_at(ctx, kind, &tokens[..tokens.len() - 1], &last.text).len() > 1
    {
        return Parsed::Ambiguous;
    }
    match furthest {
        Fail::Incomplete => Parsed::Incomplete,
        Fail::Invalid(i) => Parsed::Invalid(tokens[i.min(tokens.len() - 1)].start),
    }
}

/// What could come next after `before` (all fully typed): one entry per
/// distinct keyword or placeholder, with its help, plus `<cr>` when a
/// command can end here.
pub(crate) fn next_options(ctx: Ctx, kind: DeviceKind, before: &[Token]) -> Vec<(String, String)> {
    let mut options: Vec<(String, String)> = Vec::new();
    let mut can_end = false;
    for command in commands_for(ctx, kind) {
        let elems = elems(command.syntax);
        for (next, arg_index) in continuations(&elems, before, kind) {
            match next {
                None => can_end = true,
                Some(Elem::Keyword(k)) => push(
                    &mut options,
                    k.to_string(),
                    keyword_help(k, ctx).to_string(),
                ),
                Some(Elem::Iface) | Some(Elem::Target) => {
                    if matches!(next, Some(Elem::Target)) {
                        push(&mut options, "A.B.C.D".into(), arg_help(command, arg_index));
                    }
                    for (full, help) in iface::types_for(kind) {
                        push(&mut options, full.into(), help.into());
                    }
                }
                Some(Elem::Ip) => {
                    push(&mut options, "A.B.C.D".into(), arg_help(command, arg_index))
                }
                Some(Elem::Word) => push(&mut options, "WORD".into(), arg_help(command, arg_index)),
                Some(Elem::Line) => push(&mut options, "LINE".into(), arg_help(command, arg_index)),
                Some(Elem::Num(a, b)) => push(
                    &mut options,
                    format!("<{a}-{b}>"),
                    arg_help(command, arg_index),
                ),
                Some(Elem::Optional(_)) => {}
            }
        }
    }
    options.sort_by_key(|a| a.0.to_ascii_lowercase());
    if can_end {
        options.push(("<cr>".into(), String::new()));
    }
    options
}

fn push(options: &mut Vec<(String, String)>, name: String, help: String) {
    if !options.iter().any(|(n, _)| *n == name) {
        options.push((name, help));
    }
}

fn arg_help(command: &Command, index: usize) -> String {
    command.args.get(index).copied().unwrap_or("").to_string()
}

/// Keywords at the position after `before` that start with `partial`.
pub(crate) fn keywords_at(
    ctx: Ctx,
    kind: DeviceKind,
    before: &[Token],
    partial: &str,
) -> Vec<&'static str> {
    let mut words: Vec<&'static str> = Vec::new();
    for command in commands_for(ctx, kind) {
        for (next, _) in continuations(&elems(command.syntax), before, kind) {
            if let Some(Elem::Keyword(k)) = next
                && keyword_matches(k, partial)
                && !words.contains(&k)
            {
                words.push(k);
            }
        }
    }
    words.sort_unstable();
    words
}

/// The element that could follow `before` in a command (`None` = the
/// command may end), with the index of that placeholder among the args.
fn continuations(elems: &[Elem], before: &[Token], kind: DeviceKind) -> Vec<(Option<Elem>, usize)> {
    let mut out = Vec::new();
    // Try every split point: the command must match `before` exactly as a prefix.
    fn walk(
        elems: &[Elem],
        before: &[Token],
        kind: DeviceKind,
        ei: usize,
        ti: usize,
        args: usize,
        out: &mut Vec<(Option<Elem>, usize)>,
    ) {
        if ti == before.len() {
            match elems.get(ei) {
                None => out.push((None, args)),
                Some(Elem::Optional(inner)) => {
                    out.push((Some((**inner).clone()), args));
                    walk(elems, before, kind, ei + 1, ti, args + 1, out);
                }
                Some(e) => out.push((Some(e.clone()), args)),
            }
            return;
        }
        let Some(elem) = elems.get(ei) else { return };
        let token = &before[ti];
        let elem = match elem {
            Elem::Optional(inner) => (**inner).clone(),
            e => e.clone(),
        };
        let is_arg = !matches!(elem, Elem::Keyword(_));
        let next_args = args + usize::from(is_arg);
        match &elem {
            Elem::Keyword(k) => {
                if keyword_matches(k, &token.text) {
                    walk(elems, before, kind, ei + 1, ti + 1, next_args, out);
                }
            }
            Elem::Ip => {
                if parse_ip(&token.text).is_some() {
                    walk(elems, before, kind, ei + 1, ti + 1, next_args, out);
                }
            }
            Elem::Word => walk(elems, before, kind, ei + 1, ti + 1, next_args, out),
            Elem::Line => {}
            Elem::Num(a, b) => {
                if token
                    .text
                    .parse::<u32>()
                    .is_ok_and(|n| (*a..=*b).contains(&n))
                {
                    walk(elems, before, kind, ei + 1, ti + 1, next_args, out);
                }
            }
            Elem::Iface | Elem::Target => {
                if elem == Elem::Target && parse_ip(&token.text).is_some() {
                    walk(elems, before, kind, ei + 1, ti + 1, next_args, out);
                }
                if iface::parse(&token.text, kind).is_some() {
                    walk(elems, before, kind, ei + 1, ti + 1, next_args, out);
                }
                if let Some(next) = before.get(ti + 1)
                    && iface::parse_two(&token.text, &next.text, kind).is_some()
                {
                    walk(elems, before, kind, ei + 1, ti + 2, next_args, out);
                }
            }
            Elem::Optional(_) => {}
        }
    }
    walk(elems, before, kind, 0, 0, 0, &mut out);
    out
}
