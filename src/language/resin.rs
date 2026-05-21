use std::collections::HashMap;
use std::str::FromStr;

use super::Vector;
use super::{CategoricalSource, Clause, Source, Target};
use crate::channels::ipc::TypedWriter;
use crate::channels::manager::Manager;
use crate::circuit::category::Category;
use crate::circuit::leaf;
use crate::circuit::semiring::{LogProb, Semiring};
use crate::language::concepts::{ComparisonLiteral, ResinType};
use crate::language::matching::{
    args_of, cause_atom_base_name, cause_atom_name, has_variable_arg,
    parameterized_comparison_predicate, predicate_of, split_statements,
};
use crate::language::{asp::solve, Dnf};

/// The compiled Resin runtime: holds the parsed program, the reactive circuit
/// manager, and the comparison registry that maps Density/Number source atoms
/// to their registered threshold comparisons.
pub struct Resin<S: Semiring = LogProb> {
    /// The clauses (head if body rules) of the Resin program, dictating the
    /// structure of the ReactiveCircuits.
    pub clauses: Vec<Clause>,
    /// The sources of the Resin program, i.e., data channels that provide
    /// Probability, Number, Density or Boolean values for inference.
    pub sources: Vec<Source>,
    /// Categorical sources are mapped to atoms with different names instead of a true/false binary.
    pub categorical_sources: Vec<CategoricalSource>,
    /// The outputs of this Resin program, each represented by a single ReactiveCircuit.
    pub targets: Vec<Target>,
    /// The runtime manager controlling the inter process communication (IPC).
    pub manager: Manager<S>,
    /// The number of elements held by each Leaf, i.e., a batch size for inference.
    /// In the ProbGradient case, this is 1 + n_leafs instead, holding the computed
    /// probability and all gradients.
    value_size: usize,
    /// Maps each Density/Number source atom name to its registered comparisons:
    /// `(threshold, upper_tail, canonical_leaf_name)`.
    comparison_registry: HashMap<String, Vec<(f64, bool, String)>>,
    /// Maps each learnable probabilistic-clause parameter to the names of its
    /// positive-polarity cause leaves.  Key format: `"{predicate}#{clause_index}"`.
    /// All leaves in a group share the same underlying `P(...)` value and should
    /// be kept in sync during gradient-based learning via `fit_parameters`.
    pub parameter_groups: HashMap<String, Vec<String>>,
}

impl<S: Semiring> Resin<S> {
    /// Parses `model`, sets up signal leaves, runs Clingo to obtain stable
    /// models, and builds the reactive circuit for each declared target.
    ///
    /// `value_size` is the number of parallel value slots (e.g. particles).
    /// Set `verbose` to `true` to print intermediate ASP and circuit info.
    ///
    /// Currently only the first target is compiled (see the `TODO` in the body).
    pub fn compile(
        model: &str,
        value_size: usize,
        verbose: bool,
    ) -> Result<Resin<S>, Box<dyn std::error::Error>> {
        S::validate_value_size(value_size);

        // Parse and setup Resin runtime environment
        let mut resin: Resin<S> = model.parse().unwrap();
        resin.value_size = value_size;
        resin.manager.reactive_circuit.lock().unwrap().value_size = value_size;

        // Setup data distribution through signal leafs
        resin.value_size = value_size;
        resin.setup_signals()?;
        if verbose {
            println!(
                "Resin setup with {} source signals",
                resin.manager.reactive_circuit.lock().unwrap().leafs.len(),
            );
        }

        // Pass data to Clingo and obtain stable models
        // TODO: Handle multiple targets
        if !resin.targets.is_empty() {
            let target_index = 0;

            // Compile Resin into ASP
            let program = resin.to_asp(target_index);

            // Solve ASP and obtain DNF formula from which the target is removed
            let mut dnf = solve(&program);
            dnf.remove(&resin.targets[target_index].name);

            if verbose {
                println!(
                    "Compiled Resin for target atom {} into formula over {} models",
                    resin.targets[target_index].name,
                    dnf.clauses.len()
                );
            }

            // Create leaves for grounded FOL probabilistic cause atoms now that
            // Clingo has produced concrete groundings.
            resin.setup_fol_prob_signals(&dnf);

            // Semirings such as ProbGradient require value_size = f(n_leaves).
            // Apply the override now that all leaves are known.
            {
                let n_leaves = resin.manager.reactive_circuit.lock().unwrap().leafs.len();
                if let Some(auto_size) = S::auto_value_size(n_leaves) {
                    resin.value_size = auto_size;
                    let mut rc = resin.manager.reactive_circuit.lock().unwrap();
                    rc.value_size = auto_size;
                    for leaf in rc.leafs.iter_mut() {
                        leaf.resize_for_value_size(auto_size);
                    }
                }
            }

            // Build the RC from the DNF
            resin.circuit_from_dnf(dnf, &resin.targets[target_index].channel);
        }

        // Return the compiled Resin program
        Ok(resin)
    }

