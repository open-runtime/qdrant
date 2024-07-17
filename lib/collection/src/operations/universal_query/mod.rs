//! ## Universal query request types
//!
//! Provides a common interface for querying points in a collection
//!
//! Pipeline of type conversion:
//!
//! 1. `rest::QueryRequest`, `grpc::QueryPoints`: rest or grpc request. Used in API
//! 2. `CollectionQueryRequest`: Direct representation of the API request, but to be used as a single type. Created at API to enter ToC.
//! 3. `ShardQueryRequest`: same as the common request, but all point ids have been substituted with vectors. Created at Collection
//! 4. `QueryShardPoints`: to be used in the internal service. Created for RemoteShard, converts to and from ShardQueryRequest
//! 5. `PlannedQuery`: an easier-to-execute representation of a batch of [ShardQueryRequest]. Created in LocalShard

pub mod collection_query;
pub mod planned_query;
pub mod shard_query;
