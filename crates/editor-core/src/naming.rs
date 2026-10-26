//! Deterministic net-name allocation.

use std::collections::BTreeSet;

use sim_core::netlist::MAX_NAME_LEN;

/// Result of allocating a net name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocatedName {
    pub name: String,
    pub renamed: bool,
}

/// Allocates unique node names, reserving `0` for ground.
#[derive(Clone, Debug)]
pub struct NetNameAllocator {
    used: BTreeSet<String>,
    next_generated: u64,
}

impl NetNameAllocator {
    /// Creates an allocator with `0` already reserved.
    pub fn new() -> Self {
        let mut used = BTreeSet::new();
        used.insert("0".to_owned());
        Self {
            used,
            next_generated: 1,
        }
    }

    /// Reserves an additional name without allocating it.
    pub fn reserve(&mut self, name: impl Into<String>) {
        self.used.insert(name.into());
    }

    /// Returns true when a name is already reserved.
    pub fn contains(&self, name: &str) -> bool {
        self.used.contains(name)
    }

    /// Allocates a preferred label or a generated `N$nnn` name.
    pub fn allocate(&mut self, preferred: Option<&str>) -> AllocatedName {
        if let Some(preferred) = preferred.map(str::trim).filter(|name| !name.is_empty()) {
            if self.used.insert(preferred.to_owned()) {
                return AllocatedName {
                    name: preferred.to_owned(),
                    renamed: false,
                };
            }
            let mut suffix = 2_u64;
            loop {
                let suffix_text = format!("#{suffix}");
                let base_length = MAX_NAME_LEN.saturating_sub(suffix_text.chars().count());
                let base: String = preferred.chars().take(base_length).collect();
                let candidate = format!("{base}{suffix_text}");
                if self.used.insert(candidate.clone()) {
                    return AllocatedName {
                        name: candidate,
                        renamed: true,
                    };
                }
                suffix += 1;
            }
        }

        loop {
            let candidate = format!("N${:03}", self.next_generated);
            self.next_generated += 1;
            if self.used.insert(candidate.clone()) {
                return AllocatedName {
                    name: candidate,
                    renamed: false,
                };
            }
        }
    }
}

impl Default for NetNameAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unique_labels_and_renames_duplicates() {
        let mut allocator = NetNameAllocator::new();
        assert_eq!(
            allocator.allocate(Some("OUT")),
            AllocatedName {
                name: "OUT".to_owned(),
                renamed: false
            }
        );
        assert_eq!(
            allocator.allocate(Some("OUT")),
            AllocatedName {
                name: "OUT#2".to_owned(),
                renamed: true
            }
        );
    }

    #[test]
    fn keeps_renamed_labels_within_the_length_limit() {
        let mut allocator = NetNameAllocator::new();
        let long = "x".repeat(MAX_NAME_LEN);
        let first = allocator.allocate(Some(&long));
        assert_eq!(first.name, long);
        assert!(!first.renamed);

        let second = allocator.allocate(Some(&long));
        assert_eq!(second.name.chars().count(), MAX_NAME_LEN);
        assert!(second.name.ends_with("#2"));
        assert!(second.renamed);
    }

    #[test]
    fn generates_unique_names_around_labels() {
        let mut allocator = NetNameAllocator::new();
        allocator.allocate(Some("N$001"));
        assert_eq!(
            allocator.allocate(None),
            AllocatedName {
                name: "N$002".to_owned(),
                renamed: false
            }
        );
        assert_eq!(
            allocator.allocate(Some("0")),
            AllocatedName {
                name: "0#2".to_owned(),
                renamed: true
            }
        );
    }
}
