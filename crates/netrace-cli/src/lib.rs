//! An IOS-style command line for netrace routers and switches.
//!
//! A [`Session`] is one console connection to one device. It keeps the mode
//! (`R1>`, `R1#`, `R1(config-if)#` …), parses each line the way IOS does
//! (abbreviations, `?`, Tab, `do`, `no`), edits the device's configuration
//! in the [`Project`] and runs `ping` / `traceroute` / `show` against the
//! [`Network`].

mod grammar;
mod iface;
mod render;
mod switching;
#[cfg(test)]
mod tests;

use std::net::Ipv4Addr;

use netrace_doc::{
    DeviceConfig, DeviceId, DeviceKind, InterfaceAddress, LineConfig, Project, RouteTarget,
    StaticRoute, mask,
};
use netrace_engine::{Network, interface_rows, routing_table};
use netrace_sim::SimTime;

pub use grammar::Ctx;
use grammar::{Parsed, Value};

/// `show running-config` for a router or switch, without a session.
pub fn running_config(project: &Project, device: DeviceId) -> Option<String> {
    let d = project.device(device)?;
    d.kind.has_cli().then(|| render::running_config(d))
}

/// What a line did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Response {
    /// Text to print after the echoed line.
    pub output: String,
    /// The prompt for the next line (a password prompt or `[confirm]`, too).
    pub prompt: String,
    /// The next line is a password: don't echo it.
    pub secret: bool,
    /// The device's configuration or name changed.
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Mode {
    User,
    Privileged,
    Config,
    Interface(String),
    Subinterface(String),
    /// `interface range`: several ports configured together.
    Range(Vec<String>),
    Line(String),
    /// `vlan 10` (or several VLANs at once).
    Vlan(Vec<u16>),
}

/// A question waiting for the next line.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    LoginPassword { attempts: u8 },
    EnablePassword { attempts: u8 },
    CopyDestination,
    SaveBeforeReload,
    ConfirmReload,
    ConfirmErase,
    Banner { delimiter: char, text: String },
}

/// One console session on a router or switch.
#[derive(Debug, Clone)]
pub struct Session {
    device: DeviceId,
    mode: Mode,
    pending: Option<Pending>,
    history: Vec<String>,
}

impl Session {
    pub fn new(device: DeviceId) -> Self {
        Session {
            device,
            mode: Mode::User,
            pending: None,
            history: Vec::new(),
        }
    }

    pub fn device(&self) -> DeviceId {
        self.device
    }

    /// The mode, for the app (e.g. to show which commands apply).
    pub fn context(&self) -> Ctx {
        match self.mode {
            Mode::User => Ctx::User,
            Mode::Privileged => Ctx::Privileged,
            Mode::Config => Ctx::Config,
            Mode::Interface(_) | Mode::Range(_) => Ctx::Interface,
            Mode::Subinterface(_) => Ctx::Subinterface,
            Mode::Line(_) => Ctx::Line,
            Mode::Vlan(_) => Ctx::Vlan,
        }
    }

    /// What the console shows when it's opened: the banner, then the login
    /// prompt if the console line has a password.
    pub fn start(&mut self, project: &Project) -> Response {
        let Some(device) = project.device(self.device) else {
            return Response::default();
        };
        let mut output = String::new();
        if let Some(banner) = &device.config.banner_motd {
            output += banner;
            output.push('\n');
        }
        let console = device.config.lines.get("con 0");
        if let Some(LineConfig {
            login: true,
            password: Some(_),
            ..
        }) = console
        {
            output += "\nUser Access Verification\n";
            self.pending = Some(Pending::LoginPassword { attempts: 0 });
            return Response {
                output,
                prompt: "Password: ".into(),
                secret: true,
                changed: false,
            };
        }
        Response {
            output,
            prompt: self.prompt(project),
            ..Response::default()
        }
    }

