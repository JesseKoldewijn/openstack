use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use dashmap::mapref::one::{Ref, RefMut};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

// ---------------------------------------------------------------------------
// AttributeValue — typed enum mirroring the DynamoDB wire format.
//
// DynamoDB represents each attribute as a single-key object:
//   {"S": "hello"}, {"N": "42"}, {"BOOL": true}, {"NULL": true},
//   {"B": "<base64>"}, {"SS": [...]}, {"NS": [...]}, {"BS": [...]},
//   {"L": [...]}, {"M": {...}}
//
// We store this as a typed Rust enum for O(1) variant dispatch and to
// eliminate repeated `.get("S").and_then(|v| v.as_str())` chains.
// The custom Serialize/Deserialize impls maintain full wire-format parity.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    /// String
    S(String),
    /// Number (stored as string to preserve precision, per DynamoDB wire format)
    N(String),
    /// Binary (base64-encoded string on the wire)
    B(String),
    /// Boolean
    Bool(bool),
    /// Null
    Null,
    /// String set
    Ss(Vec<String>),
    /// Number set
    Ns(Vec<String>),
    /// Binary set
    Bs(Vec<String>),
    /// List
    L(Vec<AttributeValue>),
    /// Map
    M(HashMap<String, AttributeValue>),
}

impl Serialize for AttributeValue {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = ser.serialize_map(Some(1))?;
        match self {
            AttributeValue::S(v) => map.serialize_entry("S", v)?,
            AttributeValue::N(v) => map.serialize_entry("N", v)?,
            AttributeValue::B(v) => map.serialize_entry("B", v)?,
            AttributeValue::Bool(v) => map.serialize_entry("BOOL", v)?,
            AttributeValue::Null => map.serialize_entry("NULL", &true)?,
            AttributeValue::Ss(v) => map.serialize_entry("SS", v)?,
            AttributeValue::Ns(v) => map.serialize_entry("NS", v)?,
            AttributeValue::Bs(v) => map.serialize_entry("BS", v)?,
            AttributeValue::L(v) => map.serialize_entry("L", v)?,
            AttributeValue::M(v) => map.serialize_entry("M", v)?,
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for AttributeValue {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        // Deserialize as a raw serde_json::Value first, then interpret.
        let v = Value::deserialize(de)?;
        Ok(av_from_json(&v))
    }
}

/// Convert a raw JSON `Value` (in DynamoDB wire format) into an `AttributeValue`.
/// Unknown or missing type tags fall back to `AttributeValue::Null`.
pub fn av_from_json(v: &Value) -> AttributeValue {
    if let Some(s) = v.get("S").and_then(|x| x.as_str()) {
        return AttributeValue::S(s.to_string());
    }
    if let Some(n) = v.get("N").and_then(|x| x.as_str()) {
        return AttributeValue::N(n.to_string());
    }
    if let Some(b) = v.get("B").and_then(|x| x.as_str()) {
        return AttributeValue::B(b.to_string());
    }
    if let Some(b) = v.get("BOOL").and_then(|x| x.as_bool()) {
        return AttributeValue::Bool(b);
    }
    if v.get("NULL").and_then(|x| x.as_bool()).unwrap_or(false) {
        return AttributeValue::Null;
    }
    if let Some(arr) = v.get("SS").and_then(|x| x.as_array()) {
        let items: Vec<String> = arr
            .iter()
            .filter_map(|s| s.as_str().map(String::from))
            .collect();
        return AttributeValue::Ss(items);
    }
    if let Some(arr) = v.get("NS").and_then(|x| x.as_array()) {
        let items: Vec<String> = arr
            .iter()
            .filter_map(|s| s.as_str().map(String::from))
            .collect();
        return AttributeValue::Ns(items);
    }
    if let Some(arr) = v.get("BS").and_then(|x| x.as_array()) {
        let items: Vec<String> = arr
            .iter()
            .filter_map(|s| s.as_str().map(String::from))
            .collect();
        return AttributeValue::Bs(items);
    }
    if let Some(arr) = v.get("L").and_then(|x| x.as_array()) {
        let items: Vec<AttributeValue> = arr.iter().map(av_from_json).collect();
        return AttributeValue::L(items);
    }
    if let Some(obj) = v.get("M").and_then(|x| x.as_object()) {
        let map: HashMap<String, AttributeValue> =
            obj.iter().map(|(k, v)| (k.clone(), av_from_json(v))).collect();
        return AttributeValue::M(map);
    }
    AttributeValue::Null
}

/// Convert a `serde_json::Map` of wire-format attribute values into an `Item`.
pub fn item_from_json_map(m: &serde_json::Map<String, Value>) -> Item {
    m.iter().map(|(k, v)| (k.clone(), av_from_json(v))).collect()
}

pub type Item = HashMap<String, AttributeValue>;

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
    pub keys: Item,
    pub new_image: Option<Item>,
    pub old_image: Option<Item>,
    pub approximate_creation_date_time: f64,
}