    /// Renders the full ASP program for `target_index`, including:
    /// - choice atoms for every referenced source,
    /// - helper grounding rules for variable comparison literals,
    /// - all clause rules,
    /// - the integrity constraint for the target.
    pub fn to_asp(&self, target_index: usize) -> String {
        let mut asp = "".to_string();

        for source in &self.sources {
            match source.message_type {
                // Probability and Boolean sources are simple probabilistic atoms.
                ResinType::Probability | ResinType::Boolean => {
                    // Referenced if the source name appears verbatim in a body literal, OR
                    // if a variable body literal shares the same predicate name (e.g. `active(T)`
                    // for source `active(hospital)`, or `rel(A, hub, B)` for `rel(x, hub, y)`).
                    let source_pred = predicate_of(&source.name);
                    let referenced = self.clauses.iter().any(|c| {
                        c.body.iter().any(|lit| {
                            let base = lit.trim_start_matches("not ");
                            base == source.name
                                || (predicate_of(base) == source_pred && has_variable_arg(base))
                        })
                    });
                    if referenced {
                        asp.push_str(&source.to_asp());
                    }
                }
                // Density and Number sources manifest as one choice atom per comparison.
                ResinType::Density | ResinType::Number => {
                    if let Some(comparisons) = self.comparison_registry.get(&source.name) {
                        for (_, _, canonical) in comparisons {
                            asp.push_str(&format!("{{{}}}.\n", canonical));
                        }
                    }
                }
                // Categorical sources are handled separately via `categorical_sources`.
                ResinType::Categorical => unreachable!(),
            }
        }

        // Categorical sources: exactly-one constraint enforces mutual exclusivity.
        for cat_source in &self.categorical_sources {
            let atoms = cat_source.categories.join(" ; ");
            asp.push_str(&format!("1 {{ {} }} 1.\n", atoms));
        }

        // For every variable comparison literal, emit one helper rule per matching source so
        // that Clingo can ground the parameterized predicate, e.g.:
        //   resin_distance_gt_100(hospital) :- distance_hospital_gt_100.
        let mut emitted: std::collections::HashSet<String> = std::collections::HashSet::new();
        for clause in &self.clauses {
            for comp in &clause.comparison_literals {
                if !comp.is_variable {
                    continue;
                }
                let pred = predicate_of(&comp.source_atom);
                let param_pred = parameterized_comparison_predicate(pred, comp.op, comp.threshold);
                for source in &self.sources {
                    match source.message_type {
                        ResinType::Density | ResinType::Number => {}
                        _ => continue,
                    }
                    if let Some(ground) = comp.ground_for(&source.name) {
                        if let Some(src_args) = args_of(&source.name) {
                            let rule = format!(
                                "{}({}) :- {}.\n",
                                param_pred,
                                src_args.join(", "),
                                ground.canonical_name
                            );
                            if emitted.insert(rule.clone()) {
                                asp.push_str(&rule);
                            }
                        }
                    }
                }
            }
        }

        // Each probabilistic clause gets its own independent auxiliary choice atom so that
        // multiple clauses for the same head implement noisy-OR rather than sharing a single
        // choice with an arbitrary weight.
        //
        // Ground head (no variables in args):
        //   unsafe <- P(0.2) if close(a, b).   →   {unsafe_cause_0}.
        //   unsafe <- P(0.5) if heavy(a).       →   {unsafe_cause_1}.
        //                                           unsafe :- unsafe_cause_0, close(a, b).
        //                                           unsafe :- unsafe_cause_1, heavy(a).
        //
        // FOL head (variable args): the cause atom carries the head args so Clingo
        // creates one independent choice per grounding, not a single shared flip.
        //   heads(C) <- P(0.6) if coin(C).   →   {heads_cause_0(C)} :- coin(C).
        //                                         heads(C) :- heads_cause_0(C), coin(C).
        let mut prob_head_counts: HashMap<String, usize> = HashMap::new();
        for clause in &self.clauses {
            if clause.probability.is_some() {
                let idx = prob_head_counts.entry(clause.head.clone()).or_insert(0);

                if has_variable_arg(&clause.head) {
                    // FOL head: parameterized cause atom.
                    let base = cause_atom_base_name(&clause.head, *idx);
                    let args_str = args_of(&clause.head)
                        .map(|a| a.join(", "))
                        .unwrap_or_default();

                    // Ground the choice on domain/structural body literals only —
                    // not on source-derived ones.  In Resin, the only ASP choice
                    // atoms are sources.  Including a source-derived literal
                    // (a direct source reference or a `resin_*` parameterised
                    // comparison atom) in the grounding condition makes the cause
                    // atom a *conditional* choice: when the source condition is
                    // false the cause is forced false, contributing weight (1−p)
                    // instead of 1.0 in the WMC product.  Excluding them keeps the
                    // cause as a free coin flip within its structural domain,
                    // matching the correct Noisy-OR WMC semantics.
                    let source_predicates: std::collections::HashSet<&str> =
                        self.sources.iter().map(|s| predicate_of(&s.name)).collect();
                    let domain_body: Vec<&str> = clause
                        .body
                        .iter()
                        .filter(|lit| {
                            let base = lit.trim_start_matches("not ");
                            let pred = predicate_of(base);
                            !source_predicates.contains(pred) && !base.starts_with("resin_")
                        })
                        .map(|s| s.as_str())
                        .collect();

                    if domain_body.is_empty() {
                        asp.push_str(&format!("{{{}({})}}.\n", base, args_str));
                    } else {
                        let domain_str = domain_body.join(", ");
                        asp.push_str(&format!("{{{}({})}} :- {}.\n", base, args_str, domain_str));
                    }
                    let mut rule = format!("{} :- {}({})", clause.head, base, args_str);
                    for lit in &clause.body {
                        rule.push_str(&format!(", {}", lit));
                    }
                    rule.push_str(".\n");
                    asp.push_str(&rule);
                } else {
                    // Ground head: flat cause atom.
                    let aux = cause_atom_name(&clause.head, *idx);
                    asp.push_str(&format!("{{{}}}.\n", aux));
                    let mut rule = format!("{} :- {}", clause.head, aux);
                    for lit in &clause.body {
                        rule.push_str(&format!(", {}", lit));
                    }
                    rule.push_str(".\n");
                    asp.push_str(&rule);
                }

                *idx += 1;
            } else {
                asp.push_str(&clause.to_asp());
            }
        }

        asp.push_str(&self.targets[target_index].to_asp());
        asp
    }

    /// Creates IPC leaf pairs for every declared source and for every
    /// probabilistic clause head.
    ///
    /// - `Probability`/`Boolean` sources get a single dual-reader leaf pair.
    /// - `Density`/`Number` sources get one dual-reader leaf pair per unique
    ///   comparison threshold found across all clauses.
    /// - Probabilistic clause heads get a complementary leaf pair seeded with
    ///   `p` and `1 − p`.
    ///
    /// Also populates `comparison_registry` so that `make_writer_for` can later
    /// build the correct fan-out writer.
    pub fn setup_signals(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        for source in &self.sources {
            match source.message_type {
                ResinType::Probability | ResinType::Boolean => {
                    // Single leaf pair, driven directly by the writer.
                    let idx_normal =
                        self.manager
                            .create_leaf(&source.name, Vector::zeros(self.value_size), 0.0);
                    let idx_inverted = self.manager.create_leaf(
                        &format!("-{}", source.name),
                        Vector::ones(self.value_size),
                        0.0,
                    );
                    self.manager
                        .read_dual(idx_normal, idx_inverted, &source.channel)?;
                }
                ResinType::Density | ResinType::Number => {
                    // One leaf pair per unique comparison found in clause bodies.
                    let comparisons = self.collect_comparisons_for(&source.name);
                    let mut registry_entry: Vec<(f64, bool, String)> = Vec::new();

                    for comp in comparisons {
                        let idx_normal =
                            self.manager
                                .create_leaf(&comp.canonical_name, Vector::zeros(1), 0.0);
                        let idx_inverted = self.manager.create_leaf(
                            &format!("-{}", comp.canonical_name),
                            Vector::ones(1),
                            0.0,
                        );
                        self.manager
                            .read_dual(idx_normal, idx_inverted, &comp.canonical_name)?;

                        registry_entry.push((
                            comp.threshold,
                            comp.is_upper_tail(),
                            comp.canonical_name.clone(),
                        ));
                    }

                    self.comparison_registry
                        .insert(source.name.clone(), registry_entry);
                }
                ResinType::Categorical => unreachable!(),
            }
        }

        // Categorical sources: one positive-only leaf per category, no complement.
        // Mutual exclusivity is enforced in the ASP via `1{...}1`, so negative
        // literals in Clingo models have no leaves and are skipped in circuit_from_dnf.
        for cat_source in &self.categorical_sources.clone() {
            let mut category_indices = Vec::new();
            for category in &cat_source.categories {
                let idx = self
                    .manager
                    .create_leaf(category, Vector::zeros(self.value_size), 0.0);
                category_indices.push(idx);
            }
            self.manager
                .read_categorical(category_indices, &cat_source.channel)?;
        }

        // Create leaves for ground-head probabilistic clauses, mirroring the
        // per-head indexing used in to_asp so that leaf names match the auxiliary
        // choice atoms emitted there.  FOL (variable-head) clauses are skipped
        // here; their leaves are created after Clingo grounding via
        // setup_fol_prob_signals, because the concrete ground atoms are unknown
        // until Clingo runs.
        let mut prob_head_counts: HashMap<String, usize> = HashMap::new();
        for clause in &self.clauses {
            if clause.probability.is_none() {
                continue;
            }
            if has_variable_arg(&clause.head) {
                continue;
            }
            let idx = prob_head_counts.entry(clause.head.clone()).or_insert(0);
            let aux = cause_atom_name(&clause.head, *idx);
            let category = Category::<S>::new(
                &aux,
                clause.probability.unwrap() * Vector::ones(self.value_size),
            );
            self.manager
                .create_leaf(&category.leafs[0].name, category.leafs[0].get_value(), 0.0);
            self.manager
                .create_leaf(&category.leafs[1].name, category.leafs[1].get_value(), 0.0);
            let group_key = format!("{}#{}", predicate_of(&clause.head), *idx);
            self.parameter_groups
                .entry(group_key)
                .or_default()
                .push(aux);
            *idx += 1;
        }

        Ok(())
    }

