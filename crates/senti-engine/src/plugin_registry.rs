//! Holds plugins together with their enabled state.

use senti_core::{Plugin, PluginManifest};

use crate::EngineError;

/// Plugin metadata plus current state, for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginInfo {
    pub manifest: PluginManifest,
    pub enabled: bool,
}

struct Slot {
    plugin: Box<dyn Plugin>,
    manifest: PluginManifest,
    enabled: bool,
}

#[derive(Default)]
pub(crate) struct PluginRegistry {
    slots: Vec<Slot>,
}

impl std::fmt::Debug for PluginRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.list()).finish()
    }
}

impl PluginRegistry {
    /// Register a plugin. A plugin whose id is already registered replaces
    /// the previous one, so ids stay unique.
    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        let manifest = plugin.manifest();
        let slot = Slot {
            plugin,
            manifest,
            enabled: manifest.enabled_by_default,
        };
        match self.slots.iter_mut().find(|s| s.manifest.id == manifest.id) {
            Some(existing) => *existing = slot,
            None => self.slots.push(slot),
        }
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), EngineError> {
        let slot = self
            .slots
            .iter_mut()
            .find(|s| s.manifest.id == id)
            .ok_or_else(|| EngineError::UnknownPlugin(id.to_owned()))?;
        slot.enabled = enabled;
        Ok(())
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        self.slots
            .iter()
            .map(|s| PluginInfo {
                manifest: s.manifest,
                enabled: s.enabled,
            })
            .collect()
    }

    pub fn enabled(&self) -> impl Iterator<Item = &dyn Plugin> {
        self.slots
            .iter()
            .filter(|s| s.enabled)
            .map(|s| s.plugin.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy(&'static str, bool);

    impl Plugin for Dummy {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: self.0,
                name: self.0,
                description: "",
                enabled_by_default: self.1,
            }
        }
    }

    #[test]
    fn registers_toggles_and_lists() {
        let mut registry = PluginRegistry::default();
        registry.register(Box::new(Dummy("a", true)));
        registry.register(Box::new(Dummy("b", false)));
        assert_eq!(registry.enabled().count(), 1);

        registry.set_enabled("b", true).unwrap();
        registry.set_enabled("a", false).unwrap();
        let states: Vec<_> = registry
            .list()
            .iter()
            .map(|p| (p.manifest.id, p.enabled))
            .collect();
        assert_eq!(states, [("a", false), ("b", true)]);
        assert!(matches!(
            registry.set_enabled("zzz", true),
            Err(EngineError::UnknownPlugin(_))
        ));
    }

    #[test]
    fn duplicate_ids_replace() {
        let mut registry = PluginRegistry::default();
        registry.register(Box::new(Dummy("a", true)));
        registry.register(Box::new(Dummy("a", false)));
        assert_eq!(registry.list().len(), 1);
        assert_eq!(registry.enabled().count(), 0);
    }
}
