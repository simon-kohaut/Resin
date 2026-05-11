use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::process::Command;

use ndarray::Array1;
use petgraph::stable_graph::EdgeIndex;

use super::reactive::ReactiveCircuit;
use super::Vector;

/// A variable in the circuit; either a reactive leaf or a cached memory from a
/// child ReactiveCircuit node.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Column {
    Leaf(u32),
    Memory(u32), // EdgeIndex serialised as u32
}

/// An arithmetic circuit representing a sum-of-products formula.
///
/// Stored as a matrix: each row is a minterm (conjunction of variables),
/// each column is a variable (leaf or memory).  Evaluation is done in
/// log-space for numerical stability:
///
///   log P(f) = logsumexp_j ( Σ_{i ∈ minterm_j} log p_i )
///
/// `leafs` and `memories` are index maps: variable id → column index.
#[derive(Clone, Debug)]
pub struct AlgebraicCircuit {
    /// Each minterm: sorted Vec of column indices into `columns`.
    pub(crate) minterms: Vec<Vec<usize>>,
    /// Ordered list of variables; a column index is a position here.
    pub(crate) columns: Vec<Column>,
    /// leaf index → column index.
    pub(crate) leafs: HashMap<u32, usize>,
    /// edge index (as u32) → column index.
    pub(crate) memories: HashMap<u32, usize>,
    pub(crate) value_size: usize,
}

impl AlgebraicCircuit {
    pub fn new(value_size: usize) -> Self {
        AlgebraicCircuit {
            minterms: Vec::new(),
            columns: Vec::new(),
            leafs: HashMap::new(),
            memories: HashMap::new(),
            value_size,
        }
    }

    /// Build from a slice of products expressed as leaf-index vecs.
    pub fn from_sum_product(value_size: usize, sum_product: &[Vec<u32>]) -> Self {
        let mut ac = AlgebraicCircuit::new(value_size);
        ac.add_sum_product(sum_product);
        ac
    }

    // ── variable management ───────────────────────────────────────────────────

    /// Return the column index for leaf `index`, creating a new column if absent.
    pub fn ensure_leaf(&mut self, index: u32) -> usize {
        if let Some(&col) = self.leafs.get(&index) {
            return col;
        }
        let col = self.columns.len();
        self.columns.push(Column::Leaf(index));
        self.leafs.insert(index, col);
        col
    }

    /// Return `Some(col)` if leaf `index` is present.
    pub fn get_leaf(&self, index: u32) -> Option<usize> {
        self.leafs.get(&index).copied()
    }

    pub fn is_in_circuit(&self, index: u32) -> bool {
        self.leafs.contains_key(&index)
    }

    /// Create (or return existing) memory column for `edge`.
    pub fn create_memory(&mut self, edge: EdgeIndex) -> usize {
        let key = edge.index() as u32;
        if let Some(&col) = self.memories.get(&key) {
            return col;
        }
        let col = self.columns.len();
        self.columns.push(Column::Memory(key));
        self.memories.insert(key, col);
        col
    }

    /// Return `Some(col)` if a memory for `edge` exists.
    pub fn get_memory(&self, edge: EdgeIndex) -> Option<usize> {
        self.memories.get(&(edge.index() as u32)).copied()
    }

    /// Retarget memory column `col` from the old edge to `new_edge`.
    pub fn remap_memory(&mut self, col: usize, old_key: u32, new_edge: EdgeIndex) {
        let new_key = new_edge.index() as u32;
        self.columns[col] = Column::Memory(new_key);
        self.memories.remove(&old_key);
        self.memories.insert(new_key, col);
    }

    pub fn col_is_memory(&self, col: usize) -> bool {
        matches!(self.columns[col], Column::Memory(_))
    }

    pub fn col_memory_edge(&self, col: usize) -> Option<EdgeIndex> {
        match self.columns[col] {
            Column::Memory(key) => Some(EdgeIndex::new(key as usize)),
            _ => None,
        }
    }

    /// Remove column `col` from the variable list, every minterm, and both
    /// index maps.  Column indices above `col` are shifted down by one.
    /// Minterms that become empty are removed.
    pub fn remove_col(&mut self, col: usize) {
        self.remove_col_inner(col);
        self.minterms.retain(|row| !row.is_empty());
    }

    /// Same as `remove_col` but keeps empty minterms so that stored row
    /// indices remain valid (used by `disconnect` in ReactiveCircuit).
    pub(crate) fn remove_col_keep_rows(&mut self, col: usize) {
        self.remove_col_inner(col);
    }