    /// Creates circuit leaves for grounded FOL probabilistic cause atoms found in `dnf`.
    ///
    /// Called after Clingo solving so the concrete groundings are known.  Scans
    /// every literal in every DNF clause for atoms whose predicate matches a
    /// `*_cause_N` base name derived from a variable-head probabilistic clause,
    /// then creates a `Category` leaf pair (probability `p`, complement `1−p`)
    /// for each distinct grounded atom found.
    fn setup_fol_prob_signals(&mut self, dnf: &Dnf) {
        // Build base_predicate → (probability, group_key) map for FOL probabilistic clauses.
        let mut prob_map: HashMap<String, (f64, String)> = HashMap::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for clause in &self.clauses {
            if let Some(p) = clause.probability {
                if has_variable_arg(&clause.head) {
                    let idx = counts.entry(clause.head.clone()).or_insert(0);
                    let base = cause_atom_base_name(&clause.head, *idx);
                    let group_key = format!("{}#{}", predicate_of(&clause.head), *idx);
                    prob_map.insert(base, (p, group_key));
                    *idx += 1;
                }
            }
        }

        if prob_map.is_empty() {
            return;
        }

        // Scan all DNF literals; both positive (heads_cause_0(c0)) and negative
        // (-heads_cause_0(c0)) forms appear because Clingo includes the complement.
        let mut created: std::collections::HashSet<String> = std::collections::HashSet::new();
        for model_clause in &dnf.clauses {
            for literal in model_clause {
                let atom = literal.trim_start_matches('-');
                for (base, (p, group_key)) in &prob_map {
                    let matches = atom == base.as_str() || atom.starts_with(&format!("{}(", base));
                    if matches && created.insert(atom.to_string()) {
                        let category = Category::<S>::new(atom, *p * Vector::ones(self.value_size));
                        self.manager.create_leaf(
                            &category.leafs[0].name,
                            category.leafs[0].get_value(),
                            0.0,
                        );
                        self.manager.create_leaf(
                            &category.leafs[1].name,
                            category.leafs[1].get_value(),
                            0.0,
                        );
                        self.parameter_groups
                            .entry(group_key.clone())
                            .or_default()
                            .push(atom.to_string());
                    }
                }
            }
        }
    }

    /// Returns all unique comparison literals across all clauses that reference `source_name`,
    /// including variable comparisons whose predicate matches the source's predicate.
    fn collect_comparisons_for(&self, source_name: &str) -> Vec<ComparisonLiteral> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for clause in &self.clauses {
            for comp in &clause.comparison_literals {
                if !comp.is_variable && comp.source_atom == source_name {
                    if seen.insert(comp.canonical_name.clone()) {
                        result.push(comp.clone());
                    }
                } else if comp.is_variable {
                    if let Some(ground) = comp.ground_for(source_name) {
                        if seen.insert(ground.canonical_name.clone()) {
                            result.push(ground);
                        }
                    }
                }
            }
        }
        result
    }

    /// Returns the typed writer for the source whose IPC channel matches
    /// `channel`.  Looks up the source atom name and delegates to
    /// `make_writer_for`.
    pub fn make_writer(
        &mut self,
        channel: &str,
    ) -> Result<TypedWriter, Box<dyn std::error::Error>> {
        let source_name = self
            .sources
            .iter()
            .find(|s| s.channel == channel)
            .map(|s| s.name.clone())
            .ok_or_else(|| format!("No source with channel '{}' found", channel))?;
        self.make_writer_for(&source_name)
    }

    /// Returns the typed writer for a declared source, pre-configured with all
    /// comparisons found in the program's clauses.
    pub fn make_writer_for(
        &mut self,
        source_name: &str,
    ) -> Result<TypedWriter, Box<dyn std::error::Error>> {
        let source = self
            .sources
            .iter()
            .find(|s| s.name == source_name)
            .ok_or_else(|| format!("Source '{}' not found", source_name))?;

        match source.message_type {
            ResinType::Probability => Ok(TypedWriter::Probability(
                self.manager.make_probability_writer(&source.channel)?,
            )),
            ResinType::Boolean => Ok(TypedWriter::Boolean(
                self.manager.make_boolean_writer(&source.channel)?,
            )),
            ResinType::Density => {
                let channels = self
                    .comparison_registry
                    .get(source_name)
                    .cloned()
                    .unwrap_or_default();
                Ok(TypedWriter::Density(
                    self.manager.make_density_writer_for_channels(&channels),
                ))
            }
            ResinType::Number => {
                let channels = self
                    .comparison_registry
                    .get(source_name)
                    .cloned()
                    .unwrap_or_default();
                Ok(TypedWriter::Number(
                    self.manager.make_number_writer_for_channels(&channels),
                ))
            }
            ResinType::Categorical => unreachable!(),
        }
    }

    /// Returns the categorical writer for the source whose channel matches `channel`.
    pub fn make_categorical_writer(
        &mut self,
        channel: &str,
    ) -> Result<TypedWriter, Box<dyn std::error::Error>> {
        let cat = self
            .categorical_sources
            .iter()
            .find(|c| c.channel == channel)
            .ok_or_else(|| format!("No categorical source with channel '{}' found", channel))?;
        let n = cat.categories.len();
        Ok(TypedWriter::Categorical(
            self.manager.make_categorical_writer(channel, n)?,
        ))
    }

    /// Returns the parameter groups discovered during compilation.
    ///
    /// Each entry maps a user-facing key `"{predicate}#{clause_index}"` to the
    /// names of the positive-polarity cause leaves sharing that `P(...)` value.
    pub fn get_parameter_groups(&self) -> &HashMap<String, Vec<String>> {
        &self.parameter_groups
    }

    /// Returns the subset of `gradients` that belongs to a single source,
    /// looked up by atom name **or** IPC channel name.
    ///
    /// Delegates to `source_leaf_names` for the lookup and filters the provided
    /// gradient map.  Returns an empty map when `name` matches no source.
    pub fn source_gradients<'a>(
        &self,
        gradients: &'a HashMap<String, f64>,
        name: &str,
    ) -> HashMap<&'a str, f64> {
        self.source_leaf_names(name)
            .into_iter()
            .filter_map(|leaf| {
                gradients
                    .get_key_value(leaf.as_str())
                    .map(|(k, &v)| (k.as_str(), v))
            })
            .collect()
    }

    /// Returns the positive-polarity leaf names for a given source, looked up
    /// by source atom name **or** IPC channel name.
    ///
    /// - `Probability` / `Boolean`: one leaf (the atom name itself).
    /// - `Density` / `Number`: one leaf per registered comparison threshold
    ///   (e.g. `"speed_lt_25"`, `"speed_gt_50"`).
    /// - `Categorical`: one leaf per category atom.
    ///
    /// Returns an empty `Vec` when `name` does not match any source.
    pub fn source_leaf_names(&self, name: &str) -> Vec<String> {
        // Try regular sources first.
        if let Some(source) = self
            .sources
            .iter()
            .find(|s| s.name == name || s.channel == name)
        {
            return match source.message_type {
                ResinType::Probability | ResinType::Boolean => vec![source.name.clone()],
                ResinType::Density | ResinType::Number => self
                    .comparison_registry
                    .get(&source.name)
                    .map(|entries| entries.iter().map(|(_, _, n)| n.clone()).collect())
                    .unwrap_or_default(),
                ResinType::Categorical => unreachable!(),
            };
        }
        // Try categorical sources (matched only by channel).
        if let Some(cat) = self.categorical_sources.iter().find(|c| c.channel == name) {
            return cat.categories.clone();
        }
        vec![]
    }

    /// Applies one gradient-descent step to probabilistic-clause parameters,
    /// keeping all groundings of each clause at the same shared value.
    ///
    /// For each parameter group (one per `P(...)` clause), the gradients of all
    /// positive leaves are summed to form the shared gradient, then a single
    /// `p_new` is computed and written to every leaf in the group — both the
    /// positive leaf and its complement (`1 − p_new`).
    ///
    /// Source atoms (Probability, Boolean, Density, Number, Categorical) are
    /// never touched; only cause leaves created from `P(...)` clauses are updated.
    /// Body-less clauses (`something <- P(0.3).`) are handled identically to
    /// clauses with conditions.
    ///
    /// `loss` is `∂L/∂P` — the upstream scalar (e.g. `2·(P − target)` for MSE).
    /// Pass `parameters` to restrict which groups are updated; `None` updates all.
    pub fn fit_parameters(
        &mut self,
        gradients: &HashMap<String, f64>,
        lr: f64,
        loss: f64,
        parameters: Option<&[&str]>,
        timestamp: f64,
    ) {
        let mut rc = self.manager.reactive_circuit.lock().unwrap();
        let value_size = rc.value_size;

        let index_map: HashMap<String, usize> = rc
            .leafs
            .iter()
            .enumerate()
            .map(|(i, l)| (l.name.clone(), i))
            .collect();

        let mut updates: Vec<(u32, f64)> = Vec::new();

        for (group_key, pos_names) in &self.parameter_groups {
            if let Some(params) = parameters {
                if !params.contains(&group_key.as_str()) {
                    continue;
                }
            }

            let sum_grad: f64 = pos_names
                .iter()
                .filter_map(|name| gradients.get(name).copied())
                .sum();

            let Some(&first_idx) = index_map.get(&pos_names[0]) else {
                continue;
            };
            let p = rc.leafs[first_idx].get_encoded_value()[0];
            let p_new = (p - lr * loss * sum_grad).clamp(0.0, 1.0);

            for name in pos_names {
                if let Some(&idx) = index_map.get(name) {
                    updates.push((idx as u32, p_new));
                }
                if let Some(&idx) = index_map.get(&format!("-{}", name)) {
                    updates.push((idx as u32, 1.0 - p_new));
                }
            }
        }

        for (idx, p) in updates {
            leaf::update(
                &mut rc,
                idx,
                Vector::from_elem(value_size, p).into_shared(),
                timestamp,
            );
        }
    }

    /// Converts a `Dnf` formula into a sum-product structure inside the
    /// reactive circuit, registering the result under `target_token`.
    ///
    /// Only literals that map to a known leaf index are included; derived atoms
    /// that appear in stable models but have no circuit leaf are silently skipped.
    pub fn circuit_from_dnf(&self, dnf: Dnf, target_token: &str) {
        // Get indexing from name to foliage
        let index_map = self.manager.get_index_map();

        // A DNF is an OR over AND, i.e., a sum over products without further hirarchy
        let mut sum_product = Vec::new();
        for clause in &dnf.clauses {
            let mut product = vec![];

            for literal in clause {
                // Derived atoms (e.g. intermediate rules like `permitted`)
                // appear in stable models but have no corresponding leaf.
                // Only choice atoms have leaves; skip everything else.
                if let Some(&idx) = index_map.get(literal) {
                    product.push(idx as u32);
                }
            }

            sum_product.push(product);
        }

        // Add the target to the ReactiveCircuit
        self.manager
            .reactive_circuit
            .lock()
            .unwrap()
            .add_sum_product(&sum_product, target_token);
    }
}

