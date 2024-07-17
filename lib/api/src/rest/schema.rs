use std::collections::HashMap;
use std::str::FromStr;

use common::types::ScoreType;
use schemars::JsonSchema;
use segment::common::utils::MaybeOneOrMany;
use segment::data_types::order_by::OrderBy;
use segment::json_path::JsonPath;
use segment::types::{
    Condition, FieldCondition, Filter, Match, Payload, SearchParams, ShardKey,
    WithPayloadInterface, WithVector,
};
use serde::{Deserialize, Serialize};
use sparse::common::sparse_vector::SparseVector;
use validator::Validate;

/// Type for dense vector
pub type DenseVector = Vec<segment::data_types::vectors::VectorElementType>;

/// Type for multi dense vector
pub type MultiDenseVector = Vec<DenseVector>;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged, rename_all = "snake_case")]
pub enum Vector {
    Dense(DenseVector),
    Sparse(sparse::common::sparse_vector::SparseVector),
    MultiDense(MultiDenseVector),
}

/// Full vector data per point separator with single and multiple vector modes
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged, rename_all = "snake_case")]
pub enum VectorStruct {
    Single(DenseVector),
    MultiDense(MultiDenseVector),
    Named(HashMap<String, Vector>),
}

impl VectorStruct {
    /// Check if this vector struct is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            VectorStruct::Single(vector) => vector.is_empty(),
            VectorStruct::MultiDense(vector) => vector.is_empty(),
            VectorStruct::Named(vectors) => vectors.values().all(|v| match v {
                Vector::Dense(vector) => vector.is_empty(),
                Vector::Sparse(vector) => vector.indices.is_empty(),
                Vector::MultiDense(vector) => vector.is_empty(),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged, rename_all = "snake_case")]
pub enum BatchVectorStruct {
    Single(Vec<DenseVector>),
    MultiDense(Vec<MultiDenseVector>),
    Named(HashMap<String, Vec<Vector>>),
}

#[derive(Debug, Deserialize, Serialize, Clone, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum ShardKeySelector {
    ShardKey(ShardKey),
    ShardKeys(Vec<ShardKey>),
    // ToDo: select by pattern
}

fn version_example() -> segment::types::SeqNumberType {
    1
}

fn score_example() -> common::types::ScoreType {
    0.75
}

fn payload_example() -> Option<segment::types::Payload> {
    Some(Payload::from(serde_json::json!({
        "city": "London",
        "color": "green",
    })))
}

fn vector_example() -> Option<VectorStruct> {
    Some(VectorStruct::Single(vec![
        0.875, 0.140625, -0.15625, 0.96875,
    ]))
}

fn shard_key_example() -> Option<segment::types::ShardKey> {
    Some(ShardKey::from("region_1"))
}

fn order_value_example() -> Option<segment::data_types::order_by::OrderValue> {
    Some(segment::data_types::order_by::OrderValue::from(1))
}

/// Search result
#[derive(Serialize, JsonSchema, Clone, Debug)]
pub struct ScoredPoint {
    /// Point id
    pub id: segment::types::ExtendedPointId,
    /// Point version
    #[schemars(example = "version_example")]
    pub version: segment::types::SeqNumberType,
    /// Points vector distance to the query vector
    #[schemars(example = "score_example")]
    pub score: common::types::ScoreType,
    /// Payload - values assigned to the point
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(example = "payload_example")]
    pub payload: Option<segment::types::Payload>,
    /// Vector of the point
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(example = "vector_example")]
    pub vector: Option<VectorStruct>,
    /// Shard Key
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(example = "shard_key_example")]
    pub shard_key: Option<segment::types::ShardKey>,
    /// Order-by value
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(example = "order_value_example")]
    pub order_value: Option<segment::data_types::order_by::OrderValue>,
}

/// Point data
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Record {
    /// Id of the point
    pub id: segment::types::PointIdType,
    /// Payload - values assigned to the point
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(example = "payload_example")]
    pub payload: Option<segment::types::Payload>,
    /// Vector of the point
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(example = "vector_example")]
    pub vector: Option<VectorStruct>,
    /// Shard Key
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(example = "shard_key_example")]
    pub shard_key: Option<segment::types::ShardKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(example = "order_value_example")]
    pub order_value: Option<segment::data_types::order_by::OrderValue>,
}

