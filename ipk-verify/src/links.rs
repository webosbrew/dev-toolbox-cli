use crate::Symlinks;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

impl Symlinks {
    pub fn links(&self, target: &PathBuf) -> Vec<PathBuf> {
        let mut result = Vec::new();
        for (k, v) in &self.mapping {
            if v == target {
                result.push(k.clone());
            }
        }
        return result;
    }

    pub fn new(links: HashMap<PathBuf, PathBuf>) -> Self {
        let mut mapping: HashMap<PathBuf, PathBuf> = HashMap::new();
        for (link, _) in &links {
            if let Some(t) = Self::final_target(&links, link) {
                mapping.insert(link.clone(), t);
            }
        }
        return Self { mapping };
    }

    fn final_target(links: &HashMap<PathBuf, PathBuf>, by: &PathBuf) -> Option<PathBuf> {
        let mut cur = by;
        let mut occurrences = HashSet::new();
        while let Some(v) = links.get(cur) {
            cur = v;
            if !occurrences.insert(v) {
                // Circular linkage detected
                return None;
            }
        }
        if cur == by {
            return None;
        }
        return Some(cur.clone());
    }
}