    fn remove_col_inner(&mut self, col: usize) {
        match &self.columns[col] {
            Column::Leaf(idx) => { self.leafs.remove(idx); }
            Column::Memory(key) => { self.memories.remove(key); }
        }
        self.columns.remove(col);
        for row in &mut self.minterms {
            row.retain(|&c| c != col);
            for c in row.iter_mut() {
                if *c > col { *c -= 1; }
            }
        }
        for v in self.leafs.values_mut() {
            if *v > col { *v -= 1; }
        }
        for v in self.memories.values_mut() {
            if *v > col { *v -= 1; }
        }
    }

    // ── minterm management ────────────────────────────────────────────────────

    /// Add a minterm given a sorted slice of column indices.  Returns its row index.
    pub fn push_minterm(&mut self, mut cols: Vec<usize>) -> usize {
        cols.sort_unstable();
        cols.dedup();
        let idx = self.minterms.len();
        self.minterms.push(cols);
        idx
    }

    /// Add a single-column minterm (used when wiring a lone memory into the root).
    pub fn push_single(&mut self, col: usize) -> usize {
        self.push_minterm(vec![col])
    }

    /// Add column `col` to minterm `row`.
    pub fn add_col_to_minterm(&mut self, row: usize, col: usize) {
        let pos = self.minterms[row].partition_point(|&c| c < col);
        if self.minterms[row].get(pos) != Some(&col) {
            self.minterms[row].insert(pos, col);
        }
    }

    /// Return the column indices of minterm `row`.
    pub fn get_minterm_cols(&self, row: usize) -> &[usize] {
        &self.minterms[row]
    }

    /// Return all row indices whose minterm contains `col`.
    pub fn minterms_containing_col(&self, col: usize) -> Vec<usize> {
        self.minterms
            .iter()
            .enumerate()
            .filter(|(_, row)| row.contains(&col))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.minterms.is_empty()
    }

    // ── bulk mutation ─────────────────────────────────────────────────────────

    /// Add one minterm given leaf indices.
    pub fn add(&mut self, indices: &[u32]) {
        let cols: Vec<usize> = indices.iter().map(|&i| self.ensure_leaf(i)).collect();
        self.push_minterm(cols);
    }

    /// Add many minterms efficiently.
    pub fn add_sum_product(&mut self, sum_product: &[Vec<u32>]) {
        for product in sum_product {
            self.add(product);
        }
    }

    /// Multiply every minterm by leaf `index` (add column to every row).
    pub fn multiply(&mut self, index: u32) {
        let col = self.ensure_leaf(index);
        for row in &mut self.minterms {
            let pos = row.partition_point(|&c| c < col);
            if row.get(pos) != Some(&col) {
                row.insert(pos, col);
            }
        }
    }

    // ── split ─────────────────────────────────────────────────────────────────

    /// Partition the circuit on leaf `index`.
    ///
    /// Returns `(in_scope, out_of_scope)` where `in_scope` contains exactly
    /// the minterms that include the leaf (with it still present), and
    /// `out_of_scope` contains those that don't.
    pub fn split(&self, index: u32) -> (Option<AlgebraicCircuit>, Option<AlgebraicCircuit>) {
        let col = match self.leafs.get(&index) {
            Some(&c) => c,
            None => return (None, None),
        };

        let (in_rows, out_rows): (Vec<_>, Vec<_>) =
            self.minterms.iter().partition(|row| row.contains(&col));

        (
            self.sub_circuit(in_rows),
            self.sub_circuit(out_rows),
        )
    }

    /// Build a sub-circuit from a subset of rows, remapping column indices.
    fn sub_circuit(&self, rows: Vec<&Vec<usize>>) -> Option<AlgebraicCircuit> {
        if rows.is_empty() {
            return None;
        }
        let used: std::collections::BTreeSet<usize> =
            rows.iter().flat_map(|r| r.iter().copied()).collect();

        let mut ac = AlgebraicCircuit::new(self.value_size);
        // old col idx → new col idx
        let mut remap = vec![0usize; self.columns.len()];
        for &old in &used {
            let new_col = ac.columns.len();
            remap[old] = new_col;
            let col = self.columns[old].clone();
            match &col {
                Column::Leaf(idx) => { ac.leafs.insert(*idx, new_col); }
                Column::Memory(key) => { ac.memories.insert(*key, new_col); }
            }
            ac.columns.push(col);
        }
        for row in rows {
            ac.minterms.push(row.iter().map(|&c| remap[c]).collect());
        }
        Some(ac)
    }

    // ── canonicalisation ──────────────────────────────────────────────────────

