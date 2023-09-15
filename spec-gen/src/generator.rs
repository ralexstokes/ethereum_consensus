use crate::visitors::{
    collate_generics_from, collect_lifetimes, generics_to_arguments, ArgumentsEditor,
    TypeNameVisitor,
};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::PathBuf,
    rc::Rc,
};
use syn::{parse_quote, Ident, Item};

const SOURCE_ROOT: &str = "../ethereum-consensus/src";

fn as_syn_ident(s: String) -> Ident {
    syn::parse_str(&s).unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
enum Fork {
    #[default]
    Phase0,
    Altair,
    Bellatrix,
    Capella,
}

impl Fork {
    fn name(&self) -> String {
        format!("{self:?}").to_lowercase()
    }

    fn modules_in_diff(&self) -> &[&str] {
        match self {
            Self::Phase0 => &[
                "beacon_block",
                "beacon_state",
                "block_processing",
                "constants",
                "epoch_processing",
                "fork",
                "genesis",
                "helpers",
                "operations",
                "slot_processing",
                "state_transition",
                "validator",
            ],
            Self::Altair => &[
                "beacon_block",
                "beacon_state",
                "block_processing",
                "constants",
                "epoch_processing",
                "fork",
                "genesis",
                "helpers",
                "light_client",
                "sync",
                "validator",
            ],
            Self::Bellatrix => &[
                "beacon_block",
                "beacon_state",
                "blinded_beacon_block",
                "block_processing",
                "epoch_processing",
                "execution",
                "fork_choice",
                "fork",
                "genesis",
                "helpers",
                "state_transition",
            ],
            Self::Capella => &[
                "beacon_block",
                "beacon_state",
                "blinded_beacon_block",
                "bls_to_execution_change",
                // "block_processing",
                "epoch_processing",
                "execution",
                // "fork_choice",
                // "fork",
                // "genesis",
                "helpers",
                // "state_transition",
                "withdrawal",
            ],
        }
    }

    fn should_filter_fn_by_name(&self, name: &str) -> bool {
        match self {
            Fork::Altair => {
                matches!(
                    name,
                    "get_unslashed_attesting_indices" |
                        "get_matching_source_attestations" |
                        "get_matching_target_attestations" |
                        "get_matching_head_attestations" |
                        "get_source_deltas" |
                        "get_target_deltas" |
                        "get_head_deltas" |
                        "get_inclusion_delay_deltas" |
                        "get_inactivity_penalty_deltas" |
                        "get_attestation_deltas" |
                        "get_attestation_component_deltas" |
                        "get_attesting_balance" |
                        "process_participation_record_updates"
                )
            }
            Fork::Bellatrix => {
                matches!(name, "upgrade_to_altair" | "translate_participation")
            }
            Fork::Capella => {
                matches!(name, "upgrade_to_bellatrix")
            }
            _ => false,
        }
    }

    fn imports(&self) -> Vec<syn::Item> {
        match self {
            Fork::Phase0 => vec![],
            Fork::Altair => {
                let fragment: syn::File = parse_quote! {
                    use std::cmp;
                    use std::collections::HashSet;
                    use ssz_rs::prelude::*;
                    use crate::crypto::{hash, verify_signature, fast_aggregate_verify};
                    use crate::ssz::*;
                };
                fragment.items
            }
            Fork::Bellatrix => {
                let fragment: syn::File = parse_quote! {
                    use std::mem;
                    use std::cmp;
                    use std::collections::{HashSet, HashMap};
                    use std::iter::zip;
                    use ssz_rs::prelude::*;
                    use integer_sqrt::IntegerSquareRoot;
                    use crate::crypto::{hash, verify_signature, fast_aggregate_verify, eth_aggregate_public_keys, eth_fast_aggregate_verify};
                    use crate::ssz::*;
                    // NOTE: expose items for use by others...
                    pub use crate::bellatrix::execution::ExecutionEngine;
                };
                fragment.items
            }
            Fork::Capella => {
                let fragment: syn::File = parse_quote! {
                    use std::mem;
                    use std::cmp;
                    use std::collections::{HashSet, HashMap};
                    use std::iter::zip;
                    use ssz_rs::prelude::*;
                    use integer_sqrt::IntegerSquareRoot;
                    use crate::crypto::{hash, verify_signature, fast_aggregate_verify, eth_aggregate_public_keys, eth_fast_aggregate_verify};
                    use crate::ssz::*;
                    // NOTE: expose items for use by others...
                    // pub use crate::bellatrix::execution::ExecutionEngine;
                };
                fragment.items
            }
        }
    }
}

#[derive(Clone, Debug)]
struct TypeDef {
    item: syn::ItemType,
    name: String,
    fork: Fork,
}

impl TypeDef {
    fn new(value: syn::ItemType, fork: Fork) -> Self {
        let name = value.ident.to_string();
        Self { item: value, name, fork }
    }

    fn is_pub(&self) -> bool {
        matches!(self.item.vis, syn::Visibility::Public(..))
    }
}

#[derive(Clone, Debug)]
struct Constant {
    item: syn::ItemConst,
    fork: Fork,
}

impl Constant {
    fn new(value: syn::ItemConst, fork: Fork) -> Self {
        Self { item: value, fork }
    }

    fn is_pub(&self) -> bool {
        matches!(self.item.vis, syn::Visibility::Public(..))
    }
}

#[derive(Clone)]
struct Fn {
    name: String,
    item: syn::ItemFn,
    fork: Fork,
    expand: bool,
}

impl Fn {
    fn new(value: syn::ItemFn, fork: Fork) -> Self {
        let sig = &value.sig;
        let name = &sig.ident;
        Self { name: name.to_string(), item: value, fork, expand: false }
    }

    fn is_pub(&self) -> bool {
        matches!(self.item.vis, syn::Visibility::Public(..))
    }
}

impl std::fmt::Debug for Fn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Fn").field(&self.name).finish()
    }
}

