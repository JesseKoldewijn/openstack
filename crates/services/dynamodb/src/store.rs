use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// AttributeValue — mirrors the DynamoDB wire format
// ---------------------------------------------------------------------------

pub type Item = HashMap<String, Value>;

// ---------------------------------------------------------------------------
// Key schema
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum KeyType {
    HASH,
    RANGE,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeySchemaElement {
    pub attribute_name: String,
    pub key_type: KeyType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeDefinition {
    pub attribute_name: String,
    pub attribute_type: String, // "S" | "N" | "B"
}

// ---------------------------------------------------------------------------
// Secondary indexes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Projection {
    pub projection_type: String, // ALL | KEYS_ONLY | INCLUDE
    pub non_key_attributes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSecondaryIndex {
    pub index_name: String,
    pub key_schema: Vec<KeySchemaElement>,
    pub projection: Projection,
    pub item_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalSecondaryIndex {
    pub index_name: String,
    pub key_schema: Vec<KeySchemaElement>,
    pub projection: Projection,
    pub item_count: u64,
}

// ---------------------------------------------------------------------------
// Stream specification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StreamSpecification {
    pub stream_enabled: bool,
    pub stream_view_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Stream record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamRecord {
    pub sequence_number: String,
    pub event_name: String,
    pub keys: HashMap<String, Value>,
    pub new_image: Option<Item>,
    pub old_image: Option<Item>,
    pub approximate_creation_date_time: f64,
}

// ---------------------------------------------------------------------------
// Sort key
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum SortKeyValue {
    S(String),
    N(f64),
}

impl PartialOrd for SortKeyValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (SortKeyValue::S(a), SortKeyValue::S(b)) => a.partial_cmp(b),
            (SortKeyValue::N(a), SortKeyValue::N(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TableStatus {
    ACTIVE,
    CREATING,
    DELETING,
    UPDATING,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    pub table_name: String,
    pub table_arn: String,
    pub table_id: String,
    pub status: TableStatus,
    pub created: DateTime<Utc>,
    pub key_schema: Vec<KeySchemaElement>,
    pub attribute_definitions: Vec<AttributeDefinition>,
    pub global_secondary_indexes: Vec<GlobalSecondaryIndex>,
    pub local_secondary_indexes: Vec<LocalSecondaryIndex>,
    pub stream_specification: StreamSpecification,
    pub stream_arn: Option<String>,
    pub billing_mode: String,
    pub item_count: u64,
    pub table_size_bytes: u64,
    /// partition_key → sort_key → Item  (sort_key = "" when no range key)
    pub items: HashMap<String, HashMap<String, Item>>,
    pub stream_records: Vec<StreamRecord>,
    pub stream_sequence: u64,
}

impl Table {
    pub fn new(
        name: impl Into<String>,
        account_id: &str,
        region: &str,
        key_schema: Vec<KeySchemaElement>,
        attribute_definitions: Vec<AttributeDefinition>,
        gsis: Vec<GlobalSecondaryIndex>,
        lsis: Vec<LocalSecondaryIndex>,
        stream_spec: StreamSpecification,
    ) -> Self {
        let name = name.into();
        let table_id = uuid::Uuid::new_v4().to_string();
        let table_arn = format!("arn:aws:dynamodb:{region}:{account_id}:table/{name}");
        let stream_arn = if stream_spec.stream_enabled {
            Some(format!(
                "{table_arn}/stream/{}",
                chrono::Utc::now().timestamp()
            ))
        } else {
            None
        };
        Self {
            table_name: name,
            table_arn,
            table_id,
            status: TableStatus::ACTIVE,
            created: Utc::now(),
            key_schema,
            attribute_definitions,
            global_secondary_indexes: gsis,
            local_secondary_indexes: lsis,
            stream_specification: stream_spec,
            stream_arn,
            billing_mode: "PAY_PER_REQUEST".to_string(),
            item_count: 0,
            table_size_bytes: 0,
            items: HashMap::new(),
            stream_records: Vec::new(),
            stream_sequence: 0,
        }
    }

    pub fn hash_key_name(&self) -> Option<&str> {
        self.key_schema
            .iter()
            .find(|k| k.key_type == KeyType::HASH)
            .map(|k| k.attribute_name.as_str())
    }

    pub fn range_key_name(&self) -> Option<&str> {
        self.key_schema
            .iter()
            .find(|k| k.key_type == KeyType::RANGE)
            .map(|k| k.attribute_name.as_str())
    }

    fn make_key_map(&self, item: &Item) -> HashMap<String, Value> {
        let mut keys = HashMap::new();
        if let Some(hk) = self.hash_key_name()
            && let Some(v) = item.get(hk)
        {
            keys.insert(hk.to_string(), v.clone());
        }
        if let Some(rk) = self.range_key_name()
            && let Some(v) = item.get(rk)
        {
            keys.insert(rk.to_string(), v.clone());
        }
        keys
    }

    fn append_stream_record(
        &mut self,
        event_name: &str,
        keys: HashMap<String, Value>,
        old_image: Option<Item>,
        new_image: Option<Item>,
    ) {
        if !self.stream_specification.stream_enabled {
            return;
        }
        self.stream_sequence += 1;
        let rec = StreamRecord {
            sequence_number: format!("{:020}", self.stream_sequence),
            event_name: event_name.to_string(),
            keys,
            new_image,
            old_image,
            approximate_creation_date_time: Utc::now().timestamp() as f64,
        };
        self.stream_records.push(rec);
        if self.stream_records.len() > 1000 {
            self.stream_records.remove(0);
        }
    }

    pub fn extract_key_from_item(&self, item: &Item) -> Option<(String, String)> {
        let hk = self.hash_key_name()?;
        let hv = item.get(hk)?;
        let hash_str = av_to_key_str(hv)?;
        let sort_str = if let Some(rk) = self.range_key_name() {
            let rv = item.get(rk)?;
            av_to_key_str(rv)?
        } else {
            String::new()
        };
        Some((hash_str, sort_str))
    }

    pub fn put_item(&mut self, item: Item) -> Option<Item> {
        let (hk, sk) = self.extract_key_from_item(&item)?;
        let keys = self.make_key_map(&item);
        let old = self.items.entry(hk).or_default().insert(sk, item.clone());
        if old.is_none() {
            self.item_count += 1;
        }
        let event = if old.is_some() { "MODIFY" } else { "INSERT" };
        self.append_stream_record(event, keys, old.clone(), Some(item));
        old
    }

    pub fn get_item(&self, key: &Item) -> Option<&Item> {
        let (hk, sk) = self.extract_key_from_item(key)?;
        self.items.get(&hk)?.get(&sk)
    }

    pub fn delete_item(&mut self, key: &Item) -> Option<Item> {
        let (hk, sk) = self.extract_key_from_item(key)?;
        let keys = self.make_key_map(key);
        let partition = self.items.get_mut(&hk)?;
        let old = partition.remove(&sk);
        if old.is_some() {
            self.item_count = self.item_count.saturating_sub(1);
        }
        if old.is_some() {
            self.append_stream_record("REMOVE", keys, old.clone(), None);
        }
        old
    }

    pub fn all_items(&self) -> Vec<&Item> {
        self.items.values().flat_map(|m| m.values()).collect()
    }

    /// Query by partition key, with optional sort key condition.
    pub fn query(
        &self,
        hash_key_val: &str,
        range_condition: Option<&RangeCondition>,
        index_name: Option<&str>,
        scan_index_forward: bool,
    ) -> Vec<&Item> {
        if let Some(idx) = index_name {
            let idx_hk = self.index_hash_key(idx).unwrap_or("");
            let idx_rk = self.index_range_key(idx);
            let mut items: Vec<&Item> = self
                .all_items()
                .into_iter()
                .filter(|item| {
                    let item_hash = item.get(idx_hk).and_then(av_to_key_str);
                    if item_hash.as_deref() != Some(hash_key_val) {
                        return false;
                    }
                    if let Some(rc) = range_condition
                        && let Some(rk) = idx_rk
                        && let Some(rv) = item.get(rk)
                    {
                        return rc.matches(rv);
                    }
                    true
                })
                .collect();
            if let Some(rk) = idx_rk {
                items.sort_by(|a, b| {
                    let ak = a.get(rk).and_then(av_sort_key);
                    let bk = b.get(rk).and_then(av_sort_key);
                    let ord = ak.partial_cmp(&bk).unwrap_or(std::cmp::Ordering::Equal);
                    if scan_index_forward {
                        ord
                    } else {
                        ord.reverse()
                    }
                });
            }
            return items;
        }

        match self.items.get(hash_key_val) {
            None => Vec::new(),
            Some(partition) => {
                let rk_name = self.range_key_name();
                let mut items: Vec<&Item> = partition.values().collect();
                if let Some(rc) = range_condition {
                    items.retain(|item| {
                        if let Some(rk) = rk_name
                            && let Some(rv) = item.get(rk)
                        {
                            return rc.matches(rv);
                        }
                        true
                    });
                }
                if let Some(rk) = rk_name {
                    items.sort_by(|a, b| {
                        let ak = a.get(rk).and_then(av_sort_key);
                        let bk = b.get(rk).and_then(av_sort_key);
                        let ord = ak.partial_cmp(&bk).unwrap_or(std::cmp::Ordering::Equal);
                        if scan_index_forward {
                            ord
                        } else {
                            ord.reverse()
                        }
                    });
                }
                items
            }
        }
    }

    fn index_hash_key(&self, index_name: &str) -> Option<&str> {
        for gsi in &self.global_secondary_indexes {
            if gsi.index_name == index_name {
                return gsi
                    .key_schema
                    .iter()
                    .find(|k| k.key_type == KeyType::HASH)
                    .map(|k| k.attribute_name.as_str());
            }
        }
        for lsi in &self.local_secondary_indexes {
            if lsi.index_name == index_name {
                return lsi
                    .key_schema
                    .iter()
                    .find(|k| k.key_type == KeyType::HASH)
                    .map(|k| k.attribute_name.as_str());
            }
        }
        None
    }

    fn index_range_key(&self, index_name: &str) -> Option<&str> {
        for gsi in &self.global_secondary_indexes {
            if gsi.index_name == index_name {
                return gsi
                    .key_schema
                    .iter()
                    .find(|k| k.key_type == KeyType::RANGE)
                    .map(|k| k.attribute_name.as_str());
            }
        }
        for lsi in &self.local_secondary_indexes {
            if lsi.index_name == index_name {
                return lsi
                    .key_schema
                    .iter()
                    .find(|k| k.key_type == KeyType::RANGE)
                    .map(|k| k.attribute_name.as_str());
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Range condition
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum RangeCondition {
    Eq(Value),
    Lt(Value),
    Lte(Value),
    Gt(Value),
    Gte(Value),
    Between(Value, Value),
    BeginsWith(String),
}

impl RangeCondition {
    pub fn matches(&self, av: &Value) -> bool {
        match self {
            RangeCondition::Eq(e) => av_compare(av, e) == Some(std::cmp::Ordering::Equal),
            RangeCondition::Lt(b) => {
                matches!(av_compare(av, b), Some(std::cmp::Ordering::Less))
            }
            RangeCondition::Lte(b) => matches!(
                av_compare(av, b),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            ),
            RangeCondition::Gt(b) => {
                matches!(av_compare(av, b), Some(std::cmp::Ordering::Greater))
            }
            RangeCondition::Gte(b) => matches!(
                av_compare(av, b),
                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            ),
            RangeCondition::Between(lo, hi) => {
                matches!(
                    av_compare(av, lo),
                    Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                ) && matches!(
                    av_compare(av, hi),
                    Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                )
            }
            RangeCondition::BeginsWith(prefix) => av
                .get("S")
                .and_then(|s| s.as_str())
                .map(|s| s.starts_with(prefix.as_str()))
                .unwrap_or(false),
        }
    }
}

// ---------------------------------------------------------------------------
// DynamoDbStore
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DynamoDbStore {
    pub tables: HashMap<String, Table>,
}

impl DynamoDbStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_table(
        &mut self,
        name: impl Into<String>,
        account_id: &str,
        region: &str,
        key_schema: Vec<KeySchemaElement>,
        attribute_definitions: Vec<AttributeDefinition>,
        gsis: Vec<GlobalSecondaryIndex>,
        lsis: Vec<LocalSecondaryIndex>,
        stream_spec: StreamSpecification,
    ) -> &Table {
        let name = name.into();
        if !self.tables.contains_key(&name) {
            let table = Table::new(
                &name,
                account_id,
                region,
                key_schema,
                attribute_definitions,
                gsis,
                lsis,
                stream_spec,
            );
            self.tables.insert(name.clone(), table);
        }
        self.tables.get(&name).unwrap()
    }

    pub fn delete_table(&mut self, name: &str) -> Option<Table> {
        self.tables.remove(name)
    }

    pub fn get_table(&self, name: &str) -> Option<&Table> {
        self.tables.get(name)
    }

    pub fn get_table_mut(&mut self, name: &str) -> Option<&mut Table> {
        self.tables.get_mut(name)
    }

    pub fn list_table_names(&self) -> Vec<&str> {
        self.tables.keys().map(|s| s.as_str()).collect()
    }
}

// ---------------------------------------------------------------------------
// Attribute value helpers
// ---------------------------------------------------------------------------

pub fn av_to_key_str(v: &Value) -> Option<String> {
    if let Some(s) = v.get("S").and_then(|x| x.as_str()) {
        return Some(s.to_string());
    }
    if let Some(n) = v.get("N").and_then(|x| x.as_str()) {
        return Some(n.to_string());
    }
    if let Some(b) = v.get("BOOL") {
        return Some(b.to_string());
    }
    None
}

pub fn av_sort_key(v: &Value) -> Option<SortKeyValue> {
    if let Some(s) = v.get("S").and_then(|x| x.as_str()) {
        return Some(SortKeyValue::S(s.to_string()));
    }
    if let Some(n) = v.get("N").and_then(|x| x.as_str())
        && let Ok(f) = n.parse::<f64>()
    {
        return Some(SortKeyValue::N(f));
    }
    None
}

pub fn av_compare(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    let ak = av_sort_key(a)?;
    let bk = av_sort_key(b)?;
    ak.partial_cmp(&bk)
}

// ---------------------------------------------------------------------------
// Filter expression evaluation
// ---------------------------------------------------------------------------

pub fn evaluate_filter(
    item: &Item,
    expression: &str,
    attr_names: &HashMap<String, String>,
    attr_values: &HashMap<String, Value>,
) -> bool {
    evaluate_expr(item, expression.trim(), attr_names, attr_values)
}

fn evaluate_expr(
    item: &Item,
    expr: &str,
    names: &HashMap<String, String>,
    values: &HashMap<String, Value>,
) -> bool {
    let expr = expr.trim();

    if let Some(pos) = find_top_level(expr, " AND ") {
        return evaluate_expr(item, &expr[..pos], names, values)
            && evaluate_expr(item, &expr[pos + 5..], names, values);
    }
    if let Some(pos) = find_top_level(expr, " OR ") {
        return evaluate_expr(item, &expr[..pos], names, values)
            || evaluate_expr(item, &expr[pos + 4..], names, values);
    }
    if let Some(stripped) = expr.strip_prefix("NOT ") {
        return !evaluate_expr(item, stripped, names, values);
    }
    if expr.starts_with("attribute_exists(") && expr.ends_with(')') {
        let path = resolve_name(expr[17..expr.len() - 1].trim(), names);
        return item.contains_key(&path);
    }
    if expr.starts_with("attribute_not_exists(") && expr.ends_with(')') {
        let path = resolve_name(expr[21..expr.len() - 1].trim(), names);
        return !item.contains_key(&path);
    }
    if expr.starts_with("begins_with(") && expr.ends_with(')') {
        let inner = &expr[12..expr.len() - 1];
        let parts: Vec<&str> = inner.splitn(2, ',').collect();
        if parts.len() == 2 {
            let attr_name = resolve_name(parts[0].trim(), names);
            let val = resolve_value(parts[1].trim(), values);
            if let Some(iv) = item.get(&attr_name) {
                let is = iv.get("S").and_then(|s| s.as_str()).unwrap_or("");
                let p = val.get("S").and_then(|s| s.as_str()).unwrap_or("");
                return is.starts_with(p);
            }
        }
        return false;
    }
    if expr.starts_with("contains(") && expr.ends_with(')') {
        let inner = &expr[9..expr.len() - 1];
        let parts: Vec<&str> = inner.splitn(2, ',').collect();
        if parts.len() == 2 {
            let attr_name = resolve_name(parts[0].trim(), names);
            let val = resolve_value(parts[1].trim(), values);
            if let Some(iv) = item.get(&attr_name) {
                let is = iv.get("S").and_then(|s| s.as_str()).unwrap_or("");
                let substr = val.get("S").and_then(|s| s.as_str()).unwrap_or("");
                return is.contains(substr);
            }
        }
        return false;
    }
    for op in &["<>", "<=", ">=", "<", ">", "="] {
        if let Some(pos) = expr.find(op) {
            let lhs = expr[..pos].trim();
            let rhs = expr[pos + op.len()..].trim();
            let lv = resolve_item_value(item, lhs, names);
            let rv = resolve_value(rhs, values);
            return match *op {
                "=" => av_compare(&lv, &rv) == Some(std::cmp::Ordering::Equal),
                "<>" => av_compare(&lv, &rv) != Some(std::cmp::Ordering::Equal),
                "<" => matches!(av_compare(&lv, &rv), Some(std::cmp::Ordering::Less)),
                "<=" => matches!(
                    av_compare(&lv, &rv),
                    Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                ),
                ">" => matches!(av_compare(&lv, &rv), Some(std::cmp::Ordering::Greater)),
                ">=" => matches!(
                    av_compare(&lv, &rv),
                    Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                ),
                _ => false,
            };
        }
    }
    true
}

fn find_top_level(expr: &str, keyword: &str) -> Option<usize> {
    let mut depth = 0i32;
    let bytes = expr.as_bytes();
    let klen = keyword.len();
    for i in 0..bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && i + klen <= bytes.len() && &expr[i..i + klen] == keyword {
            return Some(i);
        }
    }
    None
}

fn resolve_name(name: &str, names: &HashMap<String, String>) -> String {
    names.get(name).cloned().unwrap_or_else(|| name.to_string())
}

fn resolve_value(val: &str, values: &HashMap<String, Value>) -> Value {
    values.get(val).cloned().unwrap_or(Value::Null)
}

fn resolve_item_value(item: &Item, attr: &str, names: &HashMap<String, String>) -> Value {
    let name = resolve_name(attr, names);
    item.get(&name).cloned().unwrap_or(Value::Null)
}

// ---------------------------------------------------------------------------
// Update expression
// ---------------------------------------------------------------------------

pub fn apply_update_expression(
    item: &mut Item,
    expression: &str,
    attr_names: &HashMap<String, String>,
    attr_values: &HashMap<String, Value>,
) {
    let mut rest = expression.trim();
    while !rest.is_empty() {
        if rest.starts_with("SET ") {
            let (clause, rem) = extract_clause(&rest[4..]);
            apply_set_clause(item, &clause, attr_names, attr_values);
            rest = rem.trim();
        } else if rest.starts_with("REMOVE ") {
            let (clause, rem) = extract_clause(&rest[7..]);
            apply_remove_clause(item, &clause, attr_names);
            rest = rem.trim();
        } else if rest.starts_with("ADD ") {
            let (clause, rem) = extract_clause(&rest[4..]);
            apply_add_clause(item, &clause, attr_names, attr_values);
            rest = rem.trim();
        } else if rest.starts_with("DELETE ") {
            let (clause, rem) = extract_clause(&rest[7..]);
            apply_delete_clause(item, &clause, attr_names, attr_values);
            rest = rem.trim();
        } else {
            break;
        }
    }
}

fn extract_clause(input: &str) -> (String, &str) {
    let keywords = ["SET ", "REMOVE ", "ADD ", "DELETE "];
    let mut end = input.len();
    for kw in &keywords {
        if let Some(pos) = input.find(kw)
            && pos < end
        {
            end = pos;
        }
    }
    (input[..end].trim().to_string(), &input[end..])
}

fn apply_set_clause(
    item: &mut Item,
    clause: &str,
    names: &HashMap<String, String>,
    values: &HashMap<String, Value>,
) {
    for assignment in clause.split(',') {
        let assignment = assignment.trim();
        if let Some(eq_pos) = assignment.find('=') {
            let lhs = resolve_name(assignment[..eq_pos].trim(), names);
            let rhs = assignment[eq_pos + 1..].trim();
            let value = resolve_value(rhs, values);
            item.insert(lhs, value);
        }
    }
}

fn apply_remove_clause(item: &mut Item, clause: &str, names: &HashMap<String, String>) {
    for attr in clause.split(',') {
        let name = resolve_name(attr.trim(), names);
        item.remove(&name);
    }
}

fn apply_add_clause(
    item: &mut Item,
    clause: &str,
    names: &HashMap<String, String>,
    values: &HashMap<String, Value>,
) {
    for part in clause.split(',') {
        let tokens: Vec<&str> = part.trim().splitn(2, ' ').collect();
        if tokens.len() == 2 {
            let name = resolve_name(tokens[0].trim(), names);
            let delta = resolve_value(tokens[1].trim(), values);
            if let Some(existing) = item.get_mut(&name) {
                if let (Some(cur), Some(d)) = (
                    existing
                        .get("N")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<f64>().ok()),
                    delta
                        .get("N")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<f64>().ok()),
                ) {
                    *existing = serde_json::json!({"N": (cur + d).to_string()});
                }
            } else {
                item.insert(name, delta);
            }
        }
    }
}

fn apply_delete_clause(
    item: &mut Item,
    clause: &str,
    names: &HashMap<String, String>,
    _values: &HashMap<String, Value>,
) {
    for part in clause.split(',') {
        let tokens: Vec<&str> = part.trim().splitn(2, ' ').collect();
        if !tokens.is_empty() {
            let name = resolve_name(tokens[0].trim(), names);
            item.remove(&name);
        }
    }
}

// ---------------------------------------------------------------------------
// Condition expression
// ---------------------------------------------------------------------------

pub fn check_condition(
    item: Option<&Item>,
    condition: &str,
    attr_names: &HashMap<String, String>,
    attr_values: &HashMap<String, Value>,
) -> Result<(), String> {
    let empty = HashMap::new();
    let item_ref = item.unwrap_or(&empty);
    if evaluate_filter(item_ref, condition, attr_names, attr_values) {
        Ok(())
    } else {
        Err("ConditionalCheckFailedException".to_string())
    }
}