    /// Produce a canonical form: sort columns by label, reindex rows, sort
    /// rows lexicographically.
    pub fn canonicalize(&mut self) {
        if self.columns.is_empty() {
            return;
        }

        // Sort column positions by Column label.
        let mut order: Vec<usize> = (0..self.columns.len()).collect();
        order.sort_unstable_by(|&a, &b| self.columns[a].cmp(&self.columns[b]));

        // Build inverse permutation: old col idx → new col idx.
        let mut inv = vec![0usize; self.columns.len()];
        for (new, &old) in order.iter().enumerate() {
            inv[old] = new;
        }

        // Apply to columns.
        let new_cols: Vec<Column> = order.iter().map(|&o| self.columns[o].clone()).collect();
        self.columns = new_cols;
        for v in self.leafs.values_mut() { *v = inv[*v]; }
        for v in self.memories.values_mut() { *v = inv[*v]; }

        // Apply to rows and sort.
        for row in &mut self.minterms {
            for c in row.iter_mut() { *c = inv[*c]; }
            row.sort_unstable();
        }
        self.minterms.sort_unstable();
        debug_assert!(
            self.minterms.windows(2).all(|w| w[0] != w[1]),
            "duplicate minterms after canonicalize — determinism invariant violated"
        );
    }

    // ── evaluation ────────────────────────────────────────────────────────────

    /// Returns `log P(formula)` — edges are expected to already hold log-values.
    pub(crate) fn log_value(&self, rc: &ReactiveCircuit) -> Vector {
        if self.minterms.is_empty() {
            return Array1::from_elem(self.value_size, f64::NEG_INFINITY).into_shared();
        }

        // Borrow log-values directly — no copies for either leaves or edges.
        let log_cols: Vec<ndarray::ArrayView1<f64>> = self.columns.iter().map(|col| match col {
            Column::Leaf(idx) => rc.leafs[*idx as usize].get_log_value(),
            Column::Memory(key) => rc.structure[EdgeIndex::new(*key as usize)].view(),
        }).collect();

        let n = self.value_size;

        // Online logsumexp: single pass over minterms with four fixed O(N) buffers.
        // Avoids the M×N intermediate allocation of collecting all log_vals first.
        //
        // Invariant after processing k minterms:
        //   running_max[i] = max of log_val[i] seen so far
        //   running_sum[i] = Σ exp(log_val[i] - running_max[i])
        // Result: running_max + ln(running_sum)
        let mut running_max = Array1::from_elem(n, f64::NEG_INFINITY);
        let mut running_sum = Array1::<f64>::zeros(n);
        let mut log_val    = Array1::<f64>::zeros(n);
        let mut delta      = Array1::<f64>::zeros(n);

        for row in &self.minterms {
            // Accumulate this minterm's log-probability in-place.
            log_val.fill(0.0);
            for &c in row {
                log_val += &log_cols[c];
            }

            // Update running_max and compute the rescaling factor in one zip pass.
            // Guard: old_max = -inf means no valid terms yet; skip to avoid -inf - (-inf) = NaN.
            ndarray::Zip::from(&mut delta)
                .and(&mut running_max)
                .and(&log_val)
                .for_each(|d, m, &v| {
                    let new_m = m.max(v);
                    *d = if *m == f64::NEG_INFINITY { 0.0 } else { (*m - new_m).exp() };
                    *m = new_m;
                });

            // Rescale old sum, then add the new term.
            // Guard: new_max = -inf means this term is also zero-probability; skip to avoid NaN.
            running_sum *= &delta;
            ndarray::Zip::from(&mut running_sum)
                .and(&log_val)
                .and(&running_max)
                .for_each(|s, &lv, &m| {
                    if m > f64::NEG_INFINITY {
                        *s += (lv - m).exp();
                    }
                });
        }

        running_sum.mapv_inplace(f64::ln);
        running_max += &running_sum;
        running_max.into_shared()
    }

    /// Returns `P(formula)` in probability space (for tests and external callers).
    pub fn value(&self, rc: &ReactiveCircuit) -> Vector {
        self.log_value(rc).mapv(f64::exp).into_shared()
    }

    // ── visualisation ─────────────────────────────────────────────────────────

    pub fn to_dot_text(&self) -> String {
        let mut dot = String::from("digraph AlgebraicCircuit {\n");
        dot.push_str("  node [shape=record];\n");
        dot.push_str(&format!(
            "  matrix [label=\"{{{}}}\" shape=record];\n",
            self.minterms.iter().map(|row| {
                row.iter().map(|&c| match &self.columns[c] {
                    Column::Leaf(i) => format!("L{}", i),
                    Column::Memory(k) => format!("M{}", k),
                }).collect::<Vec<_>>().join("·")
            }).collect::<Vec<_>>().join(" | ")
        ));
        dot.push_str("}\n");
        dot
    }