#[derive(Clone)]
struct Container {
    name: String,
    item: syn::ItemStruct,
    fork: Fork,
}

impl Container {
    fn new(value: syn::ItemStruct, fork: Fork) -> Self {
        let name = &value.ident;
        Self { name: name.to_string(), item: value, fork }
    }

    fn is_pub(&self) -> bool {
        matches!(self.item.vis, syn::Visibility::Public(..))
    }
}

impl std::fmt::Debug for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Container").field(&self.name).finish()
    }
}

#[derive(Default)]
struct Module {
    containers: Vec<Container>,
    fns: Vec<Fn>,
    constants: Vec<Constant>,
    type_defs: Vec<TypeDef>,
}

impl Module {
    fn merge(&mut self, other: Module) {
        for container in other.containers {
            self.containers.push(container);
        }
        for f in other.fns {
            self.fns.push(f);
        }
        for c in other.constants {
            self.constants.push(c);
        }
        for t in other.type_defs {
            self.type_defs.push(t);
        }
    }
}

impl std::fmt::Debug for Module {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Module")
            .field("containers", &self.containers)
            .field("fns", &self.fns)
            .finish()
    }
}

#[derive(Default, Debug)]
pub struct ForkDiff {
    modules: BTreeMap<String, Module>,
}

#[derive(Debug, Default)]
struct Spec {
    fork: Fork,
    diff: ForkDiff,
    items: Vec<Item>,
    // index from visible identifiers to the module they are defined in
    index: HashMap<String, String>,
}

impl Spec {
    fn parse(fork: Fork) -> Self {
        let (diff, index) = parse_fork_diff_with_symbol_index(&fork);
        Self { fork, diff, items: Default::default(), index }
    }