impl<S: Semiring> FromStr for Resin<S> {
    type Err = Box<dyn std::error::Error>;

    /// Parses a Resin program string into sources, targets, and clauses.
    ///
    /// Pre-processing strips comments (`#` to end-of-line) from every line and
    /// joins the result into a single string.  That string is then split on
    /// statement-terminating dots (`\.(?!\d)`) so multi-line statements and
    /// mid-clause comments are handled correctly.  Returns a `Resin` struct
    /// with an empty manager and comparison registry; call `compile` for the
    /// full pipeline including signal setup and circuit construction.
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut resin = Resin::<S> {
            clauses: vec![],
            sources: vec![],
            categorical_sources: vec![],
            targets: vec![],
            manager: Manager::<S>::new(1),
            value_size: 1,
            comparison_registry: HashMap::new(),
            parameter_groups: HashMap::new(),
        };

        // Strip comments and join into one string so multi-line statements are
        // handled as a unit.
        let stripped = input
            .lines()
            .map(|line| line.find('#').map_or(line, |p| &line[..p]).trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        // Split on statement-terminating dots (not followed by a digit).
        // split_statements keeps the dot in each piece so individual parsers
        // receive well-formed statements.
        for statement in split_statements(&stripped) {
            let statement = statement.trim();
            if statement.is_empty() {
                continue;
            }
            let statement = statement.to_string();

            // Try categorical before regular source — `{...}` won't match SOURCE_REGEX.
            if let Ok(cat) = statement.parse::<CategoricalSource>() {
                resin.categorical_sources.push(cat);
                continue;
            }
            if let Ok(source) = statement.parse::<Source>() {
                resin.sources.push(source);
                continue;
            }
            if let Ok(target) = statement.parse::<Target>() {
                resin.targets.push(target);
                continue;
            }
            if let Ok(clause) = statement.parse::<Clause>() {
                resin.clauses.push(clause);
            }
        }

        Ok(resin)
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::circuit::semiring::{Boolean, Fuzzy, LogProb, MaxProduct, ProbGradient};

    type TestResin = Resin<LogProb>;

    #[test]
    fn test_clauses() {
        let code = "test.";
        let clause: Clause = code.parse().expect("Parse clause failed!");
        assert!(clause.body.is_empty());
        assert_eq!(clause.code, code);
        assert_eq!(clause.head, "test");
        assert!(clause.probability.is_none());

        let code = "pilot(ben).";
        let clause: Clause = code.parse().expect("Parse clause failed!");
        assert!(clause.body.is_empty());
        assert_eq!(clause.code, code);
        assert_eq!(clause.head, "pilot(ben)");
        assert!(clause.probability.is_none());

        let code = "heavy(drone_1) <- P(0.8).";
        let clause: Clause = code.parse().expect("Parse clause failed!");
        assert!(clause.body.is_empty());
        assert_eq!(clause.code, code);
        assert_eq!(clause.head, "heavy(drone_1)");
        assert_eq!(clause.probability.unwrap(), 0.8);

        let code =
            "unsafe(drone_1, drone_2) <- P(0.65) if close(drone_1, drone_2) and heavy(drone_1).";
        let clause: Clause = code.parse().expect("Parse clause failed!");
        assert_eq!(clause.code, code);
        assert_eq!(clause.head, "unsafe(drone_1, drone_2)");
        assert_eq!(clause.probability.unwrap(), 0.65);
        assert_eq!(
            clause.body,
            vec!["close(drone_1, drone_2)", "heavy(drone_1)"]
        );
    }

    // -------------------------------------------------------------------
    // Density / Number / Boolean source tests
    // -------------------------------------------------------------------

    #[test]
    fn test_density_source_compilation() {
        let model = r#"
            distance(hospital) <- source("/distance/hospital", Density).
            safe if distance(hospital) < 20.0.
            safe if distance(hospital) > 55.0.
            safe -> target("/safety").
        "#;

        let mut resin = TestResin::compile(model, 1, true).expect("Compile failed");

        // Two comparison leaf pairs should have been created
        let names = resin.manager.get_names();
        assert!(names.iter().any(|n| n.contains("lt")), "lt leaf missing");
        assert!(names.iter().any(|n| n.contains("gt")), "gt leaf missing");

        // The comparison registry should have two entries for distance(hospital)
        let registry = resin.comparison_registry.get("distance(hospital)").unwrap();
        assert_eq!(registry.len(), 2);
        let has_lt = registry.iter().any(|(_, upper_tail, _)| !upper_tail);
        let has_gt = registry.iter().any(|(_, upper_tail, _)| *upper_tail);
        assert!(has_lt, "lower-tail entry missing");
        assert!(has_gt, "upper-tail entry missing");

        // make_writer_for should return a Density writer
        let writer = resin.make_writer_for("distance(hospital)").unwrap();
        assert!(matches!(
            writer,
            crate::channels::ipc::TypedWriter::Density(_)
        ));
    }

    #[test]
    fn test_density_writer_updates_leaves() {
        use std::thread::sleep;
        use std::time::Duration;

        let model = r#"
            dist <- source("/dist", Density).
            safe if dist < 20.0.
            safe if dist > 55.0.
            safe -> target("/safety").
        "#;

        let mut resin = TestResin::compile(model, 1, false).expect("Compile failed");
        let writer = resin.make_writer_for("dist").unwrap();

        let TypedWriter::Density(density_writer) = writer else {
            panic!("Expected Density writer");
        };

        // Write a Normal(25, 5) distribution
        let dist = crate::channels::ipc::VectorDistribution::Normal {
            mean: crate::circuit::Vector::from_elem(1, 25.0),
            std: crate::circuit::Vector::from_elem(1, 5.0),
        };
        density_writer.write(&dist, None);
        sleep(Duration::from_millis(30));

        let values = resin.manager.get_values();
        let names = resin.manager.get_names();

        // Find the lt leaf value
        let lt_idx = names.iter().position(|n| n.contains("lt")).unwrap();
        let gt_idx = names.iter().position(|n| n.contains("gt")).unwrap();

        // P(X < 20) for Normal(25, 5) ≈ 0.159
        assert!(
            (values[lt_idx][0] - 0.159).abs() < 0.001,
            "lt leaf = {}",
            values[lt_idx][0]
        );
        // P(X > 55) for Normal(25, 5) ≈ 0 (extremely small)
        assert!(values[gt_idx][0] < 1e-6, "gt leaf = {}", values[gt_idx][0]);
    }

    #[test]
    fn test_number_source_compilation() {
        let model = r#"
            speed <- source("/speed", Number).
            moving if speed > 5.0.
            moving -> target("/moving").
        "#;

        let mut resin = TestResin::compile(model, 1, false).expect("Compile failed");
        let writer = resin.make_writer_for("speed").unwrap();
        assert!(matches!(
            writer,
            crate::channels::ipc::TypedWriter::Number(_)
        ));

        let TypedWriter::Number(num_writer) = writer else {
            panic!("Expected Number writer");
        };

        // value > 5.0 → 1.0; value < 5.0 → 0.0
        use std::thread::sleep;
        use std::time::Duration;

        num_writer.write(Vector::from(vec![10.0]), None);
        sleep(Duration::from_millis(30));
        let values = resin.manager.get_values();
        let names = resin.manager.get_names();
        let gt_idx = names.iter().position(|n| n.contains("gt")).unwrap();
        assert_eq!(values[gt_idx][0], 1.0, "speed=10 should be > 5");

        num_writer.write(Vector::from(vec![2.0]), None);
        sleep(Duration::from_millis(30));
        let values = resin.manager.get_values();
        assert_eq!(values[gt_idx][0], 0.0, "speed=2 should not be > 5");
    }

    #[test]
    fn test_boolean_source_compilation() {
        let model = r#"
            active <- source("/active", Boolean).
            alarm if active.
            alarm -> target("/alarm").
        "#;

        let mut resin = TestResin::compile(model, 1, false).expect("Compile failed");
        let writer = resin.make_writer_for("active").unwrap();
        assert!(matches!(
            writer,
            crate::channels::ipc::TypedWriter::Boolean(_)
        ));

        let TypedWriter::Boolean(bool_writer) = writer else {
            panic!("Expected Boolean writer");
        };

        use std::thread::sleep;
        use std::time::Duration;

        bool_writer.write(true, None);
        sleep(Duration::from_millis(30));
        let values = resin.manager.get_values();
        let names = resin.manager.get_names();
        let active_idx = names.iter().position(|n| n == "active").unwrap();
        assert_eq!(values[active_idx][0], 1.0);

        bool_writer.write(false, None);
        sleep(Duration::from_millis(30));
        let values = resin.manager.get_values();
        assert_eq!(values[active_idx][0], 0.0);
    }

    #[test]
    fn test_resin_model() {
        let model = "
        close(a,b) <- P(0.8).
        close(a,c) <- P(0.7).

        unsafe if close(X,Y).

        unsafe -> target(\"/safety\").
        ";

        // Compile Resin runtime environment
        let resin = TestResin::compile(model, 1, true);
        assert!(resin.is_ok());
        let resin = resin.unwrap();

        // Show circuit
        let _ = resin
            .manager
            .reactive_circuit
            .lock()
            .unwrap()
            .to_combined_svg("output/test/test_resin_model_circuits.svg");

        println!(
            "targets = {:#?}",
            resin.manager.reactive_circuit.lock().unwrap().targets
        );

        // Count the correct number of Resin elements
        assert_eq!(resin.clauses.len(), 3);
        assert_eq!(resin.sources.len(), 0);
        assert_eq!(resin.targets.len(), 1);

        // Check a correct result for target signal
        let result = resin.manager.reactive_circuit.lock().unwrap().update();
        assert_eq!(
            result["/safety"],
            Vector::from(vec![0.8 * 0.7 + 0.2 * 0.7 + 0.8 * 0.3])
        );
    }

    // The three tests below use the same two-clause proximity model as
    // `test_resin_model` to show how swapping the semiring changes the question
    // the circuit answers while the formula structure stays identical.
    //
    // The DNF for `unsafe if close(X,Y)` expands to three exclusive minterms:
    //   M1 = {close(a,b)=T, close(a,c)=T}  weight 0.8·0.7 = 0.56
    //   M2 = {close(a,b)=F, close(a,c)=T}  weight 0.2·0.7 = 0.14
    //   M3 = {close(a,b)=T, close(a,c)=F}  weight 0.8·0.3 = 0.24
    //
    //   LogProb  : ΣMi = 0.56+0.14+0.24 = 0.94   (probability of unsafe)
    //   MaxProduct: max(Mi) = 0.56                 (most probable unsafe world)
    //   Fuzzy    : max(min·) = 0.7                 (degree of unsafe condition)
    //   Boolean  : OR(Mi>0) = 1                    (is unsafe satisfied (if p > 0 = T)?)

    const PROXIMITY_MODEL: &str = "
        close(a,b) <- P(0.8).
        close(a,c) <- P(0.7).
        unsafe if close(X,Y).
        unsafe -> target(\"/safety\").
    ";

    /// MaxProduct semiring — Most Probable Explanation.
    ///
    /// Compiling the same proximity model under MaxProduct answers:
    /// "What is the probability of the single most-likely world in which the
    /// system is unsafe?"  This is 0.8 * 0.7 = 0.56 (M1), the world where
    /// both links are simultaneously active.
    #[test]
    fn test_max_product_most_probable_explanation() {
        let resin =
            Resin::<MaxProduct>::compile(PROXIMITY_MODEL, 1, false).expect("compile failed");
        let result = resin.manager.reactive_circuit.lock().unwrap().full_update();
        let expected = 0.8_f64 * 0.7; // max(0.56, 0.14, 0.24) = 0.56
        assert!(
            (result["/safety"][0] - expected).abs() < 1e-9,
            "MPE: expected {expected}, got {}",
            result["/safety"][0]
        );
    }

    /// Fuzzy semiring — degree of unsafe condition.
    ///
    /// Compiling under Fuzzy (AND=min, OR=max) answers:
    /// "To what degree is the system unsafe, treating each probability as a
    /// membership grade?"  M1 contributes min(0.8, 0.7) = 0.7 — the degree
    /// to which both proximity conditions hold jointly — which dominates.
    #[test]
    fn test_fuzzy_degree_of_unsafety() {
        let resin = Resin::<Fuzzy>::compile(PROXIMITY_MODEL, 1, false).expect("compile failed");
        let result = resin.manager.reactive_circuit.lock().unwrap().full_update();
        // max(min(0.8,0.7), min(0.2,0.7), min(0.8,0.3)) = max(0.7, 0.2, 0.3) = 0.7
        let expected = 0.7_f64;
        assert!(
            (result["/safety"][0] - expected).abs() < 1e-9,
            "Fuzzy: expected {expected}, got {}",
            result["/safety"][0]
        );
    }

    /// Boolean semiring — satisfiability of the unsafe condition.
    ///
    /// Compiling under Boolean (AND=·, OR=max on {0,1}) answers:
    /// "Is there ANY world in which the system is unsafe?"  Since both atoms
    /// carry positive probability they are encoded as 1, so M1 = AND(1,1) = 1
    /// and the circuit returns 1 — unsafe is satisfiable.
    #[test]
    fn test_boolean_unsafe_satisfiability() {
        let resin = Resin::<Boolean>::compile(PROXIMITY_MODEL, 1, false).expect("compile failed");
        let result = resin.manager.reactive_circuit.lock().unwrap().full_update();
        assert_eq!(
            result["/safety"][0], 1.0,
            "Boolean: unsafe should be satisfiable when both atoms have p > 0"
        );
    }

    /// ProbGradient semiring — simultaneous WMC and gradient computation.
    ///
    /// The proximity model has 4 circuit leaves (two cause-atom/complement pairs).
    /// Setting value_size = 5 gives layout [P, ∂P/∂x₀, ∂P/∂x₁, ∂P/∂x₂, ∂P/∂x₃].
    ///
    /// WMC = p0·p2 + p1·p2 + p0·p3  (p0=0.8, p1=0.2, p2=0.7, p3=0.3)
    ///     = 0.56 + 0.14 + 0.24 = 0.94
    ///
    /// Gradients (leaves are independent parameters):
    ///   ∂WMC/∂p0 = p2 + p3 = 1.0
    ///   ∂WMC/∂p1 = p2      = 0.7
    ///   ∂WMC/∂p2 = p0 + p1 = 1.0
    ///   ∂WMC/∂p3 = p0      = 0.8
    #[test]
    fn test_prob_gradient_wmc_and_derivatives() {
        let resin =
            Resin::<ProbGradient>::compile(PROXIMITY_MODEL, 1, false).expect("compile failed");
        let result = resin.manager.reactive_circuit.lock().unwrap().full_update();
        let grad = &result["/safety"];

        let tol = 1e-9_f64;
        assert!(
            (grad[0] - 0.94).abs() < tol,
            "P(unsafe)  expected 0.94,  got {}",
            grad[0]
        );
        assert!(
            (grad[1] - 1.0).abs() < tol,
            "∂/∂p0      expected 1.0,   got {}",
            grad[1]
        );
        assert!(
            (grad[2] - 0.7).abs() < tol,
            "∂/∂p1      expected 0.7,   got {}",
            grad[2]
        );
        assert!(
            (grad[3] - 1.0).abs() < tol,
            "∂/∂p2      expected 1.0,   got {}",
            grad[3]
        );
        assert!(
            (grad[4] - 0.8).abs() < tol,
            "∂/∂p3      expected 0.8,   got {}",
            grad[4]
        );
    }

    /// Gradient descent on leaf probabilities using `ProbGradient`.
    ///
    /// The proximity model has 4 circuit leaves (two cause/complement pairs).
    /// Starting from the initial leaf probabilities (p0=0.8, p1=0.2, p2=0.7, p3=0.3)
    /// the circuit computes P(unsafe)=0.94 and the full Jacobian in one pass.
    ///
    /// We minimise the MSE loss  L = (P(unsafe) − 0.5)²  by treating each leaf
    /// probability as an independent parameter:
    ///
    ///   ∂L/∂pᵢ = 2·(P − 0.5) · (∂P/∂pᵢ)
    ///
    /// Each gradient step updates every leaf, clamping to [0, 1].  After enough
    /// steps the circuit should converge to P(unsafe) ≈ 0.5.
    #[test]
    fn test_prob_gradient_learns_parameter() {
        let resin =
            Resin::<ProbGradient>::compile(PROXIMITY_MODEL, 1, false).expect("compile failed");

        let mut rc = resin.manager.reactive_circuit.lock().unwrap();
        let target_p = 0.5_f64;
        let lr = 0.1_f64;

        for step in 0..500 {
            let result = rc.full_gradient_update();
            let (p, gradients) = &result["/safety"];

            if (p - target_p).abs() < 1e-3 {
                return; // converged
            }

            rc.fit(gradients, lr, 2.0 * (p - target_p), None, step as f64);
        }

        let result = rc.full_gradient_update();
        let (p_final, _) = &result["/safety"];
        assert!(
            (p_final - target_p).abs() < 1e-2,
            "gradient descent did not converge: P(unsafe) = {:.4}, target = {:.4}",
            p_final,
            target_p
        );
    }

    #[test]
    fn test_no_redundant_choices_for_unreferenced_sources() {
        // Each source type has one referenced and one unreferenced variant.
        // Density/Number: referenced means appearing in a comparison in a clause body.
        // Boolean/Probability: referenced means appearing as a literal in a clause body.
        let model = r#"
            unused_bool <- source("/unused_bool", Boolean).
            active <- source("/active", Boolean).
            unused_prob <- source("/unused_prob", Probability).
            likely <- source("/likely", Probability).
            unused_dist <- source("/unused_dist", Density).
            dist <- source("/dist", Density).
            unused_num <- source("/unused_num", Number).
            speed <- source("/speed", Number).
            alarm if active.
            alarm if likely.
            alarm if dist < 10.0.
            alarm if speed > 5.0.
            alarm -> target("/alarm").
        "#;

        let resin: Resin = model.parse().unwrap();
        let asp = resin.to_asp(0);

        assert!(
            asp.contains("{active}"),
            "referenced Boolean source must produce a choice"
        );
        assert!(
            !asp.contains("{unused_bool}"),
            "unreferenced Boolean source must not produce a choice"
        );

        assert!(
            asp.contains("{likely}"),
            "referenced Probability source must produce a choice"
        );
        assert!(
            !asp.contains("{unused_prob}"),
            "unreferenced Probability source must not produce a choice"
        );

        // Density/Number choices use canonical comparison names, not the source name directly.
        // Verify the referenced sources generate at least one choice and the unreferenced ones generate none.
        let dist_choice_count = asp.matches("dist").count();
        let unused_dist_choice_count = asp.matches("unused_dist").count();
        assert!(
            dist_choice_count > 0,
            "referenced Density source must produce comparison choices"
        );
        assert_eq!(
            unused_dist_choice_count, 0,
            "unreferenced Density source must not produce comparison choices"
        );

        let speed_choice_count = asp.matches("speed").count();
        let unused_num_choice_count = asp.matches("unused_num").count();
        assert!(
            speed_choice_count > 0,
            "referenced Number source must produce comparison choices"
        );
        assert_eq!(
            unused_num_choice_count, 0,
            "unreferenced Number source must not produce comparison choices"
        );
    }

    // -----------------------------------------------------------------------
    // Noisy-OR / probabilistic clause tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_probabilistic_clause_uses_aux_atom() {
        // A single probabilistic clause must emit an aux choice atom and a
        // deterministic rule, not the raw `{head} :- body.` form.
        let model = r#"
            close(a, b) <- P(0.8).
            alarm if close(a, b).
            alarm -> target("/alarm").
        "#;

        let mut resin: Resin = model.parse().unwrap();
        resin.setup_signals().unwrap();
        let asp = resin.to_asp(0);

        assert!(
            asp.contains("{close_a__b_cause_0}"),
            "aux choice atom missing"
        );
        assert!(
            asp.contains("close(a, b) :- close_a__b_cause_0"),
            "aux rule missing"
        );
        assert!(
            !asp.contains("{close(a, b)}"),
            "raw head choice must not be emitted"
        );
    }

    #[test]
    fn test_same_head_gets_distinct_aux_atoms() {
        // Two probabilistic clauses for the same head must get distinct cause indices.
        let model = r#"
            risky <- P(0.3).
            risky <- P(0.6).
            risky -> target("/risky").
        "#;

        let mut resin: Resin = model.parse().unwrap();
        resin.setup_signals().unwrap();
        let asp = resin.to_asp(0);

        assert!(asp.contains("{risky_cause_0}"), "first cause atom missing");
        assert!(asp.contains("{risky_cause_1}"), "second cause atom missing");
        assert!(
            !asp.contains("{risky_cause_2}"),
            "spurious third cause atom present"
        );
    }

    #[test]
    fn test_noisy_or_probability() {
        // Two independent probabilistic causes for the same head.
        // P(risky) = 1 - (1-0.3)(1-0.6) = 1 - 0.7*0.4 = 0.72
        let model = r#"
            risky <- P(0.3).
            risky <- P(0.6).
            risky -> target("/risky").
        "#;

        let resin = TestResin::compile(model, 1, false).expect("compile failed");
        let result = resin.manager.reactive_circuit.lock().unwrap().update();
        let expected = 1.0 - 0.7_f64 * 0.4_f64;
        assert!(
            (result["/risky"][0] - expected).abs() < 1e-9,
            "noisy-OR probability wrong: got {}, expected {}",
            result["/risky"][0],
            expected
        );

        // FOL example for Noisy-Or
        let model = r#"
            coin(c0).
            coin(c1).
            coin(c2).
            coin(c3).

            heads(C) <- P(0.6) if coin(C).

            any_heads if heads(C).
            any_heads -> target("/any_heads").
        "#;

        let resin = TestResin::compile(model, 1, true).expect("compile failed");
        let result = resin.manager.reactive_circuit.lock().unwrap().update();
        let expected = 0.9744;
        assert!(
            (result["/any_heads"][0] - expected).abs() < 1e-9,
            "noisy-OR probability wrong: got {}, expected {}",
            result["/any_heads"][0],
            expected
        );
    }

    // -----------------------------------------------------------------------
    // First-order variable comparison tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_variable_comparison_generates_correct_asp() {
        let model = r#"
            distance(hospital) <- source("/distance/hospital", Density).
            distance(airport)  <- source("/distance/airport", Density).
            critical_infrastructure(hospital).
            critical_infrastructure(airport).
            safety_distance(T) if critical_infrastructure(T) and distance(T) > 100.
            safe if safety_distance(hospital) and safety_distance(airport).
            safe -> target("/safety").
        "#;

        let mut resin: Resin = model.parse().unwrap();
        resin.setup_signals().unwrap();
        let asp = resin.to_asp(0);

        // One ground choice atom per source instance
        assert!(
            asp.contains("{distance_hospital_gt_100}"),
            "missing hospital choice"
        );
        assert!(
            asp.contains("{distance_airport_gt_100}"),
            "missing airport choice"
        );

        // Helper rules that let Clingo ground the parameterized predicate
        assert!(
            asp.contains("resin_distance_gt_100(hospital) :- distance_hospital_gt_100."),
            "missing hospital helper rule"
        );
        assert!(
            asp.contains("resin_distance_gt_100(airport) :- distance_airport_gt_100."),
            "missing airport helper rule"
        );

        // The clause body uses the parameterized, groundable form
        assert!(
            asp.contains("resin_distance_gt_100(T)"),
            "missing parameterized atom in rule body"
        );

        // The flat variable-templated atom must NOT appear
        assert!(
            !asp.contains("{distance_T_gt_100}"),
            "spurious variable-template choice emitted"
        );
    }

    #[test]
    fn test_variable_comparison_full_compile() {
        // Full pipeline: parse → ASP → Clingo → ReactiveCircuit.
        let model = r#"
            distance(hospital) <- source("/distance/hospital", Density).
            distance(airport)  <- source("/distance/airport", Density).
            critical_infrastructure(hospital).
            critical_infrastructure(airport).
            safety_distance(T) if critical_infrastructure(T) and distance(T) > 100.
            safe if safety_distance(hospital) and safety_distance(airport).
            safe -> target("/safety").
        "#;

        let resin = TestResin::compile(model, 1, false).expect("compile failed");

        // Both comparison leaves must have been created
        let names = resin.manager.get_names();
        assert!(
            names.iter().any(|n| n == "distance_hospital_gt_100"),
            "hospital leaf missing: {:?}",
            names
        );
        assert!(
            names.iter().any(|n| n == "distance_airport_gt_100"),
            "airport leaf missing: {:?}",
            names
        );
    }

    // -----------------------------------------------------------------------
    // Comment stripping tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_comment_stripping() {
        // Full-line, inline, and mid-clause comments must all be ignored.
        let model = r#"
            # Full-line comment: declare sources
            active <- source("/active", Boolean). # inline comment
            speed  <- source("/speed", Number).

            # Another full-line comment
            alarm if active        # mid-clause comment
                  and speed > 5.   # comment after continuation line
            alarm -> target("/alarm").
        "#;

        let resin: Resin = model.parse().unwrap();

        assert_eq!(resin.sources.len(), 2);
        assert_eq!(resin.targets.len(), 1);
        let alarm = resin.clauses.iter().find(|c| c.head == "alarm").unwrap();
        assert!(
            alarm.body.contains(&"active".to_string()),
            "active body literal missing"
        );
        assert!(
            !alarm.comparison_literals.is_empty(),
            "speed > 5 comparison missing"
        );
    }