// ---------------------------------------------------------------------------
// Materialized secondary index
// ---------------------------------------------------------------------------

/// A hash-partitioned index maintained in parallel with the primary item store.
///
/// Each entry in `data` corresponds to one hash-key value and holds the list
/// of `(pk_hash, pk_sort)` primary-key pairs for all items that landed in that
/// partition.  The list is kept sorted by the index range-key value so that
/// queries can apply a range condition and sort in a single pass.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MaterializedIndex {
    /// Attribute name of this index's hash key.
    pub hash_key_name: String,
    /// Attribute name of this index's range key, if any.
    pub range_key_name: Option<String>,
    /// index_hash_val → Vec<(pk_hash, pk_sort)>
    pub data: HashMap<String, Vec<(String, String)>>,
}

impl MaterializedIndex {
    pub fn new(hash_key_name: String, range_key_name: Option<String>) -> Self {
        Self {
            hash_key_name,
            range_key_name,
            data: HashMap::new(),
        }
    }

    /// Insert an item's primary-key pair into this index (no-op if the item
    /// does not carry the index hash key — sparse index semantics).
    pub fn add(&mut self, item: &Item, pk_hash: String, pk_sort: String) {
        let Some(hv) = item.get(&self.hash_key_name).and_then(av_to_key_str) else {
            return;
        };
        let bucket = self.data.entry(hv).or_default();
        // Avoid duplicate entries (can happen if put_item is called twice
        // for the same primary key without a preceding delete).
        if !bucket.contains(&(pk_hash.clone(), pk_sort.clone())) {
            bucket.push((pk_hash, pk_sort));
        }
    }