/// Vector data separator for named and unnamed modes
/// Unnamed mode:
///
/// {
///   "vector": [1.0, 2.0, 3.0]
/// }
///
/// or named mode:
///
/// {
///   "vector": {
///     "vector": [1.0, 2.0, 3.0],
///     "name": "image-embeddings"
///   }
/// }
#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
#[serde(untagged)]
pub enum NamedVectorStruct {
    #[schemars(example = "vector_example")]
    Default(segment::data_types::vectors::DenseVector),
    Dense(segment::data_types::vectors::NamedVector),
    Sparse(segment::data_types::vectors::NamedSparseVector),
    // No support for multi-dense vectors in search
}

fn order_by_interface_example() -> OrderByInterface {
    OrderByInterface::Key(JsonPath::from_str("timestamp").unwrap())
}

#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(untagged)]
#[schemars(example = "order_by_interface_example")]
pub enum OrderByInterface {
    Key(JsonPath),
    Struct(OrderBy),
}

fn fusion_example() -> Fusion {
    Fusion::Rrf
}

/// Fusion algorithm allows to combine results of multiple prefetches.
/// Available fusion algorithms:
/// * `rrf` - Rank Reciprocal Fusion
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(example = "fusion_example")]
pub enum Fusion {
    Rrf,
}

fn multi_dense_vector_example() -> MultiDenseVector {
    vec![
        vec![1.0, 2.0, 3.0],
        vec![4.0, 5.0, 6.0],
        vec![7.0, 8.0, 9.0],
    ]
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum VectorInput {
    #[schemars(example = "vector_example")]
    DenseVector(DenseVector),
    SparseVector(SparseVector),
    #[schemars(example = "multi_dense_vector_example")]
    MultiDenseVector(MultiDenseVector),
    Id(segment::types::PointIdType),
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Validate)]
pub struct QueryRequestInternal {
    /// Sub-requests to perform first. If present, the query will be performed on the results of the prefetch(es).
    #[validate]
    #[serde(default, with = "MaybeOneOrMany")]
    #[schemars(with = "MaybeOneOrMany<Prefetch>")]
    pub prefetch: Option<Vec<Prefetch>>,

    /// Query to perform. If missing without prefetches, returns points ordered by their IDs.
    #[validate]
    pub query: Option<QueryInterface>,

    /// Define which vector name to use for querying. If missing, the default vector is used.
    pub using: Option<String>,

    /// Filter conditions - return only those points that satisfy the specified conditions.
    #[validate]
    pub filter: Option<Filter>,

    /// Search params for when there is no prefetch
    #[validate]
    pub params: Option<SearchParams>,

    /// Return points with scores better than this threshold.
    pub score_threshold: Option<ScoreType>,

    /// Max number of points to return. Default is 10.
    #[validate(range(min = 1))]
    pub limit: Option<usize>,

    /// Offset of the result. Skip this many points. Default is 0
    pub offset: Option<usize>,

    /// Options for specifying which vectors to include into the response. Default is false.
    pub with_vector: Option<WithVector>,

    /// Options for specifying which payload to include or not. Default is false.
    pub with_payload: Option<WithPayloadInterface>,

    /// The location to use for IDs lookup, if not specified - use the current collection and the 'using' vector
    /// Note: the other collection vectors should have the same vector size as the 'using' vector in the current collection
    #[serde(default)]
    pub lookup_from: Option<LookupLocation>,
}