    // -----------------------------------------------------------------------
    // Integration test combining  all Resin syntax features together
    // -----------------------------------------------------------------------

    #[test]
    fn test_integration() {
        use crate::channels::ipc::VectorDistribution;
        use std::thread::sleep;
        use std::time::Duration;

        let model = r#"
        # Source declarations
        over(park)         <- source("/map/over/park", Probability).
        distance(hospital) <- source("/map/distance/hospital", Density).
        distance(airport)  <- source("/map/distance/airport", Density).
        speed              <- source("/sensor/speed", Number).
        flight_hours(w1)   <- source("/metrics/flight_hours/wing_1", Number).
        flight_hours(w2)   <- source("/metrics/flight_hours/wing_2", Number).
        flight_hours(w3)   <- source("/metrics/flight_hours/wing_3", Number).
        flight_hours(w4)   <- source("/metrics/flight_hours/wing_4", Number).

        # Propositional rules
        permitted if over(park) and speed < 25.

        # First-order rules
        critical_infrastructure(hospital).
        critical_infrastructure(airport).
        safety_distance(T) if critical_infrastructure(T) and distance(T) > 100.

        # Conditional probabilities and Noisy-OR over first-order instantiations
        wing(w1). wing(w2). wing(w3). wing(w4).
        needs_checkup(W) <- P(0.9) if flight_hours(W) > 100 and wing(W).
        any_wing_needs_checkup if needs_checkup(W).

        # Target that the program will be constrained on
        safe if permitted and safety_distance(T) and not any_wing_needs_checkup.
        safe -> target("/output/safe").
        "#;

        let mut resin = TestResin::compile(model, 1, true).expect("compile failed");

        // All expected leaves must exist after compilation.
        let names = resin.manager.get_names();
        for expected in &[
            "needs_checkup_cause_0(w1)",
            "needs_checkup_cause_0(w2)",
            "needs_checkup_cause_0(w3)",
            "needs_checkup_cause_0(w4)",
            "distance_hospital_gt_100",
            "distance_airport_gt_100",
            "speed_lt_25",
            "flight_hours_w1_gt_100",
            "flight_hours_w2_gt_100",
            "flight_hours_w3_gt_100",
            "flight_hours_w4_gt_100",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "leaf '{}' missing; all leaves: {:?}",
                expected,
                names
            );
        }

