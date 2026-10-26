//! IOS-style consoles on routers and switches.

use std::sync::{Arc, Mutex, MutexGuard};

use netrace_cli as cli;
use netrace_doc as doc;
use netrace_sim::SimTime;

use crate::{CoreError, Project, Simulation};

/// One interface of a router or switch, as `show ip interface brief` lists it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InterfaceStatusInfo {
    pub name: String,
    pub address: Option<String>,
    pub prefix_len: Option<u8>,
    /// "up", "down" or "administratively down".
    pub status: String,
    pub protocol: String,
    pub description: Option<String>,
}

#[uniffi::export]
impl Project {
    /// `show running-config` text for a router or switch.
    pub fn running_config(&self, device: u64) -> Option<String> {
        cli::running_config(&self.lock(), doc::DeviceId(device))
    }

    /// Interface states of a router or switch.
    pub fn interface_status(&self, device: u64) -> Vec<InterfaceStatusInfo> {
        netrace_engine::interface_rows(&self.lock(), doc::DeviceId(device))
            .into_iter()
            .map(|r| InterfaceStatusInfo {
                name: r.name,
                address: r.address.map(|a| a.to_string()),
                prefix_len: r.prefix_len,
                status: r.status.to_string(),
                protocol: r.protocol.to_string(),
                description: r.description,
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConsoleResponse {
    /// Text to print after the echoed line.
    pub output: String,
    /// The next prompt (`R1(config)#`, `Password: `, `[confirm]`…).
    pub prompt: String,
    /// The next line is a password: don't echo it.
    pub secret: bool,
    /// The device's settings before this line, when it changed them
    /// (opaque; hand to `Project.set_device_settings` to undo).
    pub previous_settings: Option<String>,
}

/// One console session on a router or switch: its mode, pending prompts
/// and history live here between lines.
#[derive(uniffi::Object)]
pub struct Console {
    inner: Mutex<cli::Session>,
}

#[uniffi::export]
impl Console {
    #[uniffi::constructor]
    pub fn new(device: u64) -> Arc<Self> {
        Arc::new(Console {
            inner: Mutex::new(cli::Session::new(doc::DeviceId(device))),
        })
    }

    /// What the console shows when opened (banner, login prompt).
    pub fn start(&self, project: Arc<Project>) -> ConsoleResponse {
        let response = self.lock().start(&project.lock());
        ConsoleResponse {
            output: response.output,
            prompt: response.prompt,
            secret: response.secret,
            previous_settings: None,
        }
    }

    pub fn prompt(&self, project: Arc<Project>) -> String {
        self.lock().prompt(&project.lock())
    }

    /// Runs one line. Configuration changes go straight into the project;
    /// pings and traceroutes start at `at_ns` in the simulation.
    pub fn execute(
        &self,
        line: String,
        project: Arc<Project>,
        simulation: Arc<Simulation>,
        at_ns: u64,
    ) -> ConsoleResponse {
        let mut session = self.lock();
        let mut project = project.lock();
        let before = project.device_settings(session.device()).ok();
        let mut network = simulation.lock();
        let response = session.execute(
            &line,
            &mut project,
            &mut network,
            SimTime::from_nanos(at_ns),
        );
        let previous_settings = if response.changed {
            before.map(|s| serde_json::to_string(&s).expect("settings always serialise"))
        } else {
            None
        };
        ConsoleResponse {
            output: response.output,
            prompt: response.prompt,
            secret: response.secret,
            previous_settings,
        }
    }

    /// `?`: what can be typed after `line`.
    pub fn help(&self, line: String, project: Arc<Project>) -> String {
        self.lock().help(&line, &project.lock())
    }

    /// Tab: `line` with its last word completed, if unique.
    pub fn complete(&self, line: String, project: Arc<Project>) -> Option<String> {
        self.lock().complete(&line, &project.lock())
    }
}

impl Console {
    fn lock(&self) -> MutexGuard<'_, cli::Session> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceKind;

    #[test]
    fn console_edits_the_project_with_undo() {
        let project = Project::new();
        let r1 = project.add_device(DeviceKind::Router, 0.0, 0.0);
        let simulation = Simulation::new(project.clone());
        let console = Console::new(r1.id);
        assert_eq!(console.start(project.clone()).prompt, "R1>");
        for line in ["enable", "configure terminal"] {
            let r = console.execute(line.into(), project.clone(), simulation.clone(), 0);
            assert!(r.previous_settings.is_none());
        }
        let r = console.execute(
            "hostname Edge".into(),
            project.clone(),
            simulation.clone(),
            0,
        );
        assert_eq!(r.prompt, "Edge(config)#");
        let previous = r.previous_settings.expect("hostname changed the device");
        assert_eq!(project.snapshot().devices[0].name, "Edge");
        let key = project.simulation_key();
        project.set_device_settings(r1.id, previous).unwrap();
        assert_eq!(project.snapshot().devices[0].name, "R1");
        assert_ne!(project.simulation_key(), key);
        assert!(
            console
                .help("int".into(), project.clone())
                .contains("interface")
        );
        assert_eq!(
            console.complete("hostn".into(), project.clone()).as_deref(),
            Some("hostname ")
        );
    }
}

/// Something that differs between devices sharing a configuration: the
/// hostname, an interface address, the default gateway.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConfigVariable {
    /// "hostname", "address:GigabitEthernet0/0" or "default-gateway".
    pub key: String,
    /// "Hostname", "Gi0/0 address", "Default gateway".
    pub label: String,
    /// "R1", "192.168.1.1/24", "192.168.1.254"; empty for none.
    pub value: String,
}

#[uniffi::export]
impl Project {
    /// The parts of a router's or switch's configuration that usually
    /// differ from device to device, with their current values.
    pub fn config_variables(&self, device: u64) -> Vec<ConfigVariable> {
        let project = self.lock();
        let Some(d) = project.device(doc::DeviceId(device)) else {
            return Vec::new();
        };
        let mut vars = vec![ConfigVariable {
            key: "hostname".into(),
            label: "Hostname".into(),
            value: d.name.clone(),
        }];
        let mut names: Vec<&String> = d
            .config
            .interfaces
            .iter()
            .filter(|(_, i)| i.address.is_some())
            .map(|(n, _)| n)
            .collect();
        names.sort_by_key(|n| netrace_engine::natural_key(n));
        for name in names {
            let a = d.config.interfaces[name].address.expect("filtered");
            vars.push(ConfigVariable {
                key: format!("address:{name}"),
                label: format!("{} address", netrace_engine::abbreviate_port(name)),
                value: format!("{}/{}", a.address, a.prefix_len),
            });
        }
        if let Some(gw) = d.config.default_gateway {
            vars.push(ConfigVariable {
                key: "default-gateway".into(),
                label: "Default gateway".into(),
                value: gw.to_string(),
            });
        }
        vars
    }

    /// Gives `to` the configuration of `from`, with `values` filled in for
    /// its variables (see `config_variables`). All or nothing. Returns the
    /// target's previous settings, for undo.
    pub fn copy_config(
        &self,
        from: u64,
        to: u64,
        values: Vec<ConfigVariable>,
    ) -> Result<String, CoreError> {
        let mut project = self.lock();
        let invalid = |message: String| CoreError::InvalidOperation { message };
        let source = project
            .device(doc::DeviceId(from))
            .ok_or_else(|| invalid("The source device no longer exists.".into()))?
            .clone();
        let target = project
            .device(doc::DeviceId(to))
            .ok_or_else(|| invalid("The target device no longer exists.".into()))?
            .clone();
        if source.kind != target.kind {
            return Err(invalid(format!(
                "{} and {} are different models, so their configurations don't fit each other.",
                source.name, target.name
            )));
        }
        let mut config = source.config.clone();
        let mut name = target.name.clone();
        for v in &values {
            let value = v.value.trim();
            if v.key == "hostname" {
                name = value.to_string();
            } else if v.key == "default-gateway" {
                config.default_gateway = if value.is_empty() {
                    None
                } else {
                    Some(value.parse().map_err(|_| {
                        invalid(format!(
                            "{}: \"{value}\" isn't an IPv4 address.",
                            target.name
                        ))
                    })?)
                };
            } else if let Some(iface) = v.key.strip_prefix("address:") {
                let address = if value.is_empty() {
                    None
                } else {
                    Some(parse_cidr(value).ok_or_else(|| {
                        invalid(format!(
                            "{} {}: write the address as 192.168.1.1/24.",
                            target.name,
                            netrace_engine::abbreviate_port(iface)
                        ))
                    })?)
                };
                config.interface_mut(target.kind, iface).address = address;
            }
        }
        let settings = doc::DeviceSettings {
            name,
            ip: target.ip,
            dhcp: target.dhcp,
            config,
            startup: target.startup.clone(),
        };
        let previous = project
            .set_device_settings(target.id, settings)
            .map_err(crate::invalid_operation)?;
        Ok(serde_json::to_string(&previous).expect("settings always serialise"))
    }
}

/// "192.168.1.1/24" or "192.168.1.1 255.255.255.0".
fn parse_cidr(text: &str) -> Option<doc::InterfaceAddress> {
    let (address, rest) = text
        .split_once('/')
        .or_else(|| text.split_once(char::is_whitespace))?;
    let address = address.trim().parse().ok()?;
    let rest = rest.trim();
    let prefix_len = match rest.parse::<u8>() {
        Ok(p) if p <= 32 => p,
        _ => doc::prefix_from_mask(rest.parse().ok()?)?,
    };
    Some(doc::InterfaceAddress {
        address,
        prefix_len,
    })
}

#[cfg(test)]
mod copy_tests {
    use super::*;
    use crate::DeviceKind;

    #[test]
    fn copy_with_variations() {
        let project = Project::new();
        let r1 = project.add_device(DeviceKind::Router, 0.0, 0.0);
        let r2 = project.add_device(DeviceKind::Router, 0.0, 0.0);
        let pc = project.add_device(DeviceKind::Pc, 0.0, 0.0);
        let simulation = Simulation::new(project.clone());
        let console = Console::new(r1.id);
        for line in [
            "en",
            "conf t",
            "no ip domain-lookup",
            "int g0/0",
            "ip address 10.1.0.1 255.255.255.0",
            "no shut",
            "exit",
            "ip route 0.0.0.0 0.0.0.0 10.1.0.254",
        ] {
            console.execute(line.into(), project.clone(), simulation.clone(), 0);
        }
        let vars = project.config_variables(r1.id);
        assert_eq!(vars.len(), 2, "{vars:?}");
        assert_eq!(vars[1].value, "10.1.0.1/24");
        let values = vec![
            ConfigVariable {
                key: "hostname".into(),
                label: String::new(),
                value: "Branch2".into(),
            },
            ConfigVariable {
                key: "address:GigabitEthernet0/0".into(),
                label: String::new(),
                value: "10.2.0.1/24".into(),
            },
        ];
        let previous = project.copy_config(r1.id, r2.id, values.clone()).unwrap();
        let copied = project.config_variables(r2.id);
        assert_eq!(copied[0].value, "Branch2");
        assert_eq!(copied[1].value, "10.2.0.1/24");
        assert!(
            project
                .running_config(r2.id)
                .unwrap()
                .contains("no ip domain lookup")
        );
        assert!(
            project
                .running_config(r2.id)
                .unwrap()
                .contains("ip route 0.0.0.0 0.0.0.0 10.1.0.254")
        );
        project.set_device_settings(r2.id, previous).unwrap();
        assert_eq!(project.config_variables(r2.id)[0].value, "R2");
        assert!(project.copy_config(r1.id, pc.id, values).is_err());
        let bad = vec![ConfigVariable {
            key: "address:GigabitEthernet0/0".into(),
            label: String::new(),
            value: "10.2.0.1".into(),
        }];
        assert!(project.copy_config(r1.id, r2.id, bad).is_err());
    }
}