    fn on_top_of(&mut self, previous: Rc<Spec>) {
        let fork = self.fork.name();
        let index = &mut self.index;

        for (module_name, previous_module) in previous.diff.modules.iter() {
            let mut module = Module::default();
            for c in &previous_module.constants {
                let ident = &c.item.ident;
                let name = ident.to_string();
                if index.contains_key(&name) {
                    println!("skipping item: found duplicate definition for `{name}` in next spec `{fork}`");
                    continue
                }
                module.constants.push(c.clone());
                index.insert(name, module_name.to_string());
            }

            for container in &previous_module.containers {
                let name = container.name.to_string();
                if index.contains_key(&name) {
                    println!("skipping item: found duplicate definition for `{name}` in next spec `{fork}`");
                    continue
                }
                module.containers.push(container.clone());
                index.insert(name, module_name.to_string());
            }

            for type_def in &previous_module.type_defs {
                let name = type_def.name.to_string();
                if index.contains_key(&name) {
                    println!("skipping item: found duplicate definition for `{name}` in next spec `{fork}`");
                    continue
                }
                module.type_defs.push(type_def.clone());
                index.insert(name, module_name.to_string());
            }

            for f in &previous_module.fns {
                let mut f = f.clone();
                let name = f.name.to_string();
                if self.fork.should_filter_fn_by_name(&name) {
                    continue
                }
                if index.contains_key(&name) {
                    println!("skipping item: found duplicate definition for `{name}` in next spec `{fork}`");
                    continue
                }
                let item = &f.item;
                let mut fragment: syn::ItemFn = parse_quote!(
                    #item
                );
                let mut analyzer = TypeNameVisitor::default();
                analyzer.analyze(&fragment);
                let type_names = analyzer.names;
                let mut all_arguments = vec![];
                for name in &type_names {
                    if let Some(target_module) = index.get(name) {
                        let target_module = self.diff.modules.get(target_module).unwrap();
                        let container = target_module
                            .containers
                            .iter()
                            .find(|&c| &c.name == name)
                            .expect("internal state integrity");

                        let arguments = generics_to_arguments(&container.item.generics);
                        let mut editor = ArgumentsEditor::new(&container.name, &arguments);
                        editor.edit(&mut fragment);

                        all_arguments.push(arguments);
                        f.fork = self.fork;
                    }
                }

                let lifetimes = collect_lifetimes(&fragment);

                let generics = collate_generics_from(&all_arguments, &lifetimes);
                fragment.sig.generics = generics;

                f.item = fragment;
                f.expand = true;

                module.fns.push(f);
                index.insert(name, module_name.to_string());
            }

            let target_module = self.diff.modules.entry(module_name.to_string()).or_default();
            target_module.merge(module);
        }
    }

    fn assemble_items(&mut self) {
        // NOTE: iterate for _each_ "module type" to keep like things together from _all_ modules at
        // the spec level
        for (module_name, module) in self.diff.modules.iter() {
            let module_name: Ident = as_syn_ident(module_name.clone());

            for c in &module.constants {
                let ident = &c.item.ident;
                let fork_name = as_syn_ident(c.fork.name());
                let use_stmt = parse_quote! {
                    pub use crate::#fork_name::#module_name::#ident;
                };
                self.items.push(use_stmt);
            }
        }
        for (module_name, module) in self.diff.modules.iter() {
            let module_name: Ident = as_syn_ident(module_name.clone());

            for container in &module.containers {
                let item = &container.item;
                let ident = &item.ident;
                let fork_name = as_syn_ident(container.fork.name());
                let use_stmt = parse_quote! {
                    pub use crate::#fork_name::#module_name::#ident;
                };
                self.items.push(use_stmt);
            }
        }
        for (module_name, module) in self.diff.modules.iter() {
            let module_name: Ident = as_syn_ident(module_name.clone());

            for t in &module.type_defs {
                let ident = &t.item.ident;
                let fork_name = as_syn_ident(t.fork.name());
                let use_stmt = parse_quote! {
                    pub use crate::#fork_name::#module_name::#ident;
                };
                self.items.push(use_stmt);
            }
        }
        let mut all_fn_items = self
            .diff
            .modules
            .iter()
            .flat_map(|(module_name, module)| {
                module.fns.iter().map(|f| {
                    let module_name = as_syn_ident(module_name.clone());
                    (module_name, f)
                })
            })
            .collect::<Vec<_>>();

        // NOTE: keep expansions at end of generated code
        all_fn_items.sort_by_key(|(_, f)| f.expand);

        for (module_name, f) in all_fn_items {
            let item: Item = if f.expand {
                f.item.clone().into()
            } else {
                let ident = &f.item.sig.ident;
                let fork_name = as_syn_ident(f.fork.name());
                parse_quote! {
                    pub use crate::#fork_name::#module_name::#ident;
                }
            };
            self.items.push(item);
        }
    }
}