    pub fn to_dot(&self, path: &str) -> std::io::Result<()> {
        let mut file = File::create(path)?;
        file.write_all(self.to_dot_text().as_bytes())
    }

    pub fn to_svg(&self, path: &str, keep_dot: bool) -> std::io::Result<()> {
        let dot_path = if keep_dot { path.to_owned() + ".dot" } else { path.to_owned() };
        self.to_dot(&dot_path)?;
        let svg = Command::new("dot").args(["-Tsvg", &dot_path]).output()?;
        let mut file = File::create(path)?;
        file.write_all(&svg.stdout)?;
        file.sync_all()
    }
}


// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ndarray::array;

    use crate::circuit::leaf::Leaf;
    use crate::circuit::reactive::ReactiveCircuit;

    use super::AlgebraicCircuit;

    #[test]
    fn test_ac_structure() {
        // Formula: a*b + a*c  (leaves 0,1,2)
        let mut ac = AlgebraicCircuit::new(1);
        ac.add(&[0, 1]);
        ac.add(&[0, 2]);

        assert_eq!(ac.columns.len(), 3);
        assert_eq!(ac.minterms.len(), 2);
        assert!(ac.get_leaf(0).is_some());
        assert!(ac.get_leaf(1).is_some());
        assert!(ac.get_leaf(2).is_some());
    }

    #[test]
    fn test_value_computation() {
        // Formula: 0*1 + 0*2 = 0.5*0.2 + 0.5*0.8 = 0.5
        let sum_product = vec![vec![0, 1], vec![0, 2]];

        let mut rc = ReactiveCircuit::new(1);
        rc.leafs.push(Leaf::new(array![0.5].into(), 0.0, "l0"));
        rc.leafs.push(Leaf::new(array![0.2].into(), 0.0, "l1"));
        rc.leafs.push(Leaf::new(array![0.8].into(), 0.0, "l2"));

        let ac = AlgebraicCircuit::from_sum_product(1, &sum_product);
        let result = ac.value(&rc);

        assert!((result[0] - 0.5_f64).abs() < 1e-9);
    }

    #[test]
    fn test_split() {
        // Formula: 0*1 + 0*2
        let mut ac = AlgebraicCircuit::new(1);
        ac.add(&[0, 1]);
        ac.add(&[0, 2]);

        // Split on leaf 1: in_scope = {0*1}, out_scope = {0*2}
        let (ins, outs) = ac.split(1);
        let ins = ins.unwrap();
        let outs = outs.unwrap();
        assert_eq!(ins.minterms.len(), 1);
        assert!(ins.get_leaf(1).is_some());
        assert_eq!(outs.minterms.len(), 1);
        assert!(outs.get_leaf(1).is_none());

        // Split on leaf 0: in_scope = both rows, no out_scope
        let (ins0, outs0) = ac.split(0);
        assert_eq!(ins0.unwrap().minterms.len(), 2);
        assert!(outs0.is_none());
    }

    #[test]
    fn test_canonicalize() {
        let mut ac = AlgebraicCircuit::new(1);
        ac.add(&[2, 0]); // will be normalised to [0,2]
        ac.add(&[1, 0]); // will be normalised to [0,1]

        ac.canonicalize();

        // Rows should be sorted: [0,1] before [0,2]
        let col0 = ac.get_leaf(0).unwrap();
        let col1 = ac.get_leaf(1).unwrap();
        let col2 = ac.get_leaf(2).unwrap();
        assert!(ac.minterms[0].contains(&col1));
        assert!(ac.minterms[1].contains(&col2));
        let _ = col0;
    }

    #[test]
    fn test_multiply() {
        let mut ac = AlgebraicCircuit::new(1);
        ac.add(&[1]);
        ac.add(&[2]);
        ac.multiply(0);

        // Every minterm should now contain leaf 0.
        let col0 = ac.get_leaf(0).unwrap();
        for row in &ac.minterms {
            assert!(row.contains(&col0));
        }
    }

    #[test]
    fn test_value_single_leaf() {
        // Formula: {0} — one minterm with one leaf, should equal P(leaf 0).
        let mut rc = ReactiveCircuit::new(1);
        rc.leafs.push(Leaf::new(array![0.3].into(), 0.0, "l0"));

        let ac = AlgebraicCircuit::from_sum_product(1, &[vec![0]]);
        let result = ac.value(&rc);

        assert!((result[0] - 0.3_f64).abs() < 1e-9);
    }
}
