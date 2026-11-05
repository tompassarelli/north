use super::config::Config;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Graph {
    order: Vec<String>,
    edges: HashMap<String, Vec<String>>,
}

impl Graph {
    pub(super) fn insert(&mut self, node: &str) {
        if !self.edges.contains_key(node) {
            self.order.push(node.to_owned());
            self.edges.insert(node.to_owned(), Vec::new());
        }
    }

    pub(super) fn link(&mut self, from: &str, to: &str) {
        self.insert(from);
        let targets = self.edges.get_mut(from).expect("inserted");
        if !targets.iter().any(|target| target == to) {
            targets.push(to.to_owned());
        }
    }

    pub(super) fn nodes(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.order.iter().map(|node| (node.as_str(), self.edges[node].as_slice()))
    }

    fn targets(&self, node: &str) -> &[String] {
        self.edges.get(node).map_or(&[], Vec::as_slice)
    }
}

pub(super) fn sccs(graph: &Graph) -> Vec<Vec<String>> {
    let mut index = 0;
    let mut indices: HashMap<&str, usize> = HashMap::new();
    let mut low: HashMap<&str, usize> = HashMap::new();
    let mut on_stack: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = Vec::new();
    let mut out: Vec<Vec<String>> = Vec::new();
    for (root, _) in graph.nodes() {
        if indices.contains_key(root) {
            continue;
        }
        let mut work: Vec<(&str, usize)> = Vec::new();
        indices.insert(root, index);
        low.insert(root, index);
        index += 1;
        stack.push(root);
        on_stack.insert(root);
        work.push((root, 0));
        while let Some(&(node, next)) = work.last() {
            let targets = graph.targets(node);
            if next < targets.len() {
                work.last_mut().expect("nonempty").1 += 1;
                let target = targets[next].as_str();
                if !indices.contains_key(target) {
                    indices.insert(target, index);
                    low.insert(target, index);
                    index += 1;
                    stack.push(target);
                    on_stack.insert(target);
                    work.push((target, 0));
                } else if on_stack.contains(target) {
                    let lowest = low[node].min(indices[target]);
                    low.insert(node, lowest);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                let lowest = low[parent].min(low[node]);
                low.insert(parent, lowest);
            }
            if low[node] == indices[node] {
                let mut component = Vec::new();
                loop {
                    let member = stack.pop().expect("on stack");
                    on_stack.remove(member);
                    component.push(member.to_owned());
                    if member == node {
                        break;
                    }
                }
                if component.len() > 1 {
                    component.sort();
                    out.push(component);
                }
            }
        }
    }
    out.sort_by_key(|component| std::cmp::Reverse(component.len()));
    out
}

pub(super) fn component_of(cycles: &[Vec<String>]) -> HashMap<&str, usize> {
    cycles.iter().enumerate().flat_map(|(index, cycle)| cycle.iter().map(move |file| (file.as_str(), index))).collect()
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct LayerViolation {
    pub(super) from: String,
    pub(super) to: String,
    #[serde(rename = "fromModule")]
    pub(super) from_module: String,
    #[serde(rename = "toModule")]
    pub(super) to_module: String,
}

pub(super) fn layer_violation(config: &Config, from: &str, to: &str) -> Option<LayerViolation> {
    let a = config.module_of(from)?;
    let b = config.module_of(to)?;
    if a.name == b.name || a.facade || a.may_use.contains(&b.name) {
        return None;
    }
    Some(LayerViolation { from: from.to_owned(), to: to.to_owned(), from_module: a.name.clone(), to_module: b.name.clone() })
}

pub(super) struct Fan {
    pub(super) fan_in: Vec<(String, usize)>,
    pub(super) fan_out: Vec<(String, usize)>,
}

pub(super) fn fan(graph: &Graph) -> Fan {
    let mut fan_in: Vec<(String, usize)> = Vec::new();
    let mut position: HashMap<String, usize> = HashMap::new();
    let mut fan_out = Vec::new();
    for (node, targets) in graph.nodes() {
        fan_out.push((node.to_owned(), targets.len()));
        for target in targets {
            let at = *position.entry(target.clone()).or_insert_with(|| {
                fan_in.push((target.clone(), 0));
                fan_in.len() - 1
            });
            fan_in[at].1 += 1;
        }
    }
    Fan { fan_in, fan_out }
}

pub(super) fn module_graph(config: &Config, graph: &Graph) -> Graph {
    let mut modules = Graph::default();
    for module in &config.modules {
        modules.insert(&module.name);
    }
    for (node, targets) in graph.nodes() {
        let Some(a) = config.module_of(node) else { continue };
        for target in targets {
            if let Some(b) = config.module_of(target).filter(|b| b.name != a.name) {
                modules.link(&a.name, &b.name);
            }
        }
    }
    modules
}