fn parse_fork_diff_with_symbol_index(fork: &Fork) -> (ForkDiff, HashMap<String, String>) {
    let fork_name = fork.name();
    let mut fork_diff = ForkDiff::default();
    let mut index = HashMap::default();
    for module_name in fork.modules_in_diff() {
        let source_path = format!("{SOURCE_ROOT}/{fork_name}/{module_name}.rs");
        let module_source = fs::read_to_string(&source_path).expect("exists");
        let file = syn::parse_file(&module_source).unwrap();
        let module =
            fork_diff.modules.entry(module_name.to_string()).or_insert_with(Default::default);
        for item in file.items {
            match item {
                Item::Use(item) => {
                    if matches!(item.vis, syn::Visibility::Public(..)) {
                        println!("skipping item: `use` statement in {source_path}");
                    }
                }
                Item::Const(item) => {
                    let item = Constant::new(item, *fork);
                    if item.is_pub() {
                        module.constants.push(item);
                    }
                }
                Item::Struct(item) => {
                    let item = Container::new(item, *fork);
                    if item.is_pub() {
                        index.insert(item.name.to_string(), module_name.to_string());
                        module.containers.push(item);
                    }
                }
                Item::Fn(item) => {
                    let item = Fn::new(item, *fork);
                    if item.is_pub() {
                        index.insert(item.name.to_string(), module_name.to_string());
                        module.fns.push(item);
                    }
                }
                Item::Type(item) => {
                    let item = TypeDef::new(item, *fork);
                    if item.is_pub() {
                        module.type_defs.push(item);
                    }
                }
                Item::Impl(_) => {
                    println!("skipping item: `impl` block in {source_path}");
                }
                Item::Trait(_) => {
                    println!("skipping item: `trait` block in {source_path}");
                }
                i => unimplemented!("{i:#?} from {source_path}"),
            }
        }
    }
    (fork_diff, index)
}

fn render(fork: &Fork, items: &[Item]) {
    let primitives_import = parse_quote! {
        pub use crate::primitives::*;
    };
    let signing_import = parse_quote! {
        pub use crate::signing::*;
    };
    let state_transition_import = parse_quote! {
        pub use crate::state_transition::*;
    };
    let mut all_items = vec![primitives_import, signing_import, state_transition_import];
    let imports_for_fork = fork.imports();
    all_items.extend(imports_for_fork);
    all_items.extend(items.iter().cloned());

    let warning = parse_quote! {
        //! WARNING: This file was derived by the `spec-gen` utility. DO NOT EDIT MANUALLY.
    };
    let target_file = syn::File { shebang: None, attrs: vec![warning], items: all_items };
    let output = prettyplease::unparse(&target_file);

    let fork_name = fork.name();
    let target_path = format!("{SOURCE_ROOT}/{fork_name}/spec/mod.rs");
    let target_path = PathBuf::from(target_path);
    fs::create_dir_all(target_path.parent().unwrap()).unwrap();
    fs::write(target_path, output).unwrap();
}

pub fn run() {
    let fork_sequence =
        [None, Some(Fork::Phase0), Some(Fork::Altair), Some(Fork::Bellatrix), Some(Fork::Capella)];

    let mut specs = HashMap::<_, Rc<_>>::new();
    for pair in fork_sequence.windows(2) {
        let previous_fork = pair[0];
        let previous_spec = previous_fork.as_ref().and_then(|fork| specs.get(fork).cloned());

        let target_fork = pair[1].expect("there is a fork");
        println!(">>> generating spec for fork {target_fork:?}");
        let mut target_spec = Spec::parse(target_fork);
        if let Some(previous) = previous_spec {
            target_spec.on_top_of(previous);
        }

        target_spec.assemble_items();

        render(&target_spec.fork, &target_spec.items);

        specs.insert(target_fork, Rc::new(target_spec));
    }
}