        // over(park) = 1.0 (certain)
        let TypedWriter::Probability(prob_writer) = resin.make_writer_for("over(park)").unwrap()
        else {
            panic!("Expected Probability writer");
        };
        prob_writer.write(Vector::from(vec![1.0]), None);

        // distance(hospital) and distance(airport) ~ Normal(500, 1):
        // P(X > 100) ≈ 1 and P(X > 200) ≈ 1 for both.
        let far_dist = VectorDistribution::Normal {
            mean: Vector::from_elem(1, 500.0),
            std: Vector::from_elem(1, 1.0),
        };
        let TypedWriter::Density(dist_hosp) = resin.make_writer_for("distance(hospital)").unwrap()
        else {
            panic!("Expected Density writer");
        };
        dist_hosp.write(&far_dist, None);

        let far_dist2 = VectorDistribution::Normal {
            mean: Vector::from_elem(1, 500.0),
            std: Vector::from_elem(1, 1.0),
        };
        let TypedWriter::Density(dist_air) = resin.make_writer_for("distance(airport)").unwrap()
        else {
            panic!("Expected Density writer");
        };
        dist_air.write(&far_dist2, None);

        // speed = 10 → speed_lt_25 = 1.0
        let TypedWriter::Number(speed_writer) = resin.make_writer_for("speed").unwrap() else {
            panic!("Expected Number writer");
        };
        speed_writer.write(Vector::from(vec![10.0]), None);