fn query_request_example() -> QueryRequest {
    QueryRequest {
        internal: QueryRequestInternal {
            prefetch: None,
            query: Some(QueryInterface::Nearest(VectorInput::DenseVector(vec![
                0.875, 0.140625, -0.15625, 0.96875,
            ]))),
            using: Some("image_vector".to_string()),
            filter: Some(Filter::new_must(Condition::Field(
                FieldCondition::new_match(
                    JsonPath::from_str("city").unwrap(),
                    Match::new_text("Berlin"),
                ),
            ))),
            params: Some(SearchParams {
                hnsw_ef: Some(100),
                exact: false,
                quantization: None,
                indexed_only: false,
            }),
            score_threshold: Some(0.25),
            limit: Some(10),
            offset: Some(0),
            with_vector: Some(WithVector::Bool(true)),
            with_payload: Some(WithPayloadInterface::Bool(true)),
            lookup_from: Some(lookup_location_example()),
        },
        shard_key: Some(ShardKeySelector::ShardKey(ShardKey::from("region_1"))),
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(example = "query_request_example")]
pub struct QueryRequest {
    #[validate]
    #[serde(flatten)]
    pub internal: QueryRequestInternal,
    pub shard_key: Option<ShardKeySelector>,
}

fn query_request_batch_example() -> QueryRequestBatch {
    QueryRequestBatch {
        searches: vec![
            query_request_example(),
            QueryRequest {
                internal: QueryRequestInternal {
                    prefetch: None,
                    query: Some(QueryInterface::Nearest(VectorInput::DenseVector(vec![
                        0.875, 0.140625, -0.15625, 0.96875,
                    ]))),
                    using: Some("code_vector".to_string()),
                    filter: Some(Filter::new_must(Condition::Field(
                        FieldCondition::new_match(
                            JsonPath::from_str("city").unwrap(),
                            Match::new_text("New York"),
                        ),
                    ))),
                    params: Some(SearchParams {
                        hnsw_ef: Some(100),
                        exact: false,
                        quantization: None,
                        indexed_only: false,
                    }),
                    score_threshold: Some(0.25),
                    limit: Some(10),
                    offset: Some(0),
                    with_vector: Some(WithVector::Bool(true)),
                    with_payload: Some(WithPayloadInterface::Bool(true)),
                    lookup_from: Some(lookup_location_example()),
                },
                shard_key: Some(ShardKeySelector::ShardKey(ShardKey::from("region_1"))),
            },
        ],
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(example = "query_request_batch_example")]
pub struct QueryRequestBatch {
    #[validate]
    pub searches: Vec<QueryRequest>,
}

fn query_response_example() -> QueryResponse {
    QueryResponse {
        points: vec![
            ScoredPoint {
                id: segment::types::ExtendedPointId::from(1),
                version: 1,
                score: 0.25,
                payload: Some(Payload::from(serde_json::json!({
                    "city": "London",
                    "color": "green",
                }))),
                vector: Some(VectorStruct::Single(vec![
                    0.875, 0.140625, -0.15625, 0.96875,
                ])),
                shard_key: Some(ShardKey::from("region_1")),
                order_value: Some(segment::data_types::order_by::OrderValue::from(1)),
            },
            ScoredPoint {
                id: segment::types::ExtendedPointId::from(2),
                version: 1,
                score: 0.25,
                payload: Some(Payload::from(serde_json::json!({
                    "city": "Berlin",
                    "color": "red",
                }))),
                vector: Some(VectorStruct::Single(vec![
                    0.475, 0.440625, -0.25625, 0.36875,
                ])),
                shard_key: Some(ShardKey::from("region_1")),
                order_value: Some(segment::data_types::order_by::OrderValue::from(1)),
            },
        ],
    }
}

#[derive(Debug, Serialize, JsonSchema)]
#[schemars(example = "query_response_example")]
pub struct QueryResponse {
    pub points: Vec<ScoredPoint>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum QueryInterface {
    Nearest(VectorInput),
    Query(Query),
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Query {
    /// Find the nearest neighbors to this vector.
    Nearest(NearestQuery),

    /// Use multiple positive and negative vectors to find the results.
    Recommend(RecommendQuery),

    /// Search for nearest points, but constrain the search space with context
    Discover(DiscoverQuery),

    /// Return points that live in positive areas.
    Context(ContextQuery),

    /// Order the points by a payload field.
    OrderBy(OrderByQuery),

    /// Fuse the results of multiple prefetches.
    Fusion(FusionQuery),
}

fn nearest_query_example() -> NearestQuery {
    NearestQuery {
        nearest: VectorInput::DenseVector(vec![0.875, 0.140625, -0.15625, 0.96875]),
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(example = "nearest_query_example")]
pub struct NearestQuery {
    pub nearest: VectorInput,
}

fn recommend_query_example() -> RecommendQuery {
    RecommendQuery {
        recommend: recommend_input_example(),
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(example = "recommend_query_example")]
pub struct RecommendQuery {
    pub recommend: RecommendInput,
}

fn discover_query_example() -> DiscoverQuery {
    DiscoverQuery {
        discover: discover_input_example(),
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(example = "discover_query_example")]
pub struct DiscoverQuery {
    pub discover: DiscoverInput,
}

fn context_query_example() -> ContextQuery {
    ContextQuery {
        context: context_input_example(),
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(example = "context_query_example")]
pub struct ContextQuery {
    pub context: ContextInput,
}

fn order_by_query_example() -> OrderByQuery {
    OrderByQuery {
        order_by: OrderByInterface::Key(JsonPath::from_str("timestamp").unwrap()),
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(example = "order_by_query_example")]
pub struct OrderByQuery {
    pub order_by: OrderByInterface,
}

fn fusion_query_example() -> FusionQuery {
    FusionQuery {
        fusion: Fusion::Rrf,
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(example = "fusion_query_example")]
pub struct FusionQuery {
    pub fusion: Fusion,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Validate)]
pub struct Prefetch {
    /// Sub-requests to perform first. If present, the query will be performed on the results of the prefetches.
    #[validate]
    #[serde(default, with = "MaybeOneOrMany")]
    #[schemars(with = "MaybeOneOrMany<Prefetch>")]
    pub prefetch: Option<Vec<Prefetch>>,

    /// Query to perform. If missing without prefetches, returns points ordered by their IDs.
    #[validate]
    pub query: Option<QueryInterface>,

    /// Define which vector name to use for querying. If missing, the default vector is used.
    pub using: Option<String>,

    /// Filter conditions - return only those points that satisfy the specified conditions.
    #[validate]
    pub filter: Option<Filter>,

    /// Search params for when there is no prefetch
    #[validate]
    pub params: Option<SearchParams>,

    /// Return points with scores better than this threshold.
    pub score_threshold: Option<ScoreType>,

    /// Max number of points to return. Default is 10.
    #[validate(range(min = 1))]
    pub limit: Option<usize>,

    /// The location to use for IDs lookup, if not specified - use the current collection and the 'using' vector
    /// Note: the other collection vectors should have the same vector size as the 'using' vector in the current collection
    #[serde(default)]
    pub lookup_from: Option<LookupLocation>,
}

/// How to use positive and negative examples to find the results, default is `average_vector`:
///
/// * `average_vector` - Average positive and negative vectors and create a single query
///   with the formula `query = avg_pos + avg_pos - avg_neg`. Then performs normal search.
///
/// * `best_score` - Uses custom search objective. Each candidate is compared against all
///   examples, its score is then chosen from the `max(max_pos_score, max_neg_score)`.
///   If the `max_neg_score` is chosen then it is squared and negated, otherwise it is just
///   the `max_pos_score`.

#[derive(Debug, Deserialize, Serialize, JsonSchema, Default, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum RecommendStrategy {
    #[default]
    AverageVector,
    BestScore,
}

fn recommend_input_example() -> RecommendInput {
    RecommendInput {
        positive: Some(vec![VectorInput::DenseVector(vec![
            0.875, 0.140625, -0.15625, 0.96875,
        ])]),
        negative: Some(vec![VectorInput::DenseVector(vec![
            0.475, 0.440625, -0.25625, 0.36875,
        ])]),
        strategy: Some(RecommendStrategy::AverageVector),
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "recommend_input_example")]
pub struct RecommendInput {
    /// Look for vectors closest to the vectors from these points
    pub positive: Option<Vec<VectorInput>>,

    /// Try to avoid vectors like the vector from these points
    pub negative: Option<Vec<VectorInput>>,

    /// How to use the provided vectors to find the results
    pub strategy: Option<RecommendStrategy>,
}

impl RecommendInput {
    pub fn iter(&self) -> impl Iterator<Item = &VectorInput> {
        self.positive
            .iter()
            .flatten()
            .chain(self.negative.iter().flatten())
    }
}

fn discover_input_example() -> DiscoverInput {
    DiscoverInput {
        target: VectorInput::DenseVector(vec![0.875, 0.140625, -0.15625, 0.96875]),
        context: Some(vec![ContextPair {
            positive: VectorInput::DenseVector(vec![0.875, 0.140625, -0.15625, 0.96875]),
            negative: VectorInput::DenseVector(vec![0.475, 0.440625, -0.25625, 0.36875]),
        }]),
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(example = "discover_input_example")]
pub struct DiscoverInput {
    /// Use this as the primary search objective
    #[validate]
    pub target: VectorInput,

    /// Search space will be constrained by these pairs of vectors
    #[validate]
    #[serde(with = "MaybeOneOrMany")]
    #[schemars(with = "MaybeOneOrMany<ContextPair>")]
    pub context: Option<Vec<ContextPair>>,
}

fn context_input_example() -> ContextInput {
    ContextInput(Some(vec![context_pair_example()]))
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "context_input_example")]
pub struct ContextInput(
    /// Search space will be constrained by these pairs of vectors
    #[serde(with = "MaybeOneOrMany")]
    #[schemars(with = "MaybeOneOrMany<ContextPair>")]
    pub Option<Vec<ContextPair>>,
);

fn context_pair_example() -> ContextPair {
    ContextPair {
        positive: VectorInput::DenseVector(vec![0.875, 0.140625, -0.15625, 0.96875]),
        negative: VectorInput::DenseVector(vec![0.475, 0.440625, -0.25625, 0.36875]),
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(example = "context_pair_example")]
pub struct ContextPair {
    /// A positive vector
    #[validate]
    pub positive: VectorInput,

    /// Repel from this vector
    #[validate]
    pub negative: VectorInput,
}

impl ContextPair {
    pub fn iter(&self) -> impl Iterator<Item = &VectorInput> {
        std::iter::once(&self.positive).chain(std::iter::once(&self.negative))
    }
}

fn lookup_location_example() -> LookupLocation {
    LookupLocation {
        collection: "collection_b".to_string(),
        vector: Some("image_vector".to_string()),
        shard_key: Some(ShardKeySelector::ShardKey(ShardKey::from("region_1"))),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "lookup_location_example")]
pub struct WithLookup {
    /// Name of the collection to use for points lookup
    #[serde(rename = "collection")]
    pub collection_name: String,

    /// Options for specifying which payload to include (or not)
    #[serde(default = "default_with_payload")]
    pub with_payload: Option<WithPayloadInterface>,

    /// Options for specifying which vectors to include (or not)
    #[serde(alias = "with_vector")]
    #[serde(default)]
    pub with_vectors: Option<WithVector>,
}

const fn default_with_payload() -> Option<WithPayloadInterface> {
    Some(WithPayloadInterface::Bool(true))
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum WithLookupInterface {
    Collection(String),
    WithLookup(WithLookup),
}

/// Defines a location to use for looking up the vector.
/// Specifies collection and vector field name.
#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct LookupLocation {
    /// Name of the collection used for lookup
    pub collection: String,
    /// Optional name of the vector field within the collection.
    /// If not provided, the default vector field will be used.
    #[serde(default)]
    pub vector: Option<String>,

    /// Specify in which shards to look for the points, if not specified - look in all shards
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<ShardKeySelector>,
}

#[derive(Validate, Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
pub struct BaseGroupRequest {
    /// Payload field to group by, must be a string or number field.
    /// If the field contains more than 1 value, all values will be used for grouping.
    /// One point can be in multiple groups.
    #[schemars(length(min = 1))]
    pub group_by: JsonPath,

    /// Maximum amount of points to return per group
    #[validate(range(min = 1))]
    pub group_size: u32,

    /// Maximum amount of groups to return
    #[validate(range(min = 1))]
    pub limit: u32,

    /// Look for points in another collection using the group ids
    pub with_lookup: Option<WithLookupInterface>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Validate, Clone)]
pub struct SearchGroupsRequestInternal {
    /// Look for vectors closest to this
    #[validate]
    pub vector: NamedVectorStruct,

    /// Look only for points which satisfies this conditions
    #[validate]
    pub filter: Option<Filter>,

    /// Additional search params
    #[validate]
    pub params: Option<SearchParams>,

    /// Select which payload to return with the response. Default is false.
    pub with_payload: Option<WithPayloadInterface>,

    /// Options for specifying which vectors to include into response. Default is false.
    #[serde(default, alias = "with_vectors")]
    pub with_vector: Option<WithVector>,

    /// Define a minimal score threshold for the result.
    /// If defined, less similar results will not be returned.
    /// Score of the returned result might be higher or smaller than the threshold depending on the
    /// Distance function used. E.g. for cosine similarity only higher scores will be returned.
    pub score_threshold: Option<ScoreType>,

    #[serde(flatten)]
    #[validate]
    pub group_request: BaseGroupRequest,
}

/// Search request.
/// Holds all conditions and parameters for the search of most similar points by vector similarity
/// given the filtering restrictions.
#[derive(Deserialize, Serialize, JsonSchema, Validate, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct SearchRequestInternal {
    /// Look for vectors closest to this
    #[validate]
    pub vector: NamedVectorStruct,
    /// Look only for points which satisfies this conditions
    #[validate]
    pub filter: Option<Filter>,
    /// Additional search params
    #[validate]
    pub params: Option<SearchParams>,
    /// Max number of result to return
    #[serde(alias = "top")]
    #[validate(range(min = 1))]
    pub limit: usize,
    /// Offset of the first result to return.
    /// May be used to paginate results.
    /// Note: large offset values may cause performance issues.
    pub offset: Option<usize>,
    /// Select which payload to return with the response. Default is false.
    pub with_payload: Option<WithPayloadInterface>,
    /// Options for specifying which vectors to include into response. Default is false.
    #[serde(default, alias = "with_vectors")]
    pub with_vector: Option<WithVector>,
    /// Define a minimal score threshold for the result.
    /// If defined, less similar results will not be returned.
    /// Score of the returned result might be higher or smaller than the threshold depending on the
    /// Distance function used. E.g. for cosine similarity only higher scores will be returned.
    pub score_threshold: Option<ScoreType>,
}

#[derive(Validate, Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
pub struct QueryBaseGroupRequest {
    /// Payload field to group by, must be a string or number field.
    /// If the field contains more than 1 value, all values will be used for grouping.
    /// One point can be in multiple groups.
    #[schemars(length(min = 1))]
    pub group_by: JsonPath,

    /// Maximum amount of points to return per group. Default is 3.
    #[validate(range(min = 1))]
    pub group_size: Option<usize>,

    /// Maximum amount of groups to return. Default is 10.
    #[validate(range(min = 1))]
    pub limit: Option<usize>,

    /// Look for points in another collection using the group ids
    pub with_lookup: Option<WithLookupInterface>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Validate)]
pub struct QueryGroupsRequestInternal {
    /// Sub-requests to perform first. If present, the query will be performed on the results of the prefetch(es).
    #[validate]
    #[serde(default, with = "MaybeOneOrMany")]
    #[schemars(with = "MaybeOneOrMany<Prefetch>")]
    pub prefetch: Option<Vec<Prefetch>>,

    /// Query to perform. If missing without prefetches, returns points ordered by their IDs.
    #[validate]
    pub query: Option<QueryInterface>,

    /// Define which vector name to use for querying. If missing, the default vector is used.
    pub using: Option<String>,

    /// Filter conditions - return only those points that satisfy the specified conditions.
    #[validate]
    pub filter: Option<Filter>,

    /// Search params for when there is no prefetch
    #[validate]
    pub params: Option<SearchParams>,

    /// Return points with scores better than this threshold.
    pub score_threshold: Option<ScoreType>,

    /// Options for specifying which vectors to include into the response. Default is false.
    pub with_vector: Option<WithVector>,

    /// Options for specifying which payload to include or not. Default is false.
    pub with_payload: Option<WithPayloadInterface>,

    #[serde(flatten)]
    #[validate]
    pub group_request: QueryBaseGroupRequest,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Validate)]
pub struct QueryGroupsRequest {
    #[validate]
    #[serde(flatten)]
    pub search_group_request: QueryGroupsRequestInternal,

    pub shard_key: Option<ShardKeySelector>,
}
