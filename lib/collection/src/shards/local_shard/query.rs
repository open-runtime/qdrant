use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use api::rest::OrderByInterface;
use futures::future::BoxFuture;
use futures::FutureExt;
use segment::common::reciprocal_rank_fusion::rrf_scoring;
use segment::types::{Filter, HasIdCondition, ScoredPoint, WithPayloadInterface, WithVector};
use tokio::runtime::Handle;

use super::LocalShard;
use crate::collection_manager::segments_searcher::SegmentsSearcher;
use crate::operations::types::{
    CollectionError, CollectionResult, CoreSearchRequest, CoreSearchRequestBatch,
    QueryScrollRequestInternal,
};
use crate::operations::universal_query::planned_query::{
    MergePlan, PlannedQuery, RescoreParams, Source,
};
use crate::operations::universal_query::shard_query::{Fusion, ScoringQuery, ShardQueryResponse};

pub enum FetchedSource {
    Search(usize),
    Scroll(usize),
}

struct PrefetchResults {
    search_results: Vec<Vec<ScoredPoint>>,
    scroll_results: Vec<Vec<ScoredPoint>>,
}

impl PrefetchResults {
    fn new(search_results: Vec<Vec<ScoredPoint>>, scroll_results: Vec<Vec<ScoredPoint>>) -> Self {
        Self {
            search_results,
            scroll_results,
        }
    }

    fn get(&self, element: FetchedSource) -> CollectionResult<Cow<'_, Vec<ScoredPoint>>> {
        match element {
            FetchedSource::Search(idx) => self.search_results.get(idx).map(Cow::Borrowed),
            FetchedSource::Scroll(idx) => self.scroll_results.get(idx).map(Cow::Borrowed),
        }
        .ok_or_else(|| CollectionError::service_error("Expected a prefetched source to exist"))
    }
}

impl LocalShard {
    pub async fn do_planned_query(
        &self,
        request: PlannedQuery,
        search_runtime_handle: &Handle,
        timeout: Option<Duration>,
    ) -> CollectionResult<Vec<ShardQueryResponse>> {
        let start_time = std::time::Instant::now();
        let timeout = timeout.unwrap_or(self.shared_storage_config.search_timeout);

        let searches_f = self.do_search(
            Arc::new(CoreSearchRequestBatch {
                searches: request.searches,
            }),
            search_runtime_handle,
            Some(timeout),
        );

        let scrolls_f =
            self.query_scroll_batch(Arc::new(request.scrolls), search_runtime_handle, timeout);

        // execute both searches and scrolls concurrently
        let (search_results, scroll_results) = tokio::try_join!(searches_f, scrolls_f)?;
        let prefetch_holder = PrefetchResults::new(search_results, scroll_results);

        // decrease timeout by the time spent so far
        let timeout = timeout.saturating_sub(start_time.elapsed());

        let merge_futures = request.root_plans.into_iter().map(|merge_plan| {
            self.recurse_prefetch(
                merge_plan,
                &prefetch_holder,
                search_runtime_handle,
                timeout,
                0,
            )
        });

        let batched_scored_points = futures::future::try_join_all(merge_futures).await?;

        Ok(batched_scored_points)
    }

    /// Fetches the payload and/or vector if required. This will filter out points if they are deleted between search and retrieve.
    async fn fill_with_payload_or_vectors(
        &self,
        query_response: Vec<ScoredPoint>,
        with_payload: WithPayloadInterface,
        with_vector: WithVector,
    ) -> CollectionResult<Vec<ScoredPoint>> {
        if !with_payload.is_required() && !with_vector.is_enabled() {
            return Ok(query_response);
        }

        // ids to retrieve (deduplication happens in the searcher)
        let point_ids: Vec<_> = query_response
            .iter()
            .map(|scored_point| scored_point.id)
            .collect();

        // Collect retrieved records into a hashmap for fast lookup
        let records_map = SegmentsSearcher::retrieve(
            self.segments(),
            &point_ids,
            &(&with_payload).into(),
            &with_vector,
        )?;

        // It might be possible, that we won't find all records,
        // so we need to re-collect the results
        let query_response: Vec<_> = query_response
            .into_iter()
            .filter_map(|mut point| {
                records_map.get(&point.id).map(|record| {
                    point.payload.clone_from(&record.payload);
                    point.vector.clone_from(&record.vector);
                    point
                })
            })
            .collect();

        Ok(query_response)
    }