    pub fn prompt(&self, project: &Project) -> String {
        let name = project
            .device(self.device)
            .map(|d| d.name.as_str())
            .unwrap_or("Router");
        match self.mode {
            Mode::User => format!("{name}>"),
            Mode::Privileged => format!("{name}#"),
            Mode::Config => format!("{name}(config)#"),
            Mode::Interface(_) => format!("{name}(config-if)#"),
            Mode::Range(_) => format!("{name}(config-if-range)#"),
            Mode::Subinterface(_) => format!("{name}(config-subif)#"),
            Mode::Vlan(_) => format!("{name}(config-vlan)#"),
            Mode::Line(_) => format!("{name}(config-line)#"),
        }
    }

    /// Runs one line. `now` is when a `ping` or `traceroute` starts.
    pub fn execute(
        &mut self,
        line: &str,
        project: &mut Project,
        network: &mut Network,
        now: SimTime,
    ) -> Response {
        let Some(kind) = project.device(self.device).map(|d| d.kind) else {
            return Response::default();
        };
        if !kind.has_cli() {
            return Response {
                output: "This device has no command line.".into(),
                ..Response::default()
            };
        }
        if let Some(pending) = self.pending.take() {
            return self.answer(pending, line, project);
        }
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            self.history.push(trimmed.to_string());
            if self.history.len() > 20 {
                self.history.remove(0);
            }
        }
        let before = project.device_settings(self.device).ok();
        let mut output = self.run(line, self.context(), kind, project, network, now);
        let changed = before != project.device_settings(self.device).ok();
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        let (prompt, secret) = self.pending_prompt(project);
        Response {
            output,
            prompt,
            secret,
            changed,
        }
    }

    fn pending_prompt(&self, project: &Project) -> (String, bool) {
        match &self.pending {
            Some(Pending::LoginPassword { .. } | Pending::EnablePassword { .. }) => ("Password: ".into(), true),
            Some(Pending::CopyDestination) => ("Destination filename [startup-config]? ".into(), false),
            Some(Pending::SaveBeforeReload) => ("System configuration has been modified. Save? [yes/no]: ".into(), false),
            Some(Pending::ConfirmReload) => ("Proceed with reload? [confirm]".into(), false),
            Some(Pending::ConfirmErase) => (
                "Erasing the nvram filesystem will remove all configuration files! Continue? [confirm]".into(),
                false,
            ),
            Some(Pending::Banner { .. }) => (String::new(), false),
            None => (self.prompt(project), false),
        }
    }

    /// Lists what can be typed next, as `?` does. `line` is everything
    /// typed before the `?`.
    pub fn help(&self, line: &str, project: &Project) -> String {
        let Some(kind) = project.device(self.device).map(|d| d.kind) else {
            return String::new();
        };
        let (ctx, line) = self.effective_context(line);
        let tokens = grammar::tokenize(line);
        let ends_with_space = line.is_empty() || line.ends_with(char::is_whitespace);
        if !ends_with_space {
            // "sh?": the keywords that start with what's typed.
            let (last, before) = tokens.split_last().expect("not empty");
            let words = grammar::keywords_at(ctx, kind, before, &last.text);
            let types: Vec<&str> = if words.is_empty() {
                iface::types_for(kind)
                    .into_iter()
                    .map(|(full, _)| full)
                    .filter(|t| {
                        t.to_ascii_lowercase()
                            .starts_with(&last.text.to_ascii_lowercase())
                    })
                    .collect()
            } else {
                Vec::new()
            };
            if words.is_empty() && types.is_empty() {
                return "% Unrecognized command\n".into();
            }
            return format!(
                "{}\n",
                words
                    .iter()
                    .chain(types.iter())
                    .copied()
                    .collect::<Vec<_>>()
                    .join("  ")
            );
        }
        let options = grammar::next_options(ctx, kind, &tokens);
        if options.is_empty() {
            return "% Unrecognized command\n".into();
        }
        let mut out = String::new();
        for (name, help) in options {
            if help.is_empty() {
                out += &format!("  {name}\n");
            } else {
                out += &format!("  {name:<18}{help}\n");
            }
        }
        out
    }

    /// Completes the last word, as Tab does; `None` if it isn't unique.
    pub fn complete(&self, line: &str, project: &Project) -> Option<String> {
        let kind = project.device(self.device)?.kind;
        if line.is_empty() || line.ends_with(char::is_whitespace) {
            return None;
        }
        let (ctx, effective) = self.effective_context(line);
        let tokens = grammar::tokenize(effective);
        let (last, before) = tokens.split_last()?;
        let words = grammar::keywords_at(ctx, kind, before, &last.text);
        let completion: String = match words.as_slice() {
            [one] => (*one).to_string(),
            [] => iface::expand_type(&last.text, kind)?.to_string(),
            _ => return None,
        };
        let cut = line.len() - last.text.len();
        Some(format!("{}{completion} ", &line[..cut]))
    }

    /// The previous lines typed in this session (`show history`).
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// `do …` runs privileged commands from any configuration mode.
    fn effective_context<'a>(&self, line: &'a str) -> (Ctx, &'a str) {
        let ctx = self.context();
        let trimmed = line.trim_start();
        if ctx.is_config()
            && let Some(rest) = trimmed.strip_prefix("do ")
        {
            return (Ctx::Privileged, rest);
        }
        (ctx, line)
    }

    fn answer(&mut self, pending: Pending, line: &str, project: &mut Project) -> Response {
        let device = project
            .device(self.device)
            .cloned()
            .expect("checked by caller");
        let mut output = String::new();
        let mut changed = false;
        match pending {
            Pending::LoginPassword { attempts } => {
                let expected = device
                    .config
                    .lines
                    .get("con 0")
                    .and_then(|l| l.password.clone());
                if expected.as_deref() == Some(line) {
                    self.mode = Mode::User;
                } else if attempts >= 2 {
                    output += "% Bad passwords\n";
                    self.pending = Some(Pending::LoginPassword { attempts: 0 });
                } else {
                    self.pending = Some(Pending::LoginPassword {
                        attempts: attempts + 1,
                    });
                }
            }
            Pending::EnablePassword { attempts } => {
                let ok = match (&device.config.enable_secret, &device.config.enable_password) {
                    (Some(secret), _) => secret == line,
                    (None, Some(password)) => password == line,
                    (None, None) => true,
                };
                if ok {
                    self.mode = Mode::Privileged;
                } else if attempts >= 2 {
                    output += "% Bad secrets\n";
                } else {
                    self.pending = Some(Pending::EnablePassword {
                        attempts: attempts + 1,
                    });
                }
            }
            Pending::CopyDestination => {
                let name = line.trim();
                if name.is_empty() || "startup-config".starts_with(name) {
                    let _ = project.save_startup_config(self.device);
                    output += "Building configuration...\n[OK]\n";
                    changed = true;
                } else {
                    output += &format!("%Error opening flash:{name} (Permission denied)\n");
                }
            }
            Pending::SaveBeforeReload => {
                let answer = line.trim().to_ascii_lowercase();
                if "yes".starts_with(&answer) && !answer.is_empty() {
                    let _ = project.save_startup_config(self.device);
                    output += "Building configuration...\n[OK]\n";
                    changed = true;
                    self.pending = Some(Pending::ConfirmReload);
                } else if "no".starts_with(&answer) && !answer.is_empty() {
                    self.pending = Some(Pending::ConfirmReload);
                } else {
                    output += "% Please answer 'yes' or 'no'.\n";
                    self.pending = Some(Pending::SaveBeforeReload);
                }
            }
            Pending::ConfirmReload => {
                if line.trim().is_empty() || line.trim().eq_ignore_ascii_case("y") {
                    let _ = project.reload_device(self.device);
                    changed = true;
                    self.mode = Mode::User;
                    output += "\nSystem Bootstrap, Version 15.4 (simulated)\n\nRestarting with the startup-config...\n\n";
                    let after = project.device(self.device).cloned().expect("still there");
                    if let Some(banner) = &after.config.banner_motd {
                        output += banner;
                        output.push('\n');
                    }
                    output += "\nPress RETURN to get started!\n";
                }
            }
            Pending::ConfirmErase => {
                if line.trim().is_empty() || line.trim().eq_ignore_ascii_case("y") {
                    let mut settings = project.device_settings(self.device).expect("exists");
                    settings.startup = None;
                    let _ = project.set_device_settings(self.device, settings);
                    output += "[OK]\nErase of nvram: complete\n";
                    changed = true;
                }
            }
            Pending::Banner {
                delimiter,
                mut text,
            } => {
                if let Some(at) = line.find(delimiter) {
                    text += &line[..at];
                    self.update(project, |c| c.banner_motd = Some(text.clone()));
                    changed = true;
                } else {
                    text += line;
                    text.push('\n');
                    self.pending = Some(Pending::Banner { delimiter, text });
                }
            }
        }
        let (prompt, secret) = self.pending_prompt(project);
        Response {
            output,
            prompt,
            secret,
            changed,
        }
    }

    fn run(
        &mut self,
        line: &str,
        ctx: Ctx,
        kind: DeviceKind,
        project: &mut Project,
        network: &mut Network,
        now: SimTime,
    ) -> String {
        let (command, values) = match grammar::parse(line, ctx, kind) {
            Parsed::Empty => return String::new(),
            Parsed::Ok(command, values) => (command, values),
            Parsed::Ambiguous => return format!("% Ambiguous command:  \"{}\"", line.trim()),
            Parsed::Incomplete => return "% Incomplete command.".into(),
            Parsed::Invalid(at) => {
                let tokens = grammar::tokenize(line);
                let config = project
                    .device(self.device)
                    .map(|d| d.config.clone())
                    .unwrap_or_default();
                // A lone unknown word in EXEC mode is taken as a host name to
                // connect to, which is why labs start with `no ip domain-lookup`.
                if matches!(ctx, Ctx::User | Ctx::Privileged)
                    && tokens.len() == 1
                    && at == tokens[0].start
                {
                    let word = &tokens[0].text;
                    return if config.domain_lookup {
                        format!(
                            "Translating \"{word}\"...domain server (255.255.255.255)\n\
                             % Unknown command or computer name, or unable to find computer address"
                        )
                    } else {
                        "% Unknown command or computer name, or unable to find computer address"
                            .into()
                    };
                }
                let prompt_len = self.prompt(project).chars().count();
                return format!(
                    "{}^\n% Invalid input detected at '^' marker.",
                    " ".repeat(prompt_len + at)
                );
            }
        };
        self.dispatch(command.id, &values, line, kind, project, network, now)
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch(
        &mut self,
        id: &str,
        values: &[Value],
        line: &str,
        kind: DeviceKind,
        project: &mut Project,
        network: &mut Network,
        now: SimTime,
    ) -> String {
        let ips: Vec<Ipv4Addr> = values
            .iter()
            .filter_map(|v| match v {
                Value::Ip(ip) => Some(*ip),
                _ => None,
            })
            .collect();
        let nums: Vec<u32> = values
            .iter()
            .filter_map(|v| match v {
                Value::Num(n) => Some(*n),
                _ => None,
            })
            .collect();
        let word = values.iter().find_map(|v| match v {
            Value::Word(w) | Value::Line(w) => Some(w.clone()),
            _ => None,
        });
        let iface = values.iter().find_map(|v| match v {
            Value::Iface(name) => Some(name.clone()),
            _ => None,
        });
        let device = project
            .device(self.device)
            .cloned()
            .expect("checked by caller");
        // Keep the simulation in step with configuration typed in this session.
        if network.project().simulation_key() != project.simulation_key() {
            *network = Network::new(project.clone());
        }

        match id {
            "enable" => {
                if device.config.enable_secret.is_some() || device.config.enable_password.is_some()
                {
                    self.pending = Some(Pending::EnablePassword { attempts: 0 });
                } else {
                    self.mode = Mode::Privileged;
                }
                String::new()
            }
            "disable" => {
                self.mode = Mode::User;
                String::new()
            }
            "exit" | "logout" => {
                self.mode = Mode::User;
                format!(
                    "\n{} con0 is now available\n\nPress RETURN to get started.",
                    device.name
                )
            }
            "ping" | "ping-repeat" => {
                let count = nums.first().copied().unwrap_or(5).min(1000) as u16;
                match network.ping(self.device, ips[0], count, now) {
                    Ok(session) => network.session_report(session).unwrap_or_default(),
                    Err(e) => format!("% {e}"),
                }
            }
            "traceroute" => match network.traceroute(self.device, ips[0], now) {
                Ok(session) => network.session_report(session).unwrap_or_default(),
                Err(e) => format!("% {e}"),
            },
            "configure" => {
                self.mode = Mode::Config;
                "Enter configuration commands, one per line.  End with CNTL/Z.".into()
            }
            "copy-run-start" => {
                self.pending = Some(Pending::CopyDestination);
                String::new()
            }
            "copy-start-run" => match device.startup.clone() {
                Some(saved) => {
                    let _ = project.set_device_config(self.device, saved.config);
                    let _ = project.rename_device(self.device, &saved.hostname);
                    format!(
                        "Destination filename [running-config]? \n{} bytes copied",
                        1024
                    )
                }
                None => "%Error opening nvram:/startup-config (No such file or directory)".into(),
            },
            "write" => {
                let _ = project.save_startup_config(self.device);
                "Building configuration...\n[OK]".into()
            }
            "write-erase" | "erase-startup" => {
                self.pending = Some(Pending::ConfirmErase);
                String::new()
            }
            "reload" => {
                let saved = device.startup.as_ref().map(|s| (&s.hostname, &s.config));
                let modified = saved != Some((&device.name, &device.config));
                self.pending = Some(if modified {
                    Pending::SaveBeforeReload
                } else {
                    Pending::ConfirmReload
                });
                String::new()
            }
            "clear-arp" => {
                // ARP caches are rebuilt from scratch on the next replay.
                *network = Network::new(project.clone());
                String::new()
            }
            "show-version" => render::version(&device),
            "show-run" => render::running_config(&device),
            "show-start" => render::startup_config(&device),
            "show-ip-brief" => render::ip_interface_brief(&interface_rows(project, self.device)),
            "show-ip-route" => {
                render::ip_route(kind, &device.config, &routing_table(project, self.device))
            }
            "show-interfaces" => {
                let rows = interface_rows(project, self.device);
                match iface {
                    Some(name) => match rows.iter().find(|r| r.name == name) {
                        Some(row) => render::interfaces(std::slice::from_ref(row), kind),
                        None => "% Invalid input detected at '^' marker.".into(),
                    },
                    None => render::interfaces(&rows, kind),
                }
            }
            "show-arp" | "show-ip-arp" => render::arp(network, self.device),
            "show-mac" => render::mac_table(network, self.device),
            "show-history" => self.history.iter().map(|h| format!("  {h}\n")).collect(),

            "hostname" => {
                let name = word.unwrap_or_default();
                match project.rename_device(self.device, &name) {
                    Ok(_) => String::new(),
                    Err(netrace_doc::DocError::DuplicateName(_)) => {
                        format!("% Another device in this topology is already named {name}.")
                    }
                    Err(_) => "% Hostname contains one or more illegal characters.".into(),
                }
            }
            "interface" => {
                let name = iface.expect("matched an interface");
                // Creating a subinterface, loopback or VLAN interface happens here.
                self.update(project, |c| {
                    c.interface_mut(kind, &name);
                });
                let note = String::new();
                self.mode = if name.contains('.') {
                    Mode::Subinterface(name)
                } else {
                    Mode::Interface(name)
                };
                note
            }
            "ip-route" | "no-ip-route" => {
                self.ip_route(id == "ip-route", &ips, &iface, &nums, project, &device)
            }
            "ip-default-gateway" => {
                self.update(project, |c| c.default_gateway = Some(ips[0]));
                String::new()
            }
            "no-ip-default-gateway" => {
                self.update(project, |c| c.default_gateway = None);
                String::new()
            }
            "ip-routing" => {
                self.update(project, |c| c.ip_routing = true);
                String::new()
            }
            "no-ip-routing" => {
                self.update(project, |c| c.ip_routing = false);
                String::new()
            }
            "ip-routing-router" => String::new(),
            "enable-secret" => {
                self.update(project, |c| c.enable_secret = word.clone());
                String::new()
            }
            "enable-password" => {
                let note = if device.config.enable_secret.is_some() {
                    "The enable password you have chosen is the same as your enable secret.\n\
                     This is not recommended.  Re-enter the enable password."
                } else {
                    ""
                };
                if note.is_empty() || device.config.enable_secret != word {
                    self.update(project, |c| c.enable_password = word.clone());
                    String::new()
                } else {
                    note.into()
                }
            }
            "no-enable-secret" => {
                self.update(project, |c| c.enable_secret = None);
                String::new()
            }
            "no-enable-password" => {
                self.update(project, |c| c.enable_password = None);
                String::new()
            }
            "no-domain-lookup" => {
                self.update(project, |c| c.domain_lookup = false);
                String::new()
            }
            "domain-lookup" => {
                self.update(project, |c| c.domain_lookup = true);
                String::new()
            }
            "service-pe" => {
                self.update(project, |c| c.password_encryption = true);
                String::new()
            }
            "no-service-pe" => {
                self.update(project, |c| c.password_encryption = false);
                String::new()
            }
            "banner-motd" => {
                let text = word.unwrap_or_default();
                let mut chars = text.chars();
                let Some(delimiter) = chars.next() else {
                    return "% Incomplete command.".into();
                };
                let rest: String = chars.collect();
                if let Some(at) = rest.find(delimiter) {
                    let banner = rest[..at].to_string();
                    self.update(project, |c| c.banner_motd = Some(banner.clone()));
                    String::new()
                } else {
                    self.pending = Some(Pending::Banner {
                        delimiter,
                        text: if rest.is_empty() {
                            String::new()
                        } else {
                            format!("{rest}\n")
                        },
                    });
                    format!("Enter TEXT message.  End with the character '{delimiter}'.")
                }
            }
            "no-banner-motd" => {
                self.update(project, |c| c.banner_motd = None);
                String::new()
            }
            "line-con" => {
                self.mode = Mode::Line("con 0".into());
                String::new()
            }
            "line-vty" => {
                let (first, last) = (nums[0], nums.get(1).copied().unwrap_or(nums[0]));
                // Labs configure `line vty 0 4` (or 0 15); both are kept under one key.
                let key = if first == 0 && last >= 4 {
                    "vty 0 4".to_string()
                } else {
                    format!("vty {first} {last}")
                };
                self.mode = Mode::Line(key);
                String::new()
            }

            "ip-address" | "no-ip-address" => {
                self.ip_address(id == "ip-address", &ips, kind, project, &device)
            }
            "shutdown" | "no-shutdown" => {
                let shut = id == "shutdown";
                let mut messages = Vec::new();
                for name in self.interface_names() {
                    let was_shut = device.config.interface(kind, &name).shutdown;
                    self.update(project, |c| c.interface_mut(kind, &name).shutdown = shut);
                    if was_shut != shut {
                        messages.push(link_messages(project, self.device, &name, shut));
                    }
                }
                messages.join("\n")
            }
            "description" => {
                let text: String = word.unwrap_or_default().chars().take(240).collect();
                for name in self.interface_names() {
                    self.update(project, |c| {
                        c.interface_mut(kind, &name).description = Some(text.clone())
                    });
                }
                String::new()
            }
            "no-description" => {
                for name in self.interface_names() {
                    self.update(project, |c| c.interface_mut(kind, &name).description = None);
                }
                String::new()
            }
            "encapsulation" | "encapsulation-native" => {
                let name = self.interface_name().expect("interface modes only");
                let native = id == "encapsulation-native";
                self.update(project, |c| {
                    let sub = c.interface_mut(kind, &name);
                    sub.encapsulation = Some(nums[0] as u16);
                    sub.native = native;
                });
                String::new()
            }

            "password" | "no-password" => {
                let key = self.line_key().expect("line mode only");
                let value = if id == "password" { word } else { None };
                self.update(project, |c| {
                    c.lines.entry(key.clone()).or_default().password = value.clone()
                });
                String::new()
            }
            "login" | "no-login" => {
                let key = self.line_key().expect("line mode only");
                let has_password = device
                    .config
                    .lines
                    .get(&key)
                    .and_then(|l| l.password.as_ref())
                    .is_some();
                self.update(project, |c| {
                    c.lines.entry(key.clone()).or_default().login = id == "login"
                });
                if id == "login" && !has_password {
                    "% Login disabled on line 0, until 'password' is set".into()
                } else {
                    String::new()
                }
            }
            "logging-sync" | "no-logging-sync" => {
                let key = self.line_key().expect("line mode only");
                self.update(project, |c| {
                    c.lines.entry(key.clone()).or_default().logging_synchronous =
                        id == "logging-sync"
                });
                String::new()
            }
            "exec-timeout" => {
                let key = self.line_key().expect("line mode only");
                let timeout = (nums[0] as u16, nums.get(1).copied().unwrap_or(0) as u16);
                self.update(project, |c| {
                    c.lines.entry(key.clone()).or_default().exec_timeout = Some(timeout)
                });
                String::new()
            }

            "exit-config" => {
                self.mode = match self.mode {
                    Mode::Config => Mode::Privileged,
                    _ => Mode::Config,
                };
                if self.mode == Mode::Privileged {
                    "%SYS-5-CONFIG_I: Configured from console by console".into()
                } else {
                    String::new()
                }
            }
            "end" => {
                self.mode = Mode::Privileged;
                "%SYS-5-CONFIG_I: Configured from console by console".into()
            }
            "do" => {
                let command = word.unwrap_or_default();
                let saved = self.mode.clone();
                self.mode = Mode::Privileged;
                let out = self.run(&command, Ctx::Privileged, kind, project, network, now);
                // `do` never changes the mode (a `do exit` doesn't leave config).
                self.mode = saved;
                out
            }
            other => {
                let _ = line;
                self.switching(other, values, kind, project, network)
                    .unwrap_or_else(|| format!("% {other} is not available yet."))
            }
        }
    }

    /// The interfaces the current mode configures (one, or a range).
    pub(crate) fn interface_names(&self) -> Vec<String> {
        match &self.mode {
            Mode::Interface(name) | Mode::Subinterface(name) => vec![name.clone()],
            Mode::Range(names) => names.clone(),
            _ => Vec::new(),
        }
    }

    fn interface_name(&self) -> Option<String> {
        match &self.mode {
            Mode::Interface(name) | Mode::Subinterface(name) => Some(name.clone()),
            _ => None,
        }
    }

    fn line_key(&self) -> Option<String> {
        match &self.mode {
            Mode::Line(key) => Some(key.clone()),
            _ => None,
        }
    }

    /// Edits the device's running configuration.
    pub(crate) fn update(&self, project: &mut Project, edit: impl FnOnce(&mut DeviceConfig)) {
        let Some(mut config) = project.device(self.device).map(|d| d.config.clone()) else {
            return;
        };
        edit(&mut config);
        let _ = project.set_device_config(self.device, config);
    }

    fn ip_address(
        &mut self,
        set: bool,
        ips: &[Ipv4Addr],
        kind: DeviceKind,
        project: &mut Project,
        device: &netrace_doc::Device,
    ) -> String {
        let name = self.interface_name().expect("interface modes only");
        if !set {
            self.update(project, |c| c.interface_mut(kind, &name).address = None);
            return String::new();
        }
        if kind.is_switch()
            && kind.has_port(&name)
            && !device.config.interface(kind, &name).switchport.routed
        {
            return "% A switch port works at layer 2 and has no IP address of its own.\n\
                    % Give the switch an address on `interface vlan 1` instead."
                .into();
        }
        let (address, netmask) = (ips[0], ips[1]);
        let Some(prefix_len) = netrace_doc::prefix_from_mask(netmask) else {
            return format!("% Bad mask {netmask} for address {address}");
        };
        let candidate = InterfaceAddress {
            address,
            prefix_len,
        };
        if prefix_len < 31 && (address == candidate.network() || address == candidate.broadcast()) {
            return format!("% Bad mask /{prefix_len} for address {address}");
        }
        for (other, config) in &device.config.interfaces {
            if *other == name {
                continue;
            }
            if let Some(a) = config.address {
                let shorter = a.prefix_len.min(prefix_len);
                let m = u32::from(mask(shorter));
                if u32::from(a.address) & m == u32::from(address) & m {
                    return format!("% {} overlaps with {other}", candidate.network());
                }
            }
        }
        self.update(project, |c| {
            c.interface_mut(kind, &name).address = Some(candidate)
        });
        String::new()
    }

    fn ip_route(
        &mut self,
        add: bool,
        ips: &[Ipv4Addr],
        iface: &Option<String>,
        nums: &[u32],
        project: &mut Project,
        device: &netrace_doc::Device,
    ) -> String {
        let (network, netmask) = (ips[0], ips[1]);
        let Some(prefix_len) = netrace_doc::prefix_from_mask(netmask) else {
            return "%Inconsistent address and mask".into();
        };
        if u32::from(network) & !u32::from(netmask) != 0 {
            return "%Inconsistent address and mask".into();
        }
        let target = match (ips.get(2), iface) {
            (Some(nh), _) => Some(RouteTarget::NextHop(*nh)),
            (None, Some(name)) => Some(RouteTarget::Interface(name.clone())),
            (None, None) => None,
        };
        if !add {
            self.update(project, |c| {
                c.routes.retain(|r| {
                    !(r.network == network
                        && r.prefix_len == prefix_len
                        && target.as_ref().is_none_or(|t| r.target == *t))
                })
            });
            return String::new();
        }
        let Some(target) = target else {
            return "% Incomplete command.".into();
        };
        if let RouteTarget::NextHop(nh) = &target
            && device
                .config
                .interfaces
                .values()
                .any(|i| i.address.is_some_and(|a| a.address == *nh))
        {
            return "%Invalid next hop address (it's this router)".into();
        }
        let route = StaticRoute {
            network,
            prefix_len,
            target,
            distance: nums.first().copied().unwrap_or(1) as u8,
        };
        self.update(project, |c| {
            // Same prefix and target: replaced (a new distance), as IOS does.
            c.routes.retain(|r| {
                !(r.network == route.network
                    && r.prefix_len == route.prefix_len
                    && r.target == route.target)
            });
            c.routes.push(route.clone());
        });
        String::new()
    }
}

/// The console messages IOS prints when an interface goes up or down.
fn link_messages(project: &Project, device: DeviceId, name: &str, shut: bool) -> String {
    if shut {
        return format!(
            "%LINK-5-CHANGED: Interface {name}, changed state to administratively down\n\
             %LINEPROTO-5-UPDOWN: Line protocol on Interface {name}, changed state to down"
        );
    }
    let up = interface_rows(project, device)
        .iter()
        .find(|r| r.name == name)
        .is_some_and(|r| r.is_up());
    if up {
        format!(
            "%LINK-5-CHANGED: Interface {name}, changed state to up\n\
             %LINEPROTO-5-UPDOWN: Line protocol on Interface {name}, changed state to up"
        )
    } else {
        format!("%LINK-5-CHANGED: Interface {name}, changed state to down")
    }
}