    /// Remove an item's primary-key pair from this index.
    pub fn remove(&mut self, item: &Item, pk_hash: &str, pk_sort: &str) {
        let Some(hv) = item.get(&self.hash_key_name).and_then(av_to_key_str) else {
            return;
        };
        if let Some(bucket) = self.data.get_mut(&hv) {
            bucket.retain(|(h, s)| h != pk_hash || s != pk_sort);
            if bucket.is_empty() {
                self.data.remove(&hv);
            }
        }
    }
}

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
    /// partition_key → sort_key → Item  (BTreeMap keeps sort keys ordered for Query)
    pub items: HashMap<String, BTreeMap<String, Item>>,
    pub stream_records: VecDeque<StreamRecord>,
    pub stream_sequence: u64,
    /// Materialized secondary indexes — kept in sync on every write.
    pub index_data: HashMap<String, MaterializedIndex>,
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

        // Build materialized indexes from GSI/LSI definitions.
        let mut index_data: HashMap<String, MaterializedIndex> = HashMap::new();
        for gsi in &gsis {
            let hk = gsi
                .key_schema
                .iter()
                .find(|k| k.key_type == KeyType::HASH)
                .map(|k| k.attribute_name.clone())
                .unwrap_or_default();
            let rk = gsi
                .key_schema
                .iter()
                .find(|k| k.key_type == KeyType::RANGE)
                .map(|k| k.attribute_name.clone());
            index_data.insert(gsi.index_name.clone(), MaterializedIndex::new(hk, rk));
        }
        for lsi in &lsis {
            let hk = lsi
                .key_schema
                .iter()
                .find(|k| k.key_type == KeyType::HASH)
                .map(|k| k.attribute_name.clone())
                .unwrap_or_default();
            let rk = lsi
                .key_schema
                .iter()
                .find(|k| k.key_type == KeyType::RANGE)
                .map(|k| k.attribute_name.clone());
            index_data.insert(lsi.index_name.clone(), MaterializedIndex::new(hk, rk));
        }

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
            stream_records: VecDeque::new(),
            stream_sequence: 0,
            index_data,
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

    fn make_key_map(&self, item: &Item) -> Item {
        let mut keys = Item::new();
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
        keys: Item,
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
        self.stream_records.push_back(rec);
        if self.stream_records.len() > 1000 {
            self.stream_records.pop_front();
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

        // Remove old item from all materialized indexes before replacing it.
        let old = self.items.entry(hk.clone()).or_default().insert(sk.clone(), item.clone());
        if let Some(ref old_item) = old {
            for idx in self.index_data.values_mut() {
                idx.remove(old_item, &hk, &sk);
            }
        } else {
            self.item_count += 1;
        }

        // Add new item to all materialized indexes.
        for idx in self.index_data.values_mut() {
            idx.add(&item, hk.clone(), sk.clone());
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
        if let Some(ref old_item) = old {
            for idx in self.index_data.values_mut() {
                idx.remove(old_item, &hk, &sk);
            }
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
        if let Some(idx_name) = index_name {
            // Fast path: use the materialized index when available (O(1) hash
            // lookup instead of scanning all items).
            if let Some(mat_idx) = self.index_data.get(idx_name) {
                let idx_rk = mat_idx.range_key_name.as_deref();
                let pairs = mat_idx.data.get(hash_key_val);
                let mut items: Vec<&Item> = pairs
                    .into_iter()
                    .flat_map(|v| v.iter())
                    .filter_map(|(h, s)| self.items.get(h)?.get(s))
                    .filter(|item| {
                        if let Some(rc) = range_condition
                            && let Some(rk) = idx_rk
                            && let Some(rv) = item.get(rk)
                        {
                            rc.matches(rv)
                        } else {
                            true
                        }
                    })
                    .collect();
                if let Some(rk) = idx_rk {
                    items.sort_by(|a, b| {
                        let ak = a.get(rk).and_then(av_sort_key);
                        let bk = b.get(rk).and_then(av_sort_key);
                        let ord = ak.partial_cmp(&bk).unwrap_or(std::cmp::Ordering::Equal);
                        if scan_index_forward { ord } else { ord.reverse() }
                    });
                }
                return items;
            }

            // Fallback: full-table scan (handles tables created before
            // materialized indexes were introduced, or unknown index names).
            let idx_hk = self.index_hash_key(idx_name).unwrap_or("");
            let idx_rk = self.index_range_key(idx_name);
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
                    if scan_index_forward { ord } else { ord.reverse() }
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

    /// Add a GSI dynamically (called from UpdateTable) and populate it from
    /// all existing items so it is immediately queryable.
    pub fn add_gsi(&mut self, gsi: GlobalSecondaryIndex) {
        let hk = gsi
            .key_schema
            .iter()
            .find(|k| k.key_type == KeyType::HASH)
            .map(|k| k.attribute_name.clone())
            .unwrap_or_default();
        let rk = gsi
            .key_schema
            .iter()
            .find(|k| k.key_type == KeyType::RANGE)
            .map(|k| k.attribute_name.clone());
        let mut idx = MaterializedIndex::new(hk, rk);
        // Back-fill from existing items.
        for (pk_hash, partition) in &self.items {
            for (pk_sort, item) in partition {
                idx.add(item, pk_hash.clone(), pk_sort.clone());
            }
        }
        self.index_data.insert(gsi.index_name.clone(), idx);
        self.global_secondary_indexes.push(gsi);
    }

    /// Remove a GSI dynamically (called from UpdateTable).
    pub fn remove_gsi(&mut self, index_name: &str) {
        self.index_data.remove(index_name);
        self.global_secondary_indexes
            .retain(|g| g.index_name != index_name);
    }

    pub fn index_hash_key(&self, index_name: &str) -> Option<&str> {
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
    Eq(AttributeValue),
    Lt(AttributeValue),
    Lte(AttributeValue),
    Gt(AttributeValue),
    Gte(AttributeValue),
    Between(AttributeValue, AttributeValue),
    BeginsWith(String),
}

impl RangeCondition {
    pub fn matches(&self, av: &AttributeValue) -> bool {
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
            RangeCondition::BeginsWith(prefix) => {
                if let AttributeValue::S(s) = av {
                    s.starts_with(prefix.as_str())
                } else {
                    false
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DynamoDbStore
// ---------------------------------------------------------------------------

/// Serialize `Arc<DashMap<K, V>>` by serializing the inner DashMap.
mod arc_dashmap_serde {
    use std::sync::Arc;

    use dashmap::DashMap;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S, K, V>(map: &Arc<DashMap<K, V>>, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        K: Serialize + std::hash::Hash + Eq,
        V: Serialize,
    {
        map.as_ref().serialize(ser)
    }

    pub fn deserialize<'de, D, K, V>(de: D) -> Result<Arc<DashMap<K, V>>, D::Error>
    where
        D: Deserializer<'de>,
        K: Deserialize<'de> + std::hash::Hash + Eq,
        V: Deserialize<'de>,
    {
        let inner = DashMap::<K, V>::deserialize(de)?;
        Ok(Arc::new(inner))
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DynamoDbStore {
    #[serde(with = "arc_dashmap_serde")]
    pub tables: Arc<DashMap<String, Table>>,
}

impl DynamoDbStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_table(
        &self,
        name: impl Into<String>,
        account_id: &str,
        region: &str,
        key_schema: Vec<KeySchemaElement>,
        attribute_definitions: Vec<AttributeDefinition>,
        gsis: Vec<GlobalSecondaryIndex>,
        lsis: Vec<LocalSecondaryIndex>,
        stream_spec: StreamSpecification,
    ) {
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
            self.tables.insert(name, table);
        }
    }

    pub fn delete_table(&self, name: &str) -> Option<Table> {
        self.tables.remove(name).map(|(_, t)| t)
    }

    pub fn get_table(&self, name: &str) -> Option<Ref<'_, String, Table>> {
        self.tables.get(name)
    }

    pub fn get_table_mut(&self, name: &str) -> Option<RefMut<'_, String, Table>> {
        self.tables.get_mut(name)
    }

    pub fn list_table_names(&self) -> Vec<String> {
        self.tables.iter().map(|r| r.key().clone()).collect()
    }

    /// Cheaply clone the Arc to allow callers to release the outer store lock
    /// and then access tables via the inner DashMap's own per-shard locking.
    pub fn tables_ref(&self) -> Arc<DashMap<String, Table>> {
        Arc::clone(&self.tables)
    }
}

// ---------------------------------------------------------------------------
// Attribute value helpers
// ---------------------------------------------------------------------------

/// Extract a string key representation from an `AttributeValue` for use as
/// a partition-key or sort-key string in the items map.
pub fn av_to_key_str(v: &AttributeValue) -> Option<String> {
    match v {
        AttributeValue::S(s) => Some(s.clone()),
        AttributeValue::N(n) => Some(n.clone()),
        AttributeValue::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub fn av_sort_key(v: &AttributeValue) -> Option<SortKeyValue> {
    match v {
        AttributeValue::S(s) => Some(SortKeyValue::S(s.clone())),
        AttributeValue::N(n) => n.parse::<f64>().ok().map(SortKeyValue::N),
        _ => None,
    }
}

pub fn av_compare(a: &AttributeValue, b: &AttributeValue) -> Option<std::cmp::Ordering> {
    let ak = av_sort_key(a)?;
    let bk = av_sort_key(b)?;
    ak.partial_cmp(&bk)
}

// ---------------------------------------------------------------------------
// Filter expression evaluation
// ---------------------------------------------------------------------------

/// `ExpressionAttributeValues` on the wire is `{":v": {"S": "foo"}}` — we
/// parse the inner `{"S": "foo"}` objects into `AttributeValue` at the call
/// site in `provider.rs`, so here we receive a typed map.
pub fn evaluate_filter(
    item: &Item,
    expression: &str,
    attr_names: &HashMap<String, String>,
    attr_values: &HashMap<String, AttributeValue>,
) -> bool {
    evaluate_expr(item, expression.trim(), attr_names, attr_values)
}

fn evaluate_expr(
    item: &Item,
    expr: &str,
    names: &HashMap<String, String>,
    values: &HashMap<String, AttributeValue>,
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
                let is = if let AttributeValue::S(s) = iv { s.as_str() } else { "" };
                let p = if let AttributeValue::S(s) = val { s.as_str() } else { "" };
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
                let is = if let AttributeValue::S(s) = iv { s.as_str() } else { "" };
                let substr = if let AttributeValue::S(s) = val { s.as_str() } else { "" };
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
                "=" => av_compare(&lv, rv) == Some(std::cmp::Ordering::Equal),
                "<>" => av_compare(&lv, rv) != Some(std::cmp::Ordering::Equal),
                "<" => matches!(av_compare(&lv, rv), Some(std::cmp::Ordering::Less)),
                "<=" => matches!(
                    av_compare(&lv, rv),
                    Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                ),
                ">" => matches!(av_compare(&lv, rv), Some(std::cmp::Ordering::Greater)),
                ">=" => matches!(
                    av_compare(&lv, rv),
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

/// Returns a reference to the `AttributeValue` in `values`, or `&AttributeValue::Null`
/// via a thread-local sentinel when the key is not found.
fn resolve_value<'a>(val: &str, values: &'a HashMap<String, AttributeValue>) -> &'a AttributeValue {
    static NULL_AV: AttributeValue = AttributeValue::Null;
    values.get(val).unwrap_or(&NULL_AV)
}

fn resolve_item_value<'a>(
    item: &'a Item,
    attr: &str,
    names: &HashMap<String, String>,
) -> &'a AttributeValue {
    static NULL_AV: AttributeValue = AttributeValue::Null;
    let name = resolve_name(attr, names);
    item.get(&name).unwrap_or(&NULL_AV)
}

// ---------------------------------------------------------------------------
// Update expression
// ---------------------------------------------------------------------------

pub fn apply_update_expression(
    item: &mut Item,
    expression: &str,
    attr_names: &HashMap<String, String>,
    attr_values: &HashMap<String, AttributeValue>,
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
            apply_delete_clause(item, &clause, attr_names);
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
    values: &HashMap<String, AttributeValue>,
) {
    for assignment in clause.split(',') {
        let assignment = assignment.trim();
        if let Some(eq_pos) = assignment.find('=') {
            let lhs = resolve_name(assignment[..eq_pos].trim(), names);
            let rhs = assignment[eq_pos + 1..].trim();
            let value = resolve_value(rhs, values).clone();
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
    values: &HashMap<String, AttributeValue>,
) {
    for part in clause.split(',') {
        let tokens: Vec<&str> = part.trim().splitn(2, ' ').collect();
        if tokens.len() == 2 {
            let name = resolve_name(tokens[0].trim(), names);
            let delta = resolve_value(tokens[1].trim(), values);
            if let Some(existing) = item.get_mut(&name) {
                if let (AttributeValue::N(cur_s), AttributeValue::N(d_s)) = (&*existing, delta) {
                    if let (Ok(cur), Ok(d)) = (cur_s.parse::<f64>(), d_s.parse::<f64>()) {
                        *existing = AttributeValue::N((cur + d).to_string());
                    }
                }
            } else {
                item.insert(name, delta.clone());
            }
        }
    }
}

fn apply_delete_clause(item: &mut Item, clause: &str, names: &HashMap<String, String>) {
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
    attr_values: &HashMap<String, AttributeValue>,
) -> Result<(), String> {
    let empty = HashMap::new();
    let item_ref = item.unwrap_or(&empty);
    if evaluate_filter(item_ref, condition, attr_names, attr_values) {
        Ok(())
    } else {
        Err("ConditionalCheckFailedException".to_string())
    }
}