        // flight_hours = 200 → flight_hours_gt_100 = 1.0 (maintenance due)
        let TypedWriter::Number(flight_writer) = resin.make_writer_for("flight_hours(w1)").unwrap()
        else {
            panic!("Expected Number writer");
        };
        flight_writer.write(Vector::from(vec![200.0]), None);

        sleep(Duration::from_millis(50));

        // With flight_hours_w1_gt_100 = 1.0 (deterministic), needs_checkup fires
        // whenever needs_checkup_cause_0 (P=0.9) is true.
        // so needs_checkup_cause_1 never fires regardless of its state.
        //
        // P(safe) = P(not needs_checkup) * P(all other conditions)
        //         = P(cause_0 = false) * 1.0 * 1.0 * ...
        //         = 0.1
        let result = resin.manager.reactive_circuit.lock().unwrap().update();
        let p_safe = result["/output/safe"][0];
        assert!(
            (p_safe - 0.1).abs() < 0.01,
            "P(safe) = {:.4}, expected ≈ 0.1",
            p_safe
        );
    }

    // -----------------------------------------------------------------------
    // MNIST addition
    // -----------------------------------------------------------------------

    /// MNIST addition: P(digit1 + digit2 = S).
    ///
    /// Two categorical digit sources with 3 classes each (digits 0-2 for brevity).
    /// `circuit_from_dnf` skips the negative literals
    /// (no corresponding leaves), so each product collapses to one leaf per digit:
    ///
    ///   P(sum = 2) = P(d1=0)·P(d2=2) + P(d1=1)·P(d2=1) + P(d1=2)·P(d2=0)
    ///
    /// With P(d1) = [0.1, 0.6, 0.3] and P(d2) = [0.4, 0.3, 0.2] (unnormalised
    /// is fine — Resin treats each leaf independently):
    ///
    ///   P(sum=2) = 0.1·0.2 + 0.6·0.3 + 0.3·0.4 = 0.02 + 0.18 + 0.12 = 0.32
    #[test]
    fn test_mnist_addition() {
        const MNIST_MODEL: &str = r#"
            {digit1(0), digit1(1), digit1(2)} <- source("/digit1", Categorical).
            {digit2(0), digit2(1), digit2(2)} <- source("/digit2", Categorical).

            sum_eq_2 if digit1(0) and digit2(2).
            sum_eq_2 if digit1(1) and digit2(1).
            sum_eq_2 if digit1(2) and digit2(0).

            sum_eq_2 -> target("/sum_eq_2").
        "#;

        use crate::channels::ipc::TypedWriter;
        use std::thread::sleep;
        use std::time::Duration;

        let mut resin = TestResin::compile(MNIST_MODEL, 1, false).expect("compile failed");

        // Verify the categorical leaves were created (positive only, no negatives).
        let names = resin.manager.get_names();
        for expected in &[
            "digit1(0)",
            "digit1(1)",
            "digit1(2)",
            "digit2(0)",
            "digit2(1)",
            "digit2(2)",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "leaf '{}' missing; all: {:?}",
                expected,
                names
            );
        }
        for unexpected in &["-digit1(0)", "-digit1(1)", "-digit2(0)"] {
            assert!(
                !names.iter().any(|n| n == unexpected),
                "unexpected negative leaf '{}'",
                unexpected
            );
        }

        // Write P(digit1) = [0.1, 0.6, 0.3] and P(digit2) = [0.4, 0.3, 0.2].
        let TypedWriter::Categorical(w1) = resin.make_categorical_writer("/digit1").unwrap() else {
            panic!("expected CategoricalWriter for /digit1")
        };
        let TypedWriter::Categorical(w2) = resin.make_categorical_writer("/digit2").unwrap() else {
            panic!("expected CategoricalWriter for /digit2")
        };

        w1.write(Vector::from(vec![0.1, 0.6, 0.3]), None);
        w2.write(Vector::from(vec![0.4, 0.3, 0.2]), None);

        sleep(Duration::from_millis(50));

        let result = resin.manager.reactive_circuit.lock().unwrap().full_update();
        let p = result["/sum_eq_2"][0];

        // Expected: 0.1·0.2 + 0.6·0.3 + 0.3·0.4 = 0.32
        let expected = 0.1_f64 * 0.2 + 0.6 * 0.3 + 0.3 * 0.4;
        assert!(
            (p - expected).abs() < 1e-9,
            "P(sum=2) expected {:.4}, got {:.4}",
            expected,
            p
        );
    }
}
