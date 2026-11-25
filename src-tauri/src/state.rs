use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::RwLock;

use crate::error::{AppError, AppResult};
use crate::modules::{self, InstalledModule};
use crate::paths::{parse_service_id, Layout};
use crate::registry::{Registry, ServiceSpec};
use crate::settings::Settings;
use crate::supervisor::{Launch, Supervisor};

pub struct AppState {
    pub settings: RwLock<Settings>,
    pub settings_path: PathBuf,
    /// Module registry (fixed at build time; tests point it elsewhere).
    pub registry_url: String,
    pub client: reqwest::Client,
    pub supervisor: Supervisor,
    /// Module names with an install/uninstall in flight.
    pub busy_modules: std::sync::Mutex<HashSet<String>>,
    pub quitting: AtomicBool,
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn layout(&self) -> Layout {
        Layout::new(self.settings().data_dir)
    }

    pub fn registry<'a>(&'a self, layout: &'a Layout) -> Registry<'a> {
        Registry { client: &self.client, base_url: &self.registry_url, layout }
    }

    /// Mutates and persists settings in one step.
    pub fn update_settings<T>(&self, f: impl FnOnce(&mut Settings) -> AppResult<T>) -> AppResult<T> {
        let mut guard = self.settings.write().unwrap_or_else(|e| e.into_inner());
        let mut next = guard.clone();
        let out = f(&mut next)?;
        next.save(&self.settings_path)?;
        *guard = next;
        Ok(out)
    }

    pub fn port_for(&self, id: &str, spec: &ServiceSpec) -> Option<u16> {
        self.settings().service(id).port.or_else(|| modules::default_port(spec))
    }

    /// Resolves an installed service into a ready-to-spawn launch plan.
    pub fn launch_for(&self, id: &str) -> AppResult<(Launch, Option<u16>, InstalledModule)> {
        let (name, version) = parse_service_id(id)?;
        let layout = self.layout();
        let m = modules::load_installed(&layout, &name, &version)?;
        let spec = m.service.clone().ok_or(AppError::NoService)?;
        let port = self.port_for(id, &spec);
        modules::write_configs(&layout, &m, port, false)?;

        let vars = modules::template_vars(&layout, &m, port);
        let (program, args) = modules::resolve_command(&layout, &m, spec.args(), &vars)?;
        let launch = Launch {
            id: id.to_string(),
            program,
            args,
            cwd: layout.conf_dir(&name, &version),
            log_file: layout.logs().join(format!("{name}-{version}.log")),
            auto_restart: self.settings().auto_restart,
        };
        Ok((launch, port, m))
    }
}