    fn recurse_prefetch<'shard, 'query>(
        &'shard self,
        merge_plan: MergePlan,
        prefetch_holder: &'query PrefetchResults,
        search_runtime_handle: &'shard Handle,
        timeout: Duration,
        depth: usize,
    ) -> BoxFuture<'query, CollectionResult<Vec<Vec<ScoredPoint>>>>
    where
        'shard: 'query,
    {
        async move {
            let max_len = merge_plan.sources.len();
            let mut cow_sources = Vec::with_capacity(max_len);

            // We need to preserve the order of the sources for some fusion strategies
            for source in merge_plan.sources.into_iter() {
                match source {
                    Source::SearchesIdx(idx) => {
                        cow_sources.push(prefetch_holder.get(FetchedSource::Search(idx))?)
                    }
                    Source::ScrollsIdx(idx) => {
                        cow_sources.push(prefetch_holder.get(FetchedSource::Scroll(idx))?)
                    }
                    Source::Prefetch(prefetch) => {
                        let merged = self
                            .recurse_prefetch(
                                prefetch,
                                prefetch_holder,
                                search_runtime_handle,
                                timeout,
                                depth + 1,
                            )
                            .await?
                            .into_iter()
                            .map(Cow::Owned);
                        cow_sources.extend(merged);
                    }
                }
            }

            // Rescore or return plain sources
            if let Some(rescore_params) = merge_plan.rescore_params {
                let rescored = self
                    .rescore(
                        cow_sources.into_iter(),
                        rescore_params,
                        search_runtime_handle,
                        timeout,
                    )
                    .await?;

                Ok(vec![rescored])
            } else {
                // The sources here are passed to the next layer without any extra processing.
                // It is either a query without prefetches, or a fusion request and the intermediate results are passed to the next layer.
                debug_assert_eq!(depth, 0);
                // TODO(universal-query): maybe there's a way to pass ownership of the prefetch_holder to avoid cloning with Cow::into_owned here
                Ok(cow_sources.into_iter().map(Cow::into_owned).collect())
            }
        }
        .boxed()
    }

    /// Rescore list of scored points
    async fn rescore<'a>(
        &self,
        sources: impl Iterator<Item = Cow<'a, Vec<ScoredPoint>>>,
        rescore_params: RescoreParams,
        search_runtime_handle: &Handle,
        timeout: Duration,
    ) -> CollectionResult<Vec<ScoredPoint>> {
        let RescoreParams {
            rescore,
            score_threshold,
            limit,
            with_vector,
            with_payload,
        } = rescore_params;

        match rescore {
            ScoringQuery::Fusion(Fusion::Rrf) => {
                let sources: Vec<_> = sources.map(Cow::into_owned).collect();

                let top_rrf = rrf_scoring(sources);

                let top_rrf: Vec<_> = if let Some(score_threshold) = score_threshold {
                    top_rrf
                        .into_iter()
                        .take_while(|point| point.score >= score_threshold)
                        .take(limit)
                        .collect()
                } else {
                    top_rrf.into_iter().take(limit).collect()
                };

                let filled_top_rrf = self
                    .fill_with_payload_or_vectors(top_rrf, with_payload, with_vector)
                    .await?;

                Ok(filled_top_rrf)
            }
            ScoringQuery::OrderBy(order_by) => {
                // create single scroll request for rescoring query
                let filter = filter_with_sources_ids(sources);

                // Note: score_threshold is not used in this case, as all results will have same score,
                // but different order_value
                let scroll_request = QueryScrollRequestInternal {
                    limit,
                    filter: Some(filter),
                    with_payload,
                    with_vector,
                    order_by: Some(OrderByInterface::Struct(order_by)),
                };

                self.query_scroll_batch(
                    Arc::new(vec![scroll_request]),
                    search_runtime_handle,
                    timeout,
                )
                .await?
                .pop()
                .ok_or_else(|| {
                    CollectionError::service_error(
                        "Rescoring with order-by query didn't return expected batch of results",
                    )
                })
            }
            ScoringQuery::Vector(query_enum) => {
                // create single search request for rescoring query
                let filter = filter_with_sources_ids(sources);

                let search_request = CoreSearchRequest {
                    query: query_enum,
                    filter: Some(filter),
                    params: None,
                    limit,
                    offset: 0,
                    with_payload: Some(with_payload),
                    with_vector: Some(with_vector),
                    score_threshold,
                };

                let rescoring_core_search_request = CoreSearchRequestBatch {
                    searches: vec![search_request],
                };

                self.do_search(
                    Arc::new(rescoring_core_search_request),
                    search_runtime_handle,
                    Some(timeout),
                )
                .await?
                // One search request is sent. We expect only one result
                .pop()
                .ok_or_else(|| {
                    CollectionError::service_error(
                        "Rescoring with vector(s) query didn't return expected batch of results",
                    )
                })
            }
        }
    }
}

/// Extracts point ids from sources, and creates a filter to only include those ids.
fn filter_with_sources_ids<'a>(sources: impl Iterator<Item = Cow<'a, Vec<ScoredPoint>>>) -> Filter {
    let mut point_ids = HashSet::new();

    for source in sources {
        for point in source.iter() {
            point_ids.insert(point.id);
        }
    }

    // create filter for target point ids
    Filter::new_must(segment::types::Condition::HasId(HasIdCondition::from(
        point_ids,
    )))
}
